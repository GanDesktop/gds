set positional-arguments

# 直接求值开发环境，避免无 Git 的工作目录将 target/ 一并复制进 Nix store。
develop := "nix develop --impure --expr 'import ./shell.nix'"

# 列出常用开发命令。
default:
    @just --list

# 打开测试虚拟机桌面，或执行 status / start / stop / reset / ssh / copy（machine 可传 all）。
vm machine="all" *args:
    @just _python scripts/vm.py "$@"

# 编译并启动指定目标，可继续传应用参数。
dev app *args:
    @just _cargo run "$@"

# 进入完整的 Nix 开发环境。
shell:
    {{ develop }}

# 构建整个套件，或指定 files / portal。
build app="all":
    @just _cargo build "$@"

# 构建整个套件或指定应用的发布版。
release app="all":
    @just _cargo build "$@" --release --locked

# 在宿主机的 Fedora 容器中增量构建 Files 和可选 Portal 的原生 RPM。
rpm fedora="43" profile="dev":
    @just _python scripts/build-rpm.py --fedora "$1" --profile "$2"

# 在 Arch 容器中构建 Files 和独立 Portal 的 pacman 包。
arch profile="dev":
    @just _python scripts/build-linux-package.py arch --profile "$1"

# 在 Debian/Ubuntu 容器中构建可通过 APT 安装的原生 DEB。
deb distro="debian" release="" profile="dev":
    @just _python scripts/build-linux-package.py "$1" --release "$2" --profile "$3"

# 安装 Files（或指定 portal / all）到当前用户的 Nix profile。
install app="files":
    nix profile add {{ if app == "files" { ".#gds-files" } else if app == "portal" { ".#xdg-desktop-portal-gds" } else if app == "all" { ".#gds-files .#xdg-desktop-portal-gds" } else { error("未知组件：" + app + "；可选 files、portal、all") } }}

# 从当前用户的 Nix profile 移除已安装的 GDS 组件（files / portal / all）。
uninstall app="all":
    #!/usr/bin/env bash
    set -euo pipefail
    case "$1" in
        files|portal|all) ;;
        *) printf '未知组件：%s；可选 files、portal、all\n' "$1" >&2; exit 1 ;;
    esac
    json=$(nix profile list --json 2>/dev/null || printf '')
    paths=()
    add_matches() {
        local m
        while IFS= read -r m; do
            m=${m%\"}
            [[ -n $m ]] && paths+=("$m")
        done < <(printf '%s' "$json" | grep -oE "$1" || true)
    }
    if [[ $1 == files || $1 == all ]]; then
        add_matches '/nix/store/[0-9a-z]+-gds-files(-[0-9][^"[:space:],}]*)?"'
    fi
    if [[ $1 == portal || $1 == all ]]; then
        add_matches '/nix/store/[0-9a-z]+-xdg-desktop-portal-gds(-[0-9][^"[:space:],}]*)?"'
    fi
    if [[ $1 == all ]]; then
        add_matches '/nix/store/[0-9a-z]+-gds(-[0-9][^"[:space:],}]*)?"'
    fi
    if ((${#paths[@]} == 0)); then
        if [[ $1 != all ]] && printf '%s' "$json" | grep -qE '/nix/store/[0-9a-z]+-gds(-[0-9][^"[:space:],}]*)?"'; then
            printf '检测到旧版合并包 gds，请运行 just uninstall（或 just uninstall all）一并移除。\n' >&2
            exit 1
        fi
        printf '当前 Nix profile 中未找到对应的 GDS 组件（%s）。\n' "$1"
        exit 0
    fi
    mapfile -t uniq_paths < <(printf '%s\n' "${paths[@]}" | awk '!seen[$0]++')
    nix profile remove "${uniq_paths[@]}"

# 使用 target/debug 缓存构建，并安装本机开发版入口。
install-dev app="all":
    #!/usr/bin/env bash
    set -euo pipefail
    just _cargo build "$1" --locked
    runtime_profile="${XDG_STATE_HOME:-$HOME/.local/state}/gds/development/environment"
    mkdir -p "$(dirname "$runtime_profile")"
    {{ develop }} --profile "$runtime_profile" --command python3 scripts/install-dev.py "$1"

# 移除开发版入口，恢复此前的命令和桌面入口。
uninstall-dev app="all":
    {{ develop }} --command python3 scripts/install-dev.py --remove "$1"

# 检查全部 Rust 目标。
check:
    {{ develop }} --command cargo check --workspace --features files,portal --all-targets

# 检查代码格式与全部 Clippy 规范门禁。
lint:
    #!/usr/bin/env bash
    set -euo pipefail
    {{ develop }} --command cargo fmt --all --check
    {{ develop }} --command cargo clippy --workspace --features gui-tests,files-e2e,portal --lib --bins -- -D clippy::disallowed_methods -D warnings
    {{ develop }} --command cargo clippy --workspace --features gui-tests,files-e2e,portal --all-targets -- -D warnings

# 运行测试，可传测试名称等 Cargo 参数。
test *args:
    {{ develop }} --command cargo test --workspace --features files,portal "$@"

# 为真实桌面 VM 构建 Files 普通版本，并打包精确源码和运行依赖。
e2e-build:
    {{ develop }} --command cargo build --locked --bin gds-files --features files
    {{ develop }} --command python3 scripts/e2e.py bundle

# 安装客体的 AT-SPI/Python/剪贴板测试依赖，不重置磁盘快照。
e2e-prepare machine="all":
    @just _python scripts/e2e.py prepare "$1"

# 在 QEMU 桌面中运行 pytest 工作流；可传 --vm、-k、-m、--collect-only。
test-e2e *args:
    uv run --locked --project tests/e2e pytest tests/e2e "$@"

# 用统一 VM 框架运行 Files / Images 工作流。
test-files *args:
    @just test-e2e -m files "$@"

test-images *args:
    @just test-e2e -m images "$@"

# 原生 UI 事件/渲染工作流，加独立 Weston 启动、截图与退出验证。
test-files-native:
    #!/usr/bin/env bash
    set -euo pipefail
    {{ develop }} --command cargo build --locked --bin gds-files --features files-e2e
    files_weston=$(nix build --impure --no-link --print-out-paths --expr 'let lock = builtins.fromJSON (builtins.readFile ./flake.lock); source = builtins.fetchTree lock.nodes.nixpkgs.locked; pkgs = import source.outPath {}; in pkgs.weston')
    {{ develop }} --command python3 bin/files/tests/run.py target/debug/gds-files "$files_weston/bin/weston"

# 格式化项目代码。
fmt:
    {{ develop }} --command cargo fmt --all

# 只检查格式，不修改文件。
fmt-check:
    {{ develop }} --command cargo fmt --all --check

[private]
_cargo command app *args:
    #!/usr/bin/env bash
    set -euo pipefail
    command="$1"
    app="$2"
    shift 2
    case "$app" in
        all)
            if [ "$command" = run ]; then
                printf '%s\n' '启动时请指定目标：just dev files / just dev portal' >&2
                exit 2
            fi
            targets=(--bins --features files,portal)
            ;;
        files) targets=(--bin gds-files --features files) ;;
        portal) targets=(--bin xdg-desktop-portal-gds --features portal) ;;
        *)
            printf '未知应用：%s；可选 files、portal\n' "$app" >&2
            exit 2
            ;;
    esac
    if [ "$command" = run ]; then
        targets+=(--)
    fi
    {{ develop }} --command cargo "$command" "${targets[@]}" "$@"

[private]
_python *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v python3 >/dev/null 2>&1; then
        exec python3 "$@"
    fi
    exec {{ develop }} --command python3 "$@"
