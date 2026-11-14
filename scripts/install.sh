#!/usr/bin/env bash
# Standalone GDS installer: safe to pipe via `curl -fsSL ... | bash`.
set -euo pipefail

DEFAULT_DIST_URL='https://pkgs.gandesktop.dev'
DEFAULT_FLAKE_URI='github:GanDesktop/gds'

GDS_INSTALL_TMP=''

cleanup_tmp() {
    if [[ -n ${GDS_INSTALL_TMP:-} ]]; then
        rm -rf -- "$GDS_INSTALL_TMP"
    fi
}
trap cleanup_tmp EXIT

fail() {
    printf 'GDS: %s\n' "$*" >&2
    exit 1
}

warn() {
    printf 'GDS: Warning: %s\n' "$*" >&2
}

usage() {
    cat <<'HELP'
Install, upgrade, or remove GDS Files (and optional GDS Portal) for Linux.

Usage:
  curl -fsSL https://gds.gandesktop.dev/install.sh | bash
  curl -fsSL https://gds.gandesktop.dev/install.sh | bash -s -- [OPTIONS]

Options:
  -y, --yes, --non-interactive  Skip interactive prompts and apply defaults
      --with-portal             Include optional xdg-desktop-portal-gds backend
      --without-portal          Exclude xdg-desktop-portal-gds even if installed
      --reinstall               Reinstall even when already at the target version
      --distro <ID:VER>         Override native target (e.g. ubuntu:24.04, arch:rolling)
      --version <TAG>           Install a specific release tag (e.g. v0.0.2)
      --nix                     Use the existing Nix installation via flake profile
      --dry-run                 Probe system, inspect installed versions, and print plan only
      --uninstall               Remove installed GDS packages (keeps user config and state)
  -h, --help                    Show this help

Native packages (x86_64):
  Tier 1 (exact): Arch Linux, Debian 13, Ubuntu 24.04 / 26.04 LTS, Fedora 43 / 44
  Tier 2 (mapped): Ubuntu interim releases & Mint/Pop!/elementary/Zorin derivatives,
                   Debian derivatives (LMDE), Fedora derivatives (Nobara), Arch
                   derivatives (CachyOS, EndeavourOS, Garuda)
Nix: x86_64-linux and aarch64-linux (selected automatically on NixOS)
HELP
}

word_in_list() {
    local word=$1 list=$2
    [[ " $list " == *" $word "* ]]
}

strip_trailing_slashes() {
    local value=$1
    while [[ $value == */ && ${#value} -gt 1 ]]; do
        value=${value%/}
    done
    printf '%s' "$value"
}

validate_tag() {
    local tag=$1
    [[ $tag =~ ^[a-zA-Z0-9][a-zA-Z0-9._-]*$ ]]
}

init_tmp_dir() {
    if [[ -n $GDS_INSTALL_TMP ]]; then
        return 0
    fi
    # Stage under /tmp so APT's unprivileged _apt user can traverse parent directories.
    GDS_INSTALL_TMP=$(mktemp -d /tmp/gds-install.XXXXXX)
    chmod 755 "$GDS_INSTALL_TMP"
}

download_url() {
    local url=$1 output=$2
    local -a curl_args=(
        --fail --show-error --silent --location --retry 3
        --connect-timeout 10 --max-time 120
    )
    if [[ ${GDS_ALLOW_FILE_URL:-0} != 1 ]]; then
        curl_args+=(--proto '=https' --tlsv1.2)
    fi
    curl "${curl_args[@]}" "$url" --output "$output"
}

unquote_os_release_value() {
    local val=$1
    if [[ $val == \"*\" && ${#val} -ge 2 ]]; then
        val=${val:1:${#val}-2}
        val=${val//\\\"/\"}
        val=${val//\\\$/\$}
        val=${val//\\\`/\`}
        val=${val//\\\\/\\}
    elif [[ $val == \'*\' && ${#val} -ge 2 ]]; then
        val=${val:1:${#val}-2}
    fi
    printf '%s' "$val"
}

read_os_release() {
    os_id=''
    os_id_like=''
    os_version_id=''
    os_version_codename=''
    os_ubuntu_codename=''
    os_debian_codename=''
    os_variant_id=''
    os_pretty_name=''

    local os_release_file=''
    if [[ -n ${GDS_OS_RELEASE:-} ]]; then
        [[ -r $GDS_OS_RELEASE ]] || fail "Cannot identify system: cannot read $GDS_OS_RELEASE."
        os_release_file=$GDS_OS_RELEASE
    elif [[ -e /etc/os-release || -L /etc/os-release ]]; then
        [[ -r /etc/os-release ]] || fail 'Cannot identify system: /etc/os-release exists but is unreadable.'
        os_release_file='/etc/os-release'
    elif [[ -e /usr/lib/os-release || -L /usr/lib/os-release ]]; then
        [[ -r /usr/lib/os-release ]] || fail 'Cannot identify system: /usr/lib/os-release exists but is unreadable.'
        os_release_file='/usr/lib/os-release'
    else
        fail 'Cannot identify system: neither /etc/os-release nor /usr/lib/os-release exists.'
    fi

    local line key raw_val val
    while IFS= read -r line || [[ -n $line ]]; do
        line=${line%$'\r'}
        [[ -n $line && $line != \#* ]] || continue
        if [[ $line =~ ^([A-Z0-9_]+)=(.*)$ ]]; then
            key=${BASH_REMATCH[1]}
            raw_val=${BASH_REMATCH[2]}
            val=$(unquote_os_release_value "$raw_val")
            case "$key" in
                ID) os_id=$val ;;
                ID_LIKE) os_id_like=$val ;;
                VERSION_ID) os_version_id=$val ;;
                VERSION_CODENAME) os_version_codename=$val ;;
                UBUNTU_CODENAME) os_ubuntu_codename=$val ;;
                DEBIAN_CODENAME) os_debian_codename=$val ;;
                VARIANT_ID) os_variant_id=$val ;;
                PRETTY_NAME) os_pretty_name=$val ;;
            esac
        fi
    done < "$os_release_file"
}

map_ubuntu_codename() {
    case "$1" in
        jammy) printf '22.04' ;;
        lunar) printf '23.04' ;;
        mantic) printf '23.10' ;;
        noble) printf '24.04' ;;
        oracular) printf '24.10' ;;
        plucky) printf '25.04' ;;
        questing) printf '25.10' ;;
        resolute) printf '26.04' ;;
        *) return 1 ;;
    esac
}

