#!/usr/bin/env bash
# Standalone installer: this file can be served unchanged from any HTTPS host.
set -euo pipefail

fail() { printf 'GDS: %s\n' "$*" >&2; exit 1; }

usage() {
    cat <<'HELP'
Install GDS Files for Linux.

Usage: bash install.sh [--with-portal] [--version TAG] [--nix]
  --with-portal  Also install the optional file chooser portal backend.
  --version TAG Install a specific GitHub release (for example v0.0.1).
  --nix         Use the existing Nix installation instead of native packages.
  --help        Show this help.

Native packages: x86_64 Arch Linux, Debian 13, Fedora 43.
Nix: x86_64-linux and aarch64-linux; selected automatically on NixOS.
HELP
}

download() {
    curl --fail --show-error --silent --location --retry 3 \
        --proto '=https' --tlsv1.2 "$1" --output "$2"
}

main() {
    local with_portal=false use_nix=false version=''
    while (($#)); do
        case "$1" in
            --with-portal) with_portal=true; shift ;;
            --nix) use_nix=true; shift ;;
            --version)
                (($# >= 2)) || fail '--version requires a release tag.'
                version=$2
                [[ $version =~ ^[a-zA-Z0-9][a-zA-Z0-9._-]*$ ]] || fail 'Invalid release tag.'
                shift 2 ;;
            --help|-h) usage; return ;;
            *) fail "Unknown option: $1 (use --help)." ;;
        esac
    done
    [[ $(uname -s) == Linux ]] || fail 'This installer supports Linux only.'
    local architecture
    architecture=$(uname -m)
    local ID='' VERSION_ID=''
    [[ -r /etc/os-release ]] || fail 'Cannot identify this system: /etc/os-release is missing.'
    # shellcheck disable=SC1091
    . /etc/os-release
    [[ $ID != nixos ]] || use_nix=true

    if $use_nix; then
        case "$architecture" in
            x86_64|aarch64) ;;
            *) fail "Unsupported Nix architecture: $architecture." ;;
        esac
        command -v nix >/dev/null || fail 'Install Nix first, then run this installer again.'
        local flake="github:GanDesktop/gds${version:+/$version}"
        local -a packages=("$flake#gds-files")
        if $with_portal; then packages+=("$flake#xdg-desktop-portal-gds"); fi
        nix --extra-experimental-features 'nix-command flakes' profile add "${packages[@]}"
    else
        [[ $architecture == x86_64 ]] || fail "No native release package for $architecture. Use --nix with an existing Nix installation."
        local files_pattern portal_pattern manager
        case "$ID:$VERSION_ID" in
            arch:*)
                files_pattern='gds-files-git-*-x86_64.pkg.tar.zst'
                portal_pattern='xdg-desktop-portal-gds-git-*-x86_64.pkg.tar.zst'
                manager=pacman ;;
            debian:13)
                files_pattern='gds-files_*_amd64.deb'
                portal_pattern='xdg-desktop-portal-gds_*_amd64.deb'
                manager=apt-get ;;
            fedora:43)
                files_pattern='gds-files-*.fc43.x86_64.rpm'
                portal_pattern='xdg-desktop-portal-gds-*.fc43.x86_64.rpm'
                manager=dnf ;;
            *) fail "No native release package for $ID $VERSION_ID. Supported: Arch Linux, Debian 13, Fedora 43; or use --nix." ;;
        esac
        local tool
        for tool in curl sha256sum "$manager"; do
            command -v "$tool" >/dev/null || fail "Required command not found: $tool. Install it and retry."
        done
        local -a elevate=()
        if ((EUID != 0)); then
            command -v sudo >/dev/null || fail 'sudo is required to install system packages.'
            elevate=(sudo)
        fi

        local release_url base_url
        base_url='https://github.com/GanDesktop/gds/releases'
        if [[ -z $version ]]; then
            # Resolve once so checksums and packages always come from the same release.
            release_url=$(curl --fail --show-error --silent --location --retry 3 \
                --proto '=https' --tlsv1.2 --output /dev/null --write-out '%{url_effective}' "$base_url/latest")
            [[ $release_url == "$base_url/tag/"* ]] || fail 'No stable release is available yet.'
            version=${release_url#"$base_url/tag/"}
            [[ $version =~ ^[a-zA-Z0-9][a-zA-Z0-9._-]*$ ]] || fail 'Unsupported release tag; use --version TAG.'
        fi
        base_url="$base_url/download/$version"
        printf 'Installing GDS %s for %s %s (%s)...\n' "$version" "$ID" "$VERSION_ID" "$architecture"
        # A global holds the cleanup path until the EXIT trap runs after main returns.
        GDS_INSTALL_TMP=$(mktemp -d)
        trap 'rm -rf -- "$GDS_INSTALL_TMP"' EXIT
        chmod 755 "$GDS_INSTALL_TMP"
        download "$base_url/SHA256SUMS" "$GDS_INSTALL_TMP/SHA256SUMS" || \
            fail 'Release checksums are unavailable. The release may still be building; retry when its assets appear.'
        local pattern checksum filename extra
        local -a patterns=("$files_pattern") selected=() hashes=() matches=() match_hashes=()
        if $with_portal; then patterns+=("$portal_pattern"); fi
        for pattern in "${patterns[@]}"; do
            matches=(); match_hashes=()
            while read -r checksum filename extra; do
                # Intentional glob match against package names from our release workflow.
                # shellcheck disable=SC2053
                [[ $filename == $pattern ]] || continue
                [[ $checksum =~ ^[[:xdigit:]]{64}$ && $filename =~ ^[a-zA-Z0-9][a-zA-Z0-9._+-]*$ && -z $extra ]] || \
                    fail 'Invalid checksum entry.'
                matches+=("$filename"); match_hashes+=("$checksum")
            done < "$GDS_INSTALL_TMP/SHA256SUMS"
            ((${#matches[@]} == 1)) || fail "Expected one package matching $pattern in $version; found ${#matches[@]}."
            selected+=("${matches[0]}"); hashes+=("${match_hashes[0]}")
        done
        local index
        local -a package_paths=()
        for index in "${!selected[@]}"; do
            filename=${selected[$index]}
            download "$base_url/$filename" "$GDS_INSTALL_TMP/$filename"
            (cd "$GDS_INSTALL_TMP" && printf '%s  %s\n' "${hashes[$index]}" "$filename" | sha256sum --check --status) || \
                fail "Checksum verification failed: $filename"
            chmod 644 "$GDS_INSTALL_TMP/$filename"
            package_paths+=("$GDS_INSTALL_TMP/$filename")
        done
        case "$manager" in
            pacman) "${elevate[@]}" pacman -U --noconfirm "${package_paths[@]}" ;;
            apt-get)
                "${elevate[@]}" apt-get update
                "${elevate[@]}" apt-get install -y "${package_paths[@]}" ;;
            dnf) "${elevate[@]}" dnf install -y "${package_paths[@]}" ;;
        esac
    fi
    printf '\nGDS Files installed. Open it from your application menu or run: gds-files\n'
    if $with_portal; then
        printf 'Portal installed. Configure your desktop to use it: https://github.com/GanDesktop/gds/blob/main/docs/apps/files/fedora.md#optional-gds-portal\n'
    fi
}

# Keep execution at the end so this script also works when piped into bash.
main "$@"
