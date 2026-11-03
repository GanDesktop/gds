set positional-arguments

# 直接求值开发环境，避免无 Git 的工作目录将 target/ 一并复制进 Nix store。
develop := "nix develop --impure --expr 'import ./shell.nix'"

# 列出常用开发命令。
default:
    @just --list

# 编译并启动指定目标，可继续传应用参数。
dev app *args:
    @just _cargo run "$@"

# 进入完整的 Nix 开发环境。
shell:
    {{ develop }}

# 构建整个套件，或指定 files / agents / portal。
build app="all":
    @just _cargo build "$@"

# 构建整个套件或指定应用的发布版。
release app="all":
    @just _cargo build "$@" --release --locked

# 在宿主机的 Fedora 容器中增量构建 Files 和可选 Portal 的原生 RPM。
rpm fedora="43" profile="dev":
    @python3 scripts/build-rpm.py --fedora "$1" --profile "$2"

# 在 Arch 容器中构建 Files 和独立 Portal 的 pacman 包。
arch profile="dev":
    @python3 scripts/build-linux-package.py arch --profile "$1"

# 在 Debian/Ubuntu 容器中构建可通过 APT 安装的原生 DEB。
deb distro="debian" release="" profile="dev":
    @python3 scripts/build-linux-package.py "$1" --release "$2" --profile "$3"

# 安装整个套件，或指定 files / agents 到当前用户的 Nix profile。
install app="all":
    nix profile install {{ if app == "all" { ".#gds" } else if app == "files" { ".#gds-files" } else if app == "agents" { ".#gds-agents" } else { error("未知应用：" + app + "；可选 all、files、agents") } }}

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
    {{ develop }} --command cargo check --workspace --features files,agents,portal,gpui-kit/test-support --all-targets

# 运行测试，可传测试名称等 Cargo 参数。
test *args:
    {{ develop }} --command cargo test --workspace --features files,agents,portal,gpui-kit/test-support "$@"

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
                printf '%s\n' '启动时请指定目标：just dev files / just dev agents / just dev portal' >&2
                exit 2
            fi
            targets=(--bins --features files,agents,portal)
            ;;
        files) targets=(--bin gds-files --features files) ;;
        agents) targets=(--bin gds-agents --features agents) ;;
        portal) targets=(--bin xdg-desktop-portal-gds --features portal) ;;
        *)
            printf '未知应用：%s；可选 files、agents、portal\n' "$app" >&2
            exit 2
            ;;
    esac
    if [ "$command" = run ]; then
        targets+=(--)
    fi
    {{ develop }} --command cargo "$command" "${targets[@]}" "$@"