map_debian_codename() {
    case "$1" in
        bullseye) printf '11' ;;
        bookworm) printf '12' ;;
        trixie) printf '13' ;;
        forky) printf '14' ;;
        duke) printf '15' ;;
        sid|unstable|testing) printf 'testing' ;;
        *) return 1 ;;
    esac
}

read_debian_version_file() {
    local path=${GDS_DEBIAN_VERSION:-/etc/debian_version}
    [[ -r $path ]] || return 1
    local raw
    IFS= read -r raw < "$path" || true
    raw=${raw%$'\r'}
    raw=${raw//[[:space:]]/}
    if [[ $raw =~ ^([0-9]+) ]]; then
        printf '%s' "${BASH_REMATCH[1]}"
        return 0
    fi
    local codename=${raw%%/*}
    map_debian_codename "$codename"
}

# Print -1 if v1 < v2, 0 if v1 == v2, 1 if v1 > v2 (handles dotted integers, -pre, & "testing").
compare_baseline_versions() {
    local v1=$1 v2=$2
    if [[ $v1 == "$v2" ]]; then
        printf '0'
        return 0
    fi
    if [[ $v1 == testing ]]; then
        printf '1'
        return 0
    fi
    if [[ $v2 == testing ]]; then
        printf '%s' '-1'
        return 0
    fi

    local base1=${v1%%-*} base2=${v2%%-*}
    local pre1='' pre2=''
    if [[ $v1 == *-* ]]; then pre1=${v1#*-}; fi
    if [[ $v2 == *-* ]]; then pre2=${v2#*-}; fi

    local -a a1 a2
    IFS=. read -r -a a1 <<< "$base1"
    IFS=. read -r -a a2 <<< "$base2"
    local max_len=${#a1[@]}
    if ((${#a2[@]} > max_len)); then
        max_len=${#a2[@]}
    fi
    local i p1 p2
    for ((i = 0; i < max_len; i++)); do
        p1=${a1[i]:-0}
        p2=${a2[i]:-0}
        if [[ $p1 =~ ^[0-9]+$ && $p2 =~ ^[0-9]+$ ]]; then
            p1=$((10#$p1))
            p2=$((10#$p2))
            if ((p1 < p2)); then
                printf '%s' '-1'
                return 0
            elif ((p1 > p2)); then
                printf '1'
                return 0
            fi
        else
            if [[ $p1 < $p2 ]]; then
                printf '%s' '-1'
                return 0
            elif [[ $p1 > $p2 ]]; then
                printf '1'
                return 0
            fi
        fi
    done

    if [[ -z $pre1 && -n $pre2 ]]; then
        printf '1'
    elif [[ -n $pre1 && -z $pre2 ]]; then
        printf '%s' '-1'
    elif [[ $pre1 < $pre2 ]]; then
        printf '%s' '-1'
    elif [[ $pre1 > $pre2 ]]; then
        printf '1'
    else
        printf '0'
    fi
}

compare_package_versions() {
    local cur=$1 tgt=$2
    if [[ $cur == "$tgt" ]]; then
        printf '0'
        return 0
    fi
    case "$backend" in
        deb)
            if dpkg --compare-versions "$cur" eq "$tgt"; then
                printf '0'
            elif dpkg --compare-versions "$cur" lt "$tgt"; then
                printf '%s' '-1'
            else
                printf '1'
            fi ;;
        rpm)
            local cmp
            cmp=$(rpm --eval "%{lua: print(rpm.vercmp('$cur', '$tgt'))}" 2>/dev/null || printf '0')
            if ((cmp < 0)); then
                printf '%s' '-1'
            elif ((cmp > 0)); then
                printf '1'
            else
                printf '0'
            fi ;;
        pacman)
            local cmp
            cmp=$(vercmp "$cur" "$tgt" 2>/dev/null || printf '0')
            if ((cmp < 0)); then
                printf '%s' '-1'
            elif ((cmp > 0)); then
                printf '1'
            else
                printf '0'
            fi ;;
        nix)
            compare_baseline_versions "$cur" "$tgt" ;;
    esac
}

parse_args() {
    opt_non_interactive=false
    if [[ ${GDS_NONINTERACTIVE:-} == 1 || ${GDS_NONINTERACTIVE:-} == true ]]; then
        opt_non_interactive=true
    fi
    opt_portal='auto'
    opt_reinstall=false
    opt_distro=''
    opt_version=''
    opt_nix=false
    opt_dry_run=false
    opt_uninstall=false

    while (($#)); do
        case "$1" in
            -y|--yes|--non-interactive)
                opt_non_interactive=true
                shift ;;
            --with-portal)
                [[ $opt_portal != exclude ]] || fail '--with-portal cannot be combined with --without-portal.'
                opt_portal='include'
                shift ;;
            --without-portal)
                [[ $opt_portal != include ]] || fail '--without-portal cannot be combined with --with-portal.'
                opt_portal='exclude'
                shift ;;
            --reinstall)
                opt_reinstall=true
                shift ;;
            --distro=*)
                opt_distro=${1#*=}
                [[ $opt_distro =~ ^[a-z0-9-]+:[a-z0-9._-]+$ ]] || fail "Invalid --distro value '$opt_distro' (expected ID:VER, e.g. ubuntu:24.04)."
                shift ;;
            --distro)
                (($# >= 2)) || fail '--distro requires an ID:VER argument (e.g. ubuntu:24.04).'
                opt_distro=$2
                [[ $opt_distro =~ ^[a-z0-9-]+:[a-z0-9._-]+$ ]] || fail "Invalid --distro value '$opt_distro' (expected ID:VER, e.g. ubuntu:24.04)."
                shift 2 ;;
            --version=*)
                opt_version=${1#*=}
                validate_tag "$opt_version" || fail 'Invalid release tag.'
                shift ;;
            --version)
                (($# >= 2)) || fail '--version requires a release tag.'
                opt_version=$2
                validate_tag "$opt_version" || fail 'Invalid release tag.'
                shift 2 ;;
            --nix)
                opt_nix=true
                shift ;;
            --dry-run)
                opt_dry_run=true
                shift ;;
            --uninstall)
                opt_uninstall=true
                shift ;;
            -h|--help)
                usage
                exit 0 ;;
            *)
                fail "Unknown option: $1 (use --help)." ;;
        esac
    done

    if $opt_nix && [[ -n $opt_distro ]]; then
        fail '--nix cannot be combined with --distro.'
    fi

    dist_url=$(strip_trailing_slashes "${GDS_DIST_URL:-$DEFAULT_DIST_URL}")
    flake_uri=$(strip_trailing_slashes "${GDS_FLAKE_URI:-$DEFAULT_FLAKE_URI}")
}

probe_platform() {
    [[ $(uname -s) == Linux ]] || fail 'This installer supports Linux only.'

    local raw_arch
    raw_arch=$(uname -m)
    case "$raw_arch" in
        x86_64|amd64) host_arch='x86_64' ;;
        aarch64|arm64) host_arch='aarch64' ;;
        *) host_arch=$raw_arch ;;
    esac

    if [[ $host_arch == x86_64 || $host_arch == aarch64 ]] && command -v getconf >/dev/null 2>&1; then
        local long_bit
        long_bit=$(getconf LONG_BIT 2>/dev/null || printf '64')
        [[ $long_bit == 64 ]] || fail "Unsupported ${long_bit}-bit userland on ${host_arch} kernel; 64-bit userland is required."
    fi

    read_os_release

    use_nix=false
    if $opt_nix || { [[ $os_id == nixos && -z $opt_distro ]]; }; then
        use_nix=true
    fi

    manjaro_warn=false
    tier_kind='Tier-1'

    if $use_nix; then
        case "$host_arch" in
            x86_64|aarch64) ;;
            *) fail "Unsupported Nix architecture: $host_arch." ;;
        esac
        command -v nix >/dev/null 2>&1 || fail 'Install Nix first, then run this installer again.'
        backend='nix'
        manager='nix'
        upstream_distro='nixos'
        upstream_ver='rolling'
        resolved_ver='rolling'
        pkg_arch="${host_arch}-linux"
        tier_kind='Tier-1'
        return 0
    fi

    local ostree_marker=${GDS_OSTREE_BOOTED:-/run/ostree-booted}
    if [[ -e $ostree_marker ]]; then
        fail 'Immutable OSTree systems (Fedora Silverblue/Kinoite/Bazzite, etc.) cannot use dnf directly. Pass --nix if Nix is installed.'
    fi
    case "$os_variant_id" in
        silverblue|kinoite|sericea|onyx|bazzite|iot|coreos)
            fail "Immutable Fedora variant '$os_variant_id' cannot use dnf directly. Pass --nix if Nix is installed." ;;
    esac

    case "$host_arch" in
        x86_64|aarch64) ;;
        *) fail "Unsupported architecture: $host_arch." ;;
    esac

    if [[ -n $opt_distro ]]; then
        local manual_id=${opt_distro%%:*}
        local manual_ver=${opt_distro#*:}
        tier_kind='Manual'
        case "$manual_id" in
            arch)
                backend='pacman'; manager='pacman'; upstream_distro='arch'; upstream_ver='rolling' ;;
            debian)
                backend='deb'; manager='apt-get'; upstream_distro='debian'
                if ! upstream_ver=$(map_debian_codename "$manual_ver" 2>/dev/null); then
                    upstream_ver=${manual_ver%%.*}
                fi ;;
            ubuntu)
                backend='deb'; manager='apt-get'; upstream_distro='ubuntu'
                if ! upstream_ver=$(map_ubuntu_codename "$manual_ver" 2>/dev/null); then
                    upstream_ver=$manual_ver
                fi ;;
            fedora)
                backend='rpm'; manager='dnf'; upstream_distro='fedora'; upstream_ver=${manual_ver%%.*} ;;
            *)
                fail "Unsupported --distro target '$opt_distro' (expected arch:rolling, debian:<ver>, ubuntu:<ver>, or fedora:<ver>)." ;;
        esac
    else
        case "$os_id" in
            arch)
                backend='pacman'; manager='pacman'; upstream_distro='arch'; upstream_ver='rolling'; tier_kind='Tier-1' ;;
            debian)
                backend='deb'; manager='apt-get'; upstream_distro='debian'; tier_kind='Tier-1'
                if [[ -n $os_version_id ]]; then
                    upstream_ver=${os_version_id%%.*}
                elif [[ -n $os_version_codename ]] && upstream_ver=$(map_debian_codename "$os_version_codename" 2>/dev/null); then
                    :
                elif upstream_ver=$(read_debian_version_file 2>/dev/null); then
                    :
                else
                    fail 'Cannot determine Debian version from os-release or /etc/debian_version. Pass --distro debian:13.'
                fi ;;
            ubuntu)
                backend='deb'; manager='apt-get'; upstream_distro='ubuntu'; tier_kind='Tier-1'
                if [[ -n $os_version_id ]]; then
                    upstream_ver=$os_version_id
                elif [[ -n $os_version_codename ]] && upstream_ver=$(map_ubuntu_codename "$os_version_codename" 2>/dev/null); then
                    :
                else
                    fail 'Cannot determine Ubuntu version from os-release. Pass --distro ubuntu:24.04.'
                fi ;;
            fedora)
                backend='rpm'; manager='dnf'; upstream_distro='fedora'; tier_kind='Tier-1'
                upstream_ver=${os_version_id%%.*}
                [[ -n $upstream_ver ]] || fail 'Cannot determine Fedora version from os-release. Pass --distro fedora:43.' ;;
            *)
                if [[ -n $os_ubuntu_codename ]] || { word_in_list ubuntu "$os_id_like" && [[ -n $os_version_codename ]]; }; then
                    local ucode=${os_ubuntu_codename:-$os_version_codename}
                    backend='deb'; manager='apt-get'; upstream_distro='ubuntu'; tier_kind='Tier-2'
                    if ! upstream_ver=$(map_ubuntu_codename "$ucode" 2>/dev/null); then
                        fail "Unrecognized Ubuntu base codename '$ucode' for ${os_pretty_name:-$os_id}. Pass --distro ubuntu:24.04 (or ubuntu:26.04)."
                    fi
                elif [[ -n $os_debian_codename ]] || { word_in_list debian "$os_id_like" && ! word_in_list ubuntu "$os_id_like"; }; then
                    backend='deb'; manager='apt-get'; upstream_distro='debian'; tier_kind='Tier-2'
                    local dcode=${os_debian_codename:-$os_version_codename}
                    if [[ -n $dcode ]] && upstream_ver=$(map_debian_codename "$dcode" 2>/dev/null); then
                        :
                    elif upstream_ver=$(read_debian_version_file 2>/dev/null); then
                        :
                    else
                        fail "Cannot determine Debian base version for ${os_pretty_name:-$os_id}. Pass --distro debian:13."
                    fi
                elif word_in_list fedora "$os_id_like" && command -v rpm >/dev/null 2>&1; then
                    local rpm_fedora
                    rpm_fedora=$(rpm --eval '%{fedora}' 2>/dev/null || true)
                    [[ $rpm_fedora =~ ^[0-9]+$ ]] || fail "Cannot evaluate Fedora upstream release via rpm macro for ${os_pretty_name:-$os_id}. Pass --distro fedora:43."
                    backend='rpm'; manager='dnf'; upstream_distro='fedora'; upstream_ver=$rpm_fedora; tier_kind='Tier-2'
                elif word_in_list arch "$os_id_like" && command -v pacman >/dev/null 2>&1; then
                    [[ $os_id != steamos ]] || fail 'SteamOS immutable root is not supported via pacman -U. Pass --nix if Nix is installed.'
                    if [[ $os_id == manjaro ]]; then
                        manjaro_warn=true
                    fi
                    backend='pacman'; manager='pacman'; upstream_distro='arch'; upstream_ver='rolling'; tier_kind='Tier-2'
                else
                    fail "Unsupported system: ${os_pretty_name:-${os_id:-unknown}} ${os_version_id}. Supported: Arch Linux, Debian 13+, Ubuntu 24.04+, Fedora 43+, NixOS, and compatible derivatives (or use --distro / --nix)."
                fi ;;
        esac
    fi

    resolved_ver=$upstream_ver

    case "$backend" in
        deb)
            command -v dpkg >/dev/null 2>&1 || fail 'Required command not found: dpkg.'
            command -v dpkg-query >/dev/null 2>&1 || fail 'Required command not found: dpkg-query.'
            case "$host_arch" in
                x86_64) pkg_arch='amd64' ;;
                aarch64) pkg_arch='arm64' ;;
            esac
            local dpkg_arch
            dpkg_arch=$(dpkg --print-architecture)
            [[ $dpkg_arch == "$pkg_arch" ]] || fail "Architecture mismatch: host is $host_arch ($pkg_arch), but dpkg reports '$dpkg_arch'." ;;
        rpm)
            command -v rpm >/dev/null 2>&1 || fail 'Required command not found: rpm.'
            pkg_arch=$host_arch
            local rpm_arch
            rpm_arch=$(rpm --eval '%{_arch}' 2>/dev/null || true)
            [[ -z $rpm_arch || $rpm_arch == "$pkg_arch" ]] || fail "Architecture mismatch: host is $host_arch, but rpm reports '$rpm_arch'." ;;
        pacman)
            pkg_arch=$host_arch
            if command -v pacman-conf >/dev/null 2>&1; then
                local pac_arch
                pac_arch=$(pacman-conf Architecture 2>/dev/null || printf 'auto')
                pac_arch=${pac_arch%%$'\n'*}
                if [[ -n $pac_arch && $pac_arch != auto && $pac_arch != "$pkg_arch" ]]; then
                    fail "Architecture mismatch: host is $host_arch, but pacman reports '$pac_arch'."
                fi
            fi ;;
    esac

    if ! $opt_uninstall; then
        for req in curl sha256sum "$manager"; do
            command -v "$req" >/dev/null 2>&1 || fail "Required command not found: $req. Install it and retry."
        done
    else
        command -v "$manager" >/dev/null 2>&1 || fail "Required command not found: $manager. Install it and retry."
    fi

    if ! $opt_dry_run && ((EUID != 0)); then
        command -v sudo >/dev/null 2>&1 || fail 'sudo is required for system package changes.'
    fi

    if $manjaro_warn && ! $opt_uninstall && ! $opt_dry_run && $opt_non_interactive && [[ -z $opt_distro ]]; then
        fail 'Manjaro repositories lag behind Arch Linux rolling releases, which can cause ABI mismatches. Run interactively to confirm or pass --distro arch:rolling --yes.'
    fi
}

query_installed_pkg() {
    local pkg=$1
    case "$backend" in
        deb)
            local out status ver
            out=$(dpkg-query -W -f='${db:Status-Abbrev}\t${Version}\n' "$pkg" 2>/dev/null || true)
            IFS=$'\t' read -r status ver <<< "$out"
            if [[ $status == ii* && -n $ver ]]; then
                printf '%s\t%s' "$pkg" "$ver"
                return 0
            fi
            return 1 ;;
        rpm)
            local ver
            if ver=$(rpm -q --qf '%{VERSION}-%{RELEASE}\n' "$pkg" 2>/dev/null); then
                [[ -n $ver ]] || return 1
                printf '%s\t%s' "$pkg" "$ver"
                return 0
            fi
            return 1 ;;
        pacman)
            local out ver
            if out=$(pacman -Q "$pkg" 2>/dev/null); then
                ver=${out#* }
                printf '%s\t%s' "$pkg" "$ver"
                return 0
            elif out=$(pacman -Q "${pkg}-git" 2>/dev/null); then
                ver=${out#* }
                printf '%s-git\t%s' "$pkg" "$ver"
                return 0
            fi
            return 1 ;;
        nix)
            local json store_path ver
            json=$(nix --extra-experimental-features 'nix-command flakes' profile list --json 2>/dev/null || printf '')
            store_path=$(printf '%s' "$json" | grep -oE "/nix/store/[0-9a-z]+-${pkg}-[0-9]"'[^]"[:space:],}]*' | head -n 1 || true)
            if [[ -n $store_path ]]; then
                ver=${store_path#*-"${pkg}"-}
                printf '%s\t%s' "$store_path" "$ver"
                return 0
            fi
            return 1 ;;
    esac
}

classify_component_state() {
    local prefix=$1 pkg=$2 tgt_ver=$3
    local cur_info cur_name='' cur_ver='' state='fresh'

    if cur_info=$(query_installed_pkg "$pkg"); then
        IFS=$'\t' read -r cur_name cur_ver <<< "$cur_info"
        if [[ $cur_name == "${pkg}-git" && $backend == pacman ]]; then
            state='upgrade'
        else
            local cmp
            cmp=$(compare_package_versions "$cur_ver" "$tgt_ver")
            if [[ $cmp == -1 ]]; then
                state='upgrade'
            elif [[ $cmp == 0 ]]; then
                state='same'
            else
                state='downgrade'
            fi
        fi
    fi

    printf -v "${prefix}_cur_pkg" '%s' "$cur_name"
    printf -v "${prefix}_cur_ver" '%s' "$cur_ver"
    printf -v "${prefix}_state" '%s' "$state"
}

open_tty_fd() {
    if [[ -n ${GDS_TTY:-} ]]; then
        [[ -r $GDS_TTY ]] || fail "Cannot read interactive input from GDS_TTY=$GDS_TTY."
        exec 3< "$GDS_TTY"
    elif [[ -t 1 ]] && ( : </dev/tty ) 2>/dev/null; then
        exec 3< /dev/tty
    else
        fail 'Interactive terminal is not available. Pass -y (--yes) for non-interactive installation.'
    fi
}

close_tty_fd() {
    exec 3<&-
}

confirm_tty() {
    local prompt=$1 reply=''
    printf '%s' "$prompt"
    if ! IFS= read -r reply <&3; then
        return 1
    fi
    case "$reply" in
        [yY]|[yY][eE][sS]) return 0 ;;
        *) return 1 ;;
    esac
}

run_uninstall() {
    local files_info portal_info files_cur='' portal_cur=''
    local -a remove_pkgs=() display_pkgs=()
    if files_info=$(query_installed_pkg 'gds-files'); then
        IFS=$'\t' read -r files_cur _ <<< "$files_info"
        remove_pkgs+=("$files_cur")
        display_pkgs+=("gds-files")
    fi
    if portal_info=$(query_installed_pkg 'xdg-desktop-portal-gds'); then
        IFS=$'\t' read -r portal_cur _ <<< "$portal_info"
        remove_pkgs+=("$portal_cur")
        display_pkgs+=("xdg-desktop-portal-gds")
    fi

    if ((${#remove_pkgs[@]} == 0)); then
        printf 'GDS is not currently installed.\n'
        return 0
    fi

    printf 'GDS Uninstall Plan\n'
    printf '  Backend : %s\n' "$backend"
    printf '  Remove  : %s\n' "${display_pkgs[*]}"
    printf '  Preserved: ~/.config/gds and ~/.local/state/gds\n'

    if $opt_dry_run; then
        printf '\nDry run complete; no changes were made.\n'
        return 0
    fi

    if ! $opt_non_interactive; then
        open_tty_fd
        if ! confirm_tty $'\nProceed with uninstall? [y/N]: '; then
            close_tty_fd
            printf 'Uninstall cancelled.\n'
            return 0
        fi
        close_tty_fd
    fi

    if $use_nix; then
        if ((EUID == 0)) && [[ -n ${SUDO_USER:-} ]]; then
            fail "Do not run Nix uninstall under sudo (SUDO_USER=$SUDO_USER); run as your normal user."
        fi
        nix --extra-experimental-features 'nix-command flakes' profile remove "${remove_pkgs[@]}"
    else
        local -a elevate=()
        if ((EUID != 0)); then
            elevate=(sudo)
        fi
        case "$backend" in
            deb) "${elevate[@]}" apt-get remove -y "${remove_pkgs[@]}" ;;
            rpm) "${elevate[@]}" dnf remove -y "${remove_pkgs[@]}" ;;
            pacman) "${elevate[@]}" pacman -R --noconfirm "${remove_pkgs[@]}" ;;
        esac
    fi
    printf 'GDS removed. User settings and state were kept.\n'
}

resolve_release() {
    init_tmp_dir

    if [[ -z $opt_version ]]; then
        download_url "$dist_url/channels/stable" "$GDS_INSTALL_TMP/stable" || \
            fail "Failed to resolve stable release version from $dist_url/channels/stable."
        local stable_tag
        IFS= read -r stable_tag < "$GDS_INSTALL_TMP/stable" || true
        stable_tag=${stable_tag%$'\r'}
        stable_tag=${stable_tag//[[:space:]]/}
        validate_tag "$stable_tag" || fail "Invalid release tag received from $dist_url/channels/stable: '$stable_tag'."
        opt_version=$stable_tag
    fi

    release_tag=$opt_version

    if $use_nix; then
        files_tgt_ver=${release_tag#v}
        portal_tgt_ver=${release_tag#v}
        portal_available=true
        return 0
    fi

    local manifest_url="$dist_url/releases/$release_tag/MANIFEST"
    download_url "$manifest_url" "$GDS_INSTALL_TMP/MANIFEST" || \
        fail "Release manifest is unavailable at $manifest_url. The release may still be building or unreachable; retry shortly."

    local header_seen=false line c_comp c_backend c_distro c_ver c_arch c_pkgver c_file c_sha extra
    local -a avail_vers=()
    while IFS= read -r line || [[ -n $line ]]; do
        line=${line%$'\r'}
        if ! $header_seen; then
            [[ $line =~ ^#[[:space:]]*gds-manifest-v1([[:space:]]|$) ]] || \
                fail "Invalid release manifest header from $manifest_url."
            header_seen=true
            continue
        fi
        [[ -n $line && $line != \#* ]] || continue
        extra=''
        read -r c_comp c_backend c_distro c_ver c_arch c_pkgver c_file c_sha extra <<< "$line"
        [[ -n $c_comp && -n $c_backend && -n $c_distro && -n $c_ver && -n $c_arch && -n $c_pkgver && -n $c_file && -n $c_sha && -z $extra ]] || \
            fail 'Malformed entry in release MANIFEST.'
        [[ $c_comp =~ ^[a-z0-9-]+$ && $c_backend =~ ^[a-z0-9-]+$ && $c_distro =~ ^[a-z0-9-]+$ ]] || fail 'Unsafe MANIFEST identifier.'
        [[ $c_ver =~ ^[a-z0-9._-]+$ && $c_arch =~ ^[a-zA-Z0-9._-]+$ ]] || fail 'Unsafe MANIFEST target.'
        [[ $c_pkgver =~ ^[a-zA-Z0-9][a-zA-Z0-9._~+-]*$ ]] || fail 'Unsafe MANIFEST package version.'
        [[ $c_file =~ ^[a-zA-Z0-9][a-zA-Z0-9._~+-]*$ && $c_file != */* && $c_file != .* ]] || fail 'Unsafe MANIFEST filename.'
        [[ $c_sha =~ ^[[:xdigit:]]{64}$ ]] || fail 'Invalid SHA-256 entry in MANIFEST.'

        if [[ $c_comp == files && $c_backend == "$backend" && $c_distro == "$upstream_distro" && $c_arch == "$pkg_arch" ]]; then
            avail_vers+=("$c_ver")
        fi
    done < "$GDS_INSTALL_TMP/MANIFEST"

    $header_seen || fail "Empty release manifest at $manifest_url."
    ((${#avail_vers[@]} > 0)) || \
        fail "No native $pkg_arch packages for $upstream_distro in release $release_tag. Check supported targets or pass --nix."

    if [[ $upstream_distro == arch || $tier_kind == Manual ]]; then
        local exact_found=false v
        for v in "${avail_vers[@]}"; do
            if [[ $v == "$upstream_ver" ]]; then
                exact_found=true
                resolved_ver=$v
                break
            fi
        done
        $exact_found || fail "No native $pkg_arch package for $upstream_distro $upstream_ver in release $release_tag (available: ${avail_vers[*]})."
    else
        local exact_found=false best_lt='' min_avail='' v
        for v in "${avail_vers[@]}"; do
            if [[ -z $min_avail ]] || [[ $(compare_baseline_versions "$v" "$min_avail") == -1 ]]; then
                min_avail=$v
            fi
            if [[ $v == "$upstream_ver" ]]; then
                exact_found=true
                resolved_ver=$v
                break
            fi
            if [[ $(compare_baseline_versions "$v" "$upstream_ver") == -1 ]]; then
                if [[ -z $best_lt ]] || [[ $(compare_baseline_versions "$v" "$best_lt") == 1 ]]; then
                    best_lt=$v
                fi
            fi
        done
        if ! $exact_found; then
            if [[ -n $best_lt ]]; then
                resolved_ver=$best_lt
                tier_kind='Tier-2'
            else
                fail "${os_pretty_name:-$os_id} ($upstream_distro $upstream_ver) is older than the minimum supported release ($upstream_distro $min_avail)."
            fi
        fi
    fi

    files_tgt_ver='' files_filename='' files_sha=''
    portal_tgt_ver='' portal_filename='' portal_sha=''
    local files_hits=0 portal_hits=0
    while IFS= read -r line || [[ -n $line ]]; do
        line=${line%$'\r'}
        [[ -n $line && $line != \#* ]] || continue
        read -r c_comp c_backend c_distro c_ver c_arch c_pkgver c_file c_sha extra <<< "$line"
        if [[ $c_backend == "$backend" && $c_distro == "$upstream_distro" && $c_ver == "$resolved_ver" && $c_arch == "$pkg_arch" ]]; then
            case "$c_comp" in
                files)
                    files_tgt_ver=$c_pkgver; files_filename=$c_file; files_sha=$c_sha
                    ((files_hits += 1)) ;;
                portal)
                    portal_tgt_ver=$c_pkgver; portal_filename=$c_file; portal_sha=$c_sha
                    ((portal_hits += 1)) ;;
            esac
        fi
    done < "$GDS_INSTALL_TMP/MANIFEST"

    ((files_hits == 1)) || fail "Expected one files package for $upstream_distro:$resolved_ver ($pkg_arch) in $release_tag; found $files_hits."
    if ((portal_hits == 1)); then
        portal_available=true
    elif ((portal_hits == 0)); then
        portal_available=false
        [[ $opt_portal != include ]] || fail "Expected one portal package for $upstream_distro:$resolved_ver ($pkg_arch) in $release_tag; found 0."
    else
        fail "Expected at most one portal package for $upstream_distro:$resolved_ver ($pkg_arch) in $release_tag; found $portal_hits."
    fi
}

inspect_installed() {
    files_cur_pkg=''; files_cur_ver=''; files_state='fresh'
    portal_cur_pkg=''; portal_cur_ver=''; portal_state='fresh'
    classify_component_state files 'gds-files' "$files_tgt_ver"
    if $portal_available; then
        classify_component_state portal 'xdg-desktop-portal-gds' "$portal_tgt_ver"
    fi

    select_files=true
    select_portal=false
    portal_auto_included=false
    if $portal_available; then
        case "$opt_portal" in
            include) select_portal=true ;;
            exclude) select_portal=false ;;
            auto)
                if [[ $portal_state != fresh ]]; then
                    select_portal=true
                    portal_auto_included=true
                fi ;;
        esac
    fi
}

format_component_status() {
    local state=$1 cur=$2 tgt=$3
    case "$state" in
        fresh) printf 'fresh install: %s' "$tgt" ;;
        upgrade) printf 'upgrade: %s -> %s' "$cur" "$tgt" ;;
        same) printf 'up to date: %s' "$tgt" ;;
        downgrade) printf 'downgrade: %s -> %s' "$cur" "$tgt" ;;
    esac
}

overall_action_label() {
    local has_upgrade=false has_fresh=false has_downgrade=false has_same=false
    local s
    for s in "$@"; do
        case "$s" in
            upgrade) has_upgrade=true ;;
            fresh) has_fresh=true ;;
            downgrade) has_downgrade=true ;;
            same) has_same=true ;;
        esac
    done
    if $has_downgrade; then
        printf 'Downgrade'
    elif $has_upgrade; then
        printf 'Upgrade'
    elif $has_fresh; then
        printf 'Fresh install'
    elif $has_same; then
        printf 'Up to date'
    else
        printf 'No selection'
    fi
}

print_summary() {
    local show_indices=$1
    local src_desc="${os_pretty_name:-${os_id:-Linux}${os_version_id:+ $os_version_id}}"
    local target_desc="$upstream_distro $resolved_ver ($pkg_arch)"
    if $use_nix; then
        target_desc="Nix ($pkg_arch)"
    fi

    local -a active_states=()
    if $select_files; then active_states+=("$files_state"); fi
    if $select_portal; then active_states+=("$portal_state"); fi

    printf '\nGDS Installer (release %s)\n' "$release_tag"
    if [[ $src_desc == *"$upstream_distro"* && $resolved_ver == "$upstream_ver" && $tier_kind == Tier-1 ]]; then
        printf '  System : %s [%s]\n' "$target_desc" "$tier_kind"
    else
        printf '  System : %s -> %s [%s]\n' "$src_desc" "$target_desc" "$tier_kind"
    fi
    printf '  Backend: %s (%s)\n' "$backend" "$manager"
    if $manjaro_warn; then
        printf '  Notice : Manjaro repositories lag behind Arch rolling releases.\n'
    fi
    if ((${#active_states[@]} > 0)); then
        printf '  Action : %s\n\n' "$(overall_action_label "${active_states[@]}")"
    else
        printf '  Action : %s\n\n' "$(overall_action_label)"
    fi

    local mark_files='[ ]' mark_portal='[ ]'
    if $select_files; then mark_files='[x]'; fi
    if $select_portal; then mark_portal='[x]'; fi

    local idx1='' idx2=''
    if $show_indices; then
        idx1='[1] '
        idx2='[2] '
    fi

    printf '  %s%s %-24s %s\n' "$idx1" "$mark_files" 'gds-files' "$(format_component_status "$files_state" "$files_cur_ver" "$files_tgt_ver")"
    if $portal_available; then
        local suffix=''
        if $portal_auto_included && $select_portal; then
            suffix=' (installed, auto-included)'
        fi
        printf '  %s%s %-24s %s%s\n' "$idx2" "$mark_portal" 'xdg-desktop-portal-gds' "$(format_component_status "$portal_state" "$portal_cur_ver" "$portal_tgt_ver")" "$suffix"
    fi
}

select_components() {
    if $opt_dry_run; then
        print_summary false
        printf '\nDry run complete; no changes were made.\n'
        return 1
    fi

    if $opt_non_interactive; then
        if ! $opt_reinstall; then
            if $select_files && [[ $files_state == same ]]; then
                select_files=false
            fi
            if $select_portal && [[ $portal_state == same ]]; then
                select_portal=false
            fi
        fi
        if ! $select_files && ! $select_portal; then
            printf 'GDS is already up to date (%s). Pass --reinstall to reinstall.\n' "$release_tag"
            return 1
        fi
        print_summary false
        return 0
    fi

    open_tty_fd
    if $manjaro_warn; then
        printf '\nWarning: Manjaro repositories lag behind Arch Linux rolling releases, which may cause shared library ABI mismatches.\n'
        if ! confirm_tty 'Continue with Arch rolling packages? [y/N]: '; then
            close_tty_fd
            printf 'Installation cancelled.\n'
            return 1
        fi
    fi

    local choice=''
    while true; do
        print_summary true
        if $portal_available; then
            printf '\nToggle [1-2], press Enter to continue, or q to cancel: '
        else
            printf '\nToggle [1], press Enter to continue, or q to cancel: '
        fi
        if ! IFS= read -r choice <&3; then
            close_tty_fd
            fail 'No confirmation received from terminal.'
        fi
        case "$choice" in
            '')
                break ;;
            1)
                if $select_files; then select_files=false; else select_files=true; fi ;;
            2)
                if $portal_available; then
                    if $select_portal; then select_portal=false; else select_portal=true; fi
                else
                    printf 'Invalid choice: 2.\n'
                fi ;;
            q|Q)
                close_tty_fd
                printf 'Installation cancelled.\n'
                return 1 ;;
            *)
                printf 'Invalid choice: %s.\n' "$choice" ;;
        esac
    done

    if ! $select_files && ! $select_portal; then
        close_tty_fd
        printf 'No components selected. Cancelled.\n'
        return 1
    fi

    local all_same=true
    if $select_files && [[ $files_state != same ]]; then all_same=false; fi
    if $select_portal && [[ $portal_state != same ]]; then all_same=false; fi

    if $all_same && ! $opt_reinstall; then
        if confirm_tty $'\nSelected packages are already at the target version. Reinstall anyway? [y/N]: '; then
            opt_reinstall=true
        else
            close_tty_fd
            printf 'No changes needed.\n'
            return 1
        fi
    elif ! $opt_reinstall; then
        if $select_files && [[ $files_state == same ]]; then select_files=false; fi
        if $select_portal && [[ $portal_state == same ]]; then select_portal=false; fi
    fi

    close_tty_fd
    return 0
}

install_selected() {
    if $use_nix; then
        if ((EUID == 0)) && [[ -n ${SUDO_USER:-} ]]; then
            fail "Do not run Nix installation under sudo (SUDO_USER=$SUDO_USER); run as your normal user so it installs into your own profile."
        fi
        local flake_ref="$flake_uri/$release_tag"
        local -a nix_remove=() nix_add=()
        if $select_files; then
            if [[ -n $files_cur_pkg ]]; then nix_remove+=("$files_cur_pkg"); fi
            nix_add+=("$flake_ref#gds-files")
        fi
        if $select_portal; then
            if [[ -n $portal_cur_pkg ]]; then nix_remove+=("$portal_cur_pkg"); fi
            nix_add+=("$flake_ref#xdg-desktop-portal-gds")
        fi
        if ((${#nix_remove[@]} > 0)); then
            nix --extra-experimental-features 'nix-command flakes' profile remove "${nix_remove[@]}"
        fi
        nix --extra-experimental-features 'nix-command flakes' profile add "${nix_add[@]}"
        return 0
    fi

    local -a selected_files=() selected_hashes=() selected_states=() git_conflicts=()
    if $select_files; then
        selected_files+=("$files_filename")
        selected_hashes+=("$files_sha")
        selected_states+=("$files_state")
        if [[ $files_cur_pkg == *-git && $files_filename != *-git-* ]]; then
            git_conflicts+=("$files_cur_pkg")
        fi
    fi
    if $select_portal; then
        selected_files+=("$portal_filename")
        selected_hashes+=("$portal_sha")
        selected_states+=("$portal_state")
        if [[ $portal_cur_pkg == *-git && $portal_filename != *-git-* ]]; then
            git_conflicts+=("$portal_cur_pkg")
        fi
    fi

    local idx fname expected_hash pkg_path
    local -a package_paths=()
    for idx in "${!selected_files[@]}"; do
        fname=${selected_files[$idx]}
        expected_hash=${selected_hashes[$idx]}
        pkg_path="$GDS_INSTALL_TMP/$fname"
        printf 'Downloading %s...\n' "$fname"
        download_url "$dist_url/releases/$release_tag/$fname" "$pkg_path" || \
            fail "Failed to download $fname."
        (cd "$GDS_INSTALL_TMP" && printf '%s  %s\n' "$expected_hash" "$fname" | sha256sum --check --status) || \
            fail "Checksum verification failed: $fname"
        chmod 644 "$pkg_path"
        package_paths+=("$pkg_path")
    done

    local -a elevate=()
    if ((EUID != 0)); then
        elevate=(sudo)
    fi

    case "$backend" in
        pacman)
            if ((${#git_conflicts[@]} > 0)); then
                "${elevate[@]}" pacman -Rdd --noconfirm "${git_conflicts[@]}"
            fi
            local pac_sync_dir=${GDS_PACMAN_SYNC_DIR:-/var/lib/pacman/sync}
            if ! compgen -G "$pac_sync_dir/*.db" >/dev/null; then
                "${elevate[@]}" pacman -Sy --noconfirm
            fi
            "${elevate[@]}" pacman -U --noconfirm "${package_paths[@]}" ;;
        deb)
            local needs_reinstall=false needs_downgrade=false st
            for st in "${selected_states[@]}"; do
                case "$st" in
                    same) needs_reinstall=true ;;
                    downgrade) needs_downgrade=true ;;
                esac
            done
            local -a apt_flags=(-y)
            if $needs_reinstall; then apt_flags+=(--reinstall); fi
            if $needs_downgrade; then apt_flags+=(--allow-downgrades); fi

            local apt_lists_dir=${GDS_APT_LISTS_DIR:-/var/lib/apt/lists}
            if compgen -G "$apt_lists_dir/*Packages*" >/dev/null || \
               compgen -G "$apt_lists_dir/*InRelease" >/dev/null || \
               compgen -G "$apt_lists_dir/*Release" >/dev/null; then
                if ! "${elevate[@]}" apt-get install "${apt_flags[@]}" "${package_paths[@]}"; then
                    warn 'Direct apt-get install failed; refreshing APT package indexes and retrying...'
                    "${elevate[@]}" apt-get update
                    "${elevate[@]}" apt-get install "${apt_flags[@]}" "${package_paths[@]}"
                fi
            else
                "${elevate[@]}" apt-get update
                "${elevate[@]}" apt-get install "${apt_flags[@]}" "${package_paths[@]}"
            fi ;;
        rpm)
            local -a dnf_install=() dnf_reinstall=() dnf_downgrade=()
            for idx in "${!selected_states[@]}"; do
                case "${selected_states[$idx]}" in
                    same) dnf_reinstall+=("${package_paths[$idx]}") ;;
                    downgrade) dnf_downgrade+=("${package_paths[$idx]}") ;;
                    *) dnf_install+=("${package_paths[$idx]}") ;;
                esac
            done
            if ((${#dnf_install[@]} > 0)); then
                "${elevate[@]}" dnf install -y "${dnf_install[@]}"
            fi
            if ((${#dnf_reinstall[@]} > 0)); then
                "${elevate[@]}" dnf reinstall -y "${dnf_reinstall[@]}"
            fi
            if ((${#dnf_downgrade[@]} > 0)); then
                "${elevate[@]}" dnf downgrade -y "${dnf_downgrade[@]}"
            fi ;;
    esac
}

verify_and_report() {
    if $select_files && command -v gds-files >/dev/null 2>&1; then
        gds-files --help >/dev/null
    fi
    if $select_files; then
        printf '\nGDS Files (%s) is ready. Open it from your application menu or run: gds-files\n' "$release_tag"
    fi
    if $select_portal; then
        printf 'GDS Portal installed. Desktop routing is unchanged; enable it manually if desired:\n'
        printf '  https://github.com/GanDesktop/gds/blob/main/docs/apps/files/fedora.md#optional-gds-portal\n'
    fi
    if [[ $os_id == nixos ]]; then
        printf 'Tip (NixOS): For full desktop MIME/UDisks2/Portal integration, consider enabling the NixOS modules instead of profile-only installation:\n'
        printf '  https://github.com/GanDesktop/gds/blob/main/docs/nixos.md\n'
    fi
}

main() {
    parse_args "$@"
    probe_platform

    if $opt_uninstall; then
        run_uninstall
        return 0
    fi

    resolve_release
    inspect_installed
    if ! select_components; then
        return 0
    fi
    install_selected
    verify_and_report
}

# Keep execution at the end so this script works when piped into bash.
main "$@"
