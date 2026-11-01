# 原生 App 开发与运行

GDS 包含 Files、Agents 等平级的原生桌面应用，共用 `src/lib.rs`，各应用代码位于 `bin/<应用>/src/`。首要支持 **Linux + Wayland**；应用使用 GPUI 原生绘制，不包含 WebView。

## 仓库结构

```text
Cargo.toml                 # 所有 binary target、feature 与依赖
LICENSE                    # 项目自有代码共用的 Apache-2.0 许可证
src/                       # 公共库
macros/                    # Rust 过程宏
flake.nix / flake.lock      # Nix flake 入口与依赖锁
shell.nix                  # 本地 Nix 开发环境入口
packaging/
├── nix/                   # Nix 包、NixOS 模块与开发环境定义
└── fedora/                # Fedora RPM、构建容器与安装资源
bin/
├── files/
│   ├── README.md
│   └── src/               # Files 的 FSD 应用代码
├── agents/
│   ├── README.md
│   └── src/               # Agents 的 FSD 应用代码
└── portal/
    ├── README.md
    └── src/               # 无界面 Portal 后端
```

各应用目录的 README 提供职责、入口与运行命令；完整设计与开发文档留在 `docs/`。应用共用根 manifest 和许可证，不单独建立 Cargo package，也不重复放置 LICENSE。

各安装方式的构建、打包与系统集成统一放在 `packaging/`。Nix 的根目录入口委托给 `packaging/nix/` 中的定义；生成的二进制、RPM 和构建缓存保留在 `target/`，不放进打包定义目录。

## 运行

应用使用方式见 [Files](apps/files/README.md)、[Agents](apps/agents/README.md)；系统安装与桌面接入见 [NixOS 安装](nixos.md)。

在图形会话里的终端执行；[justfile](../justfile) 会自动进入 Nix 开发环境：

```sh
just dev files
# 指定起始目录（包含空格时正常加引号）：
just dev files "/path/to/folder"
# 启动 Agents，或打开它的快捷输入窗口：
just dev agents
just dev agents --compact
# 查看可用命令：
just
```

`just build` 构建全部应用的开发版，`just build files` / `just build agents` / `just build portal` 单独构建指定应用。`just check` 检查所有目标，`just test` 运行测试，`just fmt` 格式化，`just fmt-check` 只检查格式。`just shell` 进入交互式开发环境。开发环境也包含 just；主机没有 just 时可用 `nix develop --impure --expr 'import ./shell.nix' --command just dev files`。

应用参数会原样传递，例如 `just -- dev files --help` 或 `just -- dev agents --help`；这些命令继承当前图形会话环境，不固定显示服务器或用户路径。

### 开发版安装

频繁调整界面时使用 `just install-dev`，也可指定 `just install-dev files` / `just install-dev agents`。它用普通 Cargo dev profile 增量构建，复用当前仓库的 `target/debug` 缓存，再安装用户级命令和标为 `(Dev)` 的桌面入口，不走 Nix 发布包的完整 LTO 和包内测试流程。

入口通过轻量运行包装直接执行工作区中的 debug 二进制，不复制大型产物，也不在每次启动时进入 `nix develop`。包装仅补充运行库、终端启动器、缩略图工具和 terminfo，保留桌面会话的 `SHELL`、提示符与其它环境；安装时记录在用户 state 目录中的 Nix profile 保持这些依赖可达。后续 `just build` 或 `just install-dev` 更新相同二进制，重开窗口即可使用新版本。工作区与 `target/debug` 需要保留，尚未运行的测试仍可通过 `just test` 或 GUI 测试命令单独执行。

`just uninstall-dev`（也可指定单个应用）恢复之前的命令和桌面入口；此前的 Nix 发布包仍然保留。发布交付继续使用 `just install`。

所有图形应用共用 [About 组件](components/about.md)。应用身份在 Cargo 元数据中登记，版本沿用 package version，build tag 在构建时写入短 commit hash；新二进制只需要增加登记并挂载公共组件。

桌面入口的名称、通用名称和说明由 `assets/<应用>/desktop.toml` 统一维护。`[default]` 为英文回退，`[zh_CN]` 为简体中文；`Name` 是应用名，`GenericName` 是通用用途名，`Comment` 是简短说明。Nix 与开发版安装器读取同一份数据，生成 `Name[zh_CN]` 等标准字段；开发版仅在各语言的名称后追加 `(Dev)`。桌面使用 POSIX locale（例如 `zh_CN`），应用内 Fluent 资源仍使用 `zh-CN`，二者独立。字段与匹配规则见 [Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry/latest/localized-keys.html)。

GDS Portal 的包和 NixOS 模块独立放在 `packaging/nix/portal/`，图形应用定义仍位于 `packaging/nix/apps/`。通过 `services.gds-portal.enable` 单独启用 Portal；其软件包不绑定 Files 的路径。文件选择接口默认调用 PATH 中的 `gds-files`，也可以用 `--file-chooser` 指定实现同一原生协议的处理程序。

开发入口直接求值 [shell.nix](../shell.nix)，按 [flake.lock](../flake.lock) 的 revision 与 NAR hash 获取依赖，并和 flake 共用 [开发环境定义](../packaging/nix/dev-shell.nix)。这避免对整个项目做 flake 源快照：未置于 Git 中的工作目录，其 `.gitignore` 不会替 `nix develop` 排除 `target/`，反复快照会占用大量 Nix store 空间。请使用 `just dev <应用>` / `just shell`，而不是在这种目录中直接运行裸 `nix develop`。

保持图形会话提供的 `XDG_RUNTIME_DIR`、`WAYLAND_DISPLAY` 和 D-Bus 环境。不要为 GPUI 设置 `QT_QPA_PLATFORMTHEME`，也不要修改 Niri / DMS 配置来运行本应用。非 Nix 环境需要相应 Rust 工具链、Wayland/X11、xkbcommon、字体栈和 Vulkan/GL 运行库。

```sh
cargo fmt --all --check
cargo check --workspace --features files,agents,portal,gpui-kit/test-support --all-targets
cargo test --workspace --features files,agents,portal,gpui-kit/test-support
cargo build --bins --features files,agents,portal
```

首次构建需要下载依赖并编译图形栈。可通过 `CARGO_BUILD_JOBS` 调整本机并行度；例如 `CARGO_BUILD_JOBS=6 cargo build --bin gds-files --features files`。

## 公共库与 feature

`gds` 默认不启用任何能力。公共模块和它使用的依赖由同名 feature 一起开启，应用入口通过 `required-features` 声明组合：

| Feature | 能力 |
| --- | --- |
| `desktop-protocol` | `service::desktop::protocol` 中的进程通信类型，只依赖 Serde |
| `persistence` | 原子文件存储与路径管理 |
| `watch` | 文件变化通知，独立于 GUI |
| `desktop` | 桌面打开方式、剪贴板、FileManager1 与 Portal 客户端 |
| `storage` | Toasty / SQLite 存储 |
| `logs` | 本进程日志服务 |
| `terminal` | PTY 与终端模型 |
| `thumbnail` | 缩略图缓存与生成 |
| `remote` | SFTP / SMB 文件服务 |
| `appearance` | 主题、字体、视觉 tokens 与 GPUI 外观安装 |
| `i18n` | Fluent 翻译与 GPUI 文本适配 |
| `ui` | 公共组件及其需要的外观、翻译和服务 |
| `files` / `agents` | 对应图形应用使用的完整组合 |
| `portal` | 无界面 Portal 后端与共享桌面协议 |
| `gui-tests` | 两个应用与 GPUI 测试支持 |

`bin/portal/src` 与 Files、Agents 一样，是根 package 中声明的 binary target，没有单独的 manifest。`portal` feature 只启用共享协议及其自身的 I/O 依赖，独立构建不会带入 GPUI、SQLite、终端或远程文件后端：

```sh
cargo check --lib --no-default-features
cargo check --lib --no-default-features --features storage
cargo check --bin xdg-desktop-portal-gds --no-default-features --features portal
cargo test --bin xdg-desktop-portal-gds --no-default-features --features portal
cargo tree --no-default-features --features portal --edges normal,build
```

应用 I/O 和 Portal 统一使用 Tokio；GPUI 执行器负责界面任务。Portal 的 zbus 连接、子进程、信号和超时均在其 Tokio 运行时中执行。

## 第三方补丁

第三方源码在 GanDesktop 的独立仓库维护，通过 Git 分支引用，由 `Cargo.lock` 固定提交；不将源码复制进本项目。当前 GPUI 基于发布的 `0.3.10` 包快照，保留包名和版本：

- [gpui-pre / gds-0.3.10](https://github.com/GanDesktop/gpui-pre/tree/gds-0.3.10)：外部父窗口与调用方激活 token 的公共接口。
- [gpui-pre-linux / gds-0.3.10](https://github.com/GanDesktop/gpui-pre-linux/tree/gds-0.3.10)：对应 Wayland / X11 实现。
- [trash-rs / fix/freedesktop-trashinfo-reviewed](https://github.com/GanDesktop/trash-rs/tree/fix/freedesktop-trashinfo-reviewed)：回收站记录解析与路径解析修复，保留 `trash` 包名及 `5.2.9` 版本。

两份 GPUI 补丁配套更新，各自的 `PATCHES.md` 记录来源与范围。更新补丁分支后，同步本项目 `Cargo.lock` 和 `packaging/nix/cargo-lock.nix` 中的源码 hash；本仓库不保留 vendor 源码。

## Release 构建

Fedora 原生 RPM 的构建与安装见 [Fedora 打包流程](apps/files/fedora.md)。`just rpm` 在宿主机的 Fedora 容器中增量构建 Files 与独立的 GDS Portal 包；Fedora 测试机只安装 RPM，无需编译或安装 Nix。

执行 `just release` 构建整个套件，或用 `just release files` / `just release agents` / `just release portal` 单独构建。等价的套件命令是 `cargo build --release --locked --bins --features files,agents,portal`，产物位于 `target/release/`，包含 `gds-files`、`gds-agents` 和 `xdg-desktop-portal-gds`。Nix 打包默认使用同一份 release 配置；安装命令见 [NixOS 安装](nixos.md)。

[Cargo.toml](../Cargo.toml) 的 `[profile.release]` 以运行性能为先：`opt-level = 3`、完整 LTO（`lto = "fat"`）和 `codegen-units = 1` 提供跨 crate 优化机会，代价是更长的编译／链接时间和更高的构建内存需求。`debug = 0`、`strip = "symbols"` 去掉调试信息和符号，缩小交付体积，但也会减少崩溃回溯信息；关闭增量编译（`incremental = false`）。参数说明见 [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)。实际性能收益需要用具体工作负载衡量。

保留默认的 panic unwind 策略，以兼容 GPUI 依赖中的展开处理。发布配置使用通用目标 CPU；仅供本机使用的构建可自行加 `RUSTFLAGS="-C target-cpu=native"`，此时产物可能无法在较旧的 CPU 上运行。

## 二进制目录组织

Files 和 Agents 采用 [Feature-Sliced Design 的层定义](https://feature-sliced.design/docs/reference/layers)、[切片与分段](https://feature-sliced.design/docs/reference/slices-segments)及[公开入口规则](https://feature-sliced.design/docs/reference/public-api)。入口 `main.rs` 只声明层模块并调用 `app::run()`。层按依赖方向排列：

```text
app → pages → widgets → features → entities → shared
```

模块可以跳过中间层，直接依赖任意下层。同层切片相互独立；`app` 和 `shared` 按职责分段，不再划分业务切片，也不受切片间隔离规则约束。只创建已有代码需要的层和切片。

| 层 | 职责 | Files | Agents |
| --- | --- | --- | --- |
| `app` | 命令行入口、初始化、路由安装和退出 | [app.rs](../bin/files/src/app.rs)、`app/startup.rs`、`app/cli.rs` | [app.rs](../bin/agents/src/app.rs)、`app/startup.rs`、`app/cli.rs` |
| `pages` | 完整窗口及其页面组合 | `workspace`：浏览、选择模式、设置视图的组合 | `workspace`：会话工作区、后台切换和管理视图的窗口宿主 |
| `widgets` | 可独立使用、也可嵌入页面的界面块 | `settings` | `settings`（含 Agent 管理与安装内容）、`history` |
| `features` | 可由不同界面触发的用户操作与工作流 | `file_operations`、`pick_files` | `navigation`（切换后台、打开管理视图）、`agent_auth`、`delete_session` |
| `entities` | 领域数据、状态、相关 API 与呈现 | `file`、`location`、`preferences` | `agent`、`backend`、`session` |
| `shared` | 不承载业务决策的应用内基础能力 | `i18n`、`lib/clipboard` | `i18n`、`config`、`api`、`lib`、`ui`、`routes` |

每层使用 `<layer>.rs` 配合 `<layer>/`；每个切片使用 `<slice>.rs` 作为公开入口，内部按实际职责组织 `ui`、`model`、`api` 等分段。外部调用只通过切片入口公开的类型、函数或命名空间访问，内部实现分段保持私有。测试与资源留在所属模块附近。

`app/cli` 将启动输入转换为运行参数。翻译目录、翻译接口和语言状态在 `shared/i18n`，由启动代码安装，供各切片直接使用。配置持久化保持既有语义：Files 的用户偏好属于 `entities/preferences`，文件列布局属于 `entities/file`；Agents 的前端配置与存储适配在 `shared/config`。

Files 的位置记录和偏好引用文件列布局，通过 `entities/file/x/{location,preferences}` 明确公开这两个实体关系。这是 FSD `@x` 跨实体入口在 Rust 模块语法中的写法；其他层的同层切片不建立横向依赖。

Agents 的设置和历史浏览器既可嵌入工作区，也可独立打开，因此归入 widgets。`features/navigation` 声明路由回调与请求；`pages/workspace` 实现原生宿主，`app` 在启动时安装。这样 widgets 可以请求跳转，同时保持对页面和应用层的依赖方向。App 的跨层集成测试放在 `app/tests/`；模块测试通过各自公开接口和测试夹具验证行为。

跨应用通用组件继续使用公共库 `src/ui`。Agents 专属的通用组件和资源在 `shared/ui`。FileChooser Portal 位于 `bin/portal/src`，与其他应用共用根 manifest，保留独立的可执行入口，不属于 UI 应用的 FSD 层。

## 公共库目录与服务边界

公共库本身承担跨应用共享职责，根入口为 `src/lib.rs`，公开五组能力：

```text
src/
├── lib.rs
├── appearance.rs + appearance/
│   ├── theme.rs + theme/          # 主题来源、配色、明暗与热更新
│   ├── typography.rs             # 字体与字号规则
│   └── tokens.rs                 # 共享视觉默认值
├── service.rs + service/
│   ├── desktop.rs + desktop/       # 桌面打开、portal、剪贴板与监听
│   ├── remote.rs + remote/       # SFTP / SMB 地址、会话与文件传输接口
│   ├── logs.rs + logs/             # 本进程日志、tracing 采集与保留队列
│   ├── storage.rs + storage/       # Toasty SQLite 初始化与原生迁移接入
│   ├── terminal.rs + terminal/      # 终端进程、会话、状态、协议编码
│   └── thumbnail.rs + thumbnail/    # 图片／视频预览与标准磁盘缩略图
├── ui.rs + ui/
│   ├── feedback/                  # 通知、对话框、tooltip
│   ├── forms/                     # 字段、设置行与 ConfigEditor
│   ├── interaction/               # 快捷键提示、KeyTips、焦点区域及组件动作
│   ├── terminal/                  # 终端的 GPUI 实例、绘制、字体和 IME
│   ├── logs.rs + logs/            # 日志查看器、筛选、详情与窗口宿主
│   ├── time/                      # 时间戳组件及其刷新时钟
│   └── sidebar/、page/、frame/…    # 保留组件族的解剖边界
├── persistence.rs                 # 原子读写、锁、冲突检测
└── i18n.rs + i18n/                 # 翻译基础能力
```

`appearance` 提供外观规则：主题解析、字体应用和视觉默认值；它可以接入 GPUI，但不归入组件目录。`ui` 只放可复用组件，以及这些组件专属的状态、事件和渲染辅助实现。例如时间戳的刷新时钟、通知队列、终端的输入适配均随组件保存；全局主题、字体和视觉默认值统一从 `appearance` 使用；应用直接使用 `appearance`。

主题来源集中在 `appearance/theme/provider`，按 `system`（DMS 活动方案）、`config`（Qt 配置指定的色板）、`builtin`（内置配色）分组。来源负责发现、读取和解析，`palette` 保存通用颜色角色，主题应用及热更新协调留在外层。具体目录和明暗解析规则见 [主题文档](vistual-language/theme.md#4-固定配色分工)。

内置主题为竹林、怀旧、霓虹、马卡龙、高对比和 Aero，每套包含明暗两版，配色配置在 `appearance/theme/provider/builtin/assets/*.toml` 中并编入二进制。应用保存内置配色选择，修改主题不重置明暗偏好或透明度。

`service` 提供运行时能力，其接口使用普通 Rust 类型、原生协议模型和异步通道；服务代码不引用 `ui` 或 GPUI 类型。组件可以将服务状态映射为界面，并把鼠标、键盘和 IME 转换为服务操作。`persistence` 提供配置文件的原子读写与冲突检测；`service::storage` 提供 SQLite 应用数据存储，应用定义自己的表结构与迁移 SQL。

GPUI 与 Tokio 的执行器独立。Files 和 Agents 的启动层直接创建并持有原生 `tokio::runtime::Runtime`，直到 UI 主循环和退出钩子完成；后台 I/O 服务显式接收 `tokio::runtime::Handle` 并调用 `handle.spawn`。`cx.spawn` 在 GPUI 前台接收结果、更新界面，`cx.background_executor().spawn` 用于普通后台工作。需要随持有者销毁而取消的 Tokio 任务使用 `tokio-util::AbortOnDropHandle`；任务异常通过原生 `JoinHandle` 结果处理。没有全局 runtime 查找或通用 spawn 包装。

卷监听通过容量为 16 的 `tokio::sync::mpsc` 向 GPUI 发送 `Observation`。这个通道的普通异步收发不依赖 Tokio 上下文，GPUI 前台可以直接等待 `receiver.recv()`。GPUI 接收任务持有监听句柄，销毁时取消监听；监听拥有唯一发送端，发送端关闭并排空缓冲后，接收任务检查监听的执行结果并报告异常。监听期间的 D-Bus 错误仍通过 `Observation` 报告。

`service::desktop::Desktop` 提供文件打开和外部终端操作；Linux Portal 连接按需建立，由克隆的 `Desktop` 共享。桌面操作返回 `desktop::Error`，Portal 失败保留为 `PortalError` 及其底层错误；默认应用和选择器均失败时保留两次错误，用户取消是正常结果。

`service::desktop::clipboard::Clipboard` 创建通用 `Reader`：`mime_types()` 列出可用格式，`read(mime)` 按需返回 `Option<Vec<u8>>` 原始字节。它不解码文本、文件或图片，也不限制 MIME 类别；多个格式读取共用截止时间，每份载荷受大小限制。Agents 负责文本兼容、换行处理、本地文件 URI、图片格式选择及附件优先级；Files 负责复制／剪切协议。底层库每次读取重新连接，读取器不代表剪贴板内容的原子快照；Files 组合 KDE 标记与 URI 列表时仍会重读 URI，核对内容是否变化。

剪贴板先提供可用的 MIME 表示，再由消费者选择并请求数据，见 [Wayland 数据提供协议](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_data_offer)。例如同一份内容可以提供 `text/plain` 与 `text/html`，或文件 URI 与复制／剪切标记。当前锁定的 GPUI Wayland 后端从外部剪贴板读取时优先文本、其次图片，不能同时取回两者，也不解析文件 URI；公共服务补充格式级读取能力，其余 GPUI 剪贴板功能继续直接复用。

Files 的 UDisks2 连接、设备操作、重连计时器以及 OpenURI 请求在这个 Tokio runtime 中运行。zbus 5.19 同时编译两种后端时，会根据连接创建时的 Tokio 环境选择后端。GPUI 自身依赖 ashpd 的 `async-io` feature，且 ashpd 不允许同时开启 `tokio` feature；应用通过 ashpd 官方 `OpenFileRequest::connection` 接口注入在 Tokio 中创建的 zbus 连接，避免复用框架可能已在其它执行器初始化的全局 Portal 连接。框架内部仍保留其原有依赖配置。相关机制见 [zbus 运行时文档](https://docs.rs/zbus/5.19.0/zbus/#special-tokio-support) 和 [GPUI 前台／后台执行器说明](https://zed.dev/blog/zed-decoded-async-rust)。

SQLite 入口为 `gds::service::storage::Options`：应用通过 `Options::new(migrations).models(models!(...)).open(path)` 注册模型并打开数据库，返回 Toasty 原生 `Db`（转导出为 `Database`）。Toasty 管理连接池、连接任务、请求队列与事务，本地库的池容量为 1，句柄克隆共享该连接。应用使用 `Model` 派生生成的强类型查询、更新和 upsert 接口；原生 `Json<T>` 字段负责结构化数据的编解码。裸 SQL 留给 SQLite 初始化配置和迁移。数据库操作在应用持有的多线程 Tokio runtime 中运行，GPUI 只等待结果并更新界面。Toasty 的 SQLite 驱动底层仍使用同步 rusqlite，由其连接任务执行。

Files 的 `location/model/address` 定义与页签无关的位置身份：普通目录、最近访问、收藏、网络、回收站、卷列表，以及卷身份加卷内相对路径。`location/api/places/record` 保存位置记录和视图设置；Toasty 模型显式声明 `key_kind`、`key_identity`、`key_path` 三列组成主键，位置类型使用原生 `Embed` 枚举，路径保留非 UTF-8 OS 数据。`view_settings` 按位置保存列布局、以列身份索引的逻辑像素列宽、排序方式、详情栏开关和列表／网格模式；应用默认值也保存在这张表。页签只承载位置的浏览过程，设置不依赖页签 ID。侧栏位置在当前页签导航，特殊位置沿用相同的位置身份和视图设置。Agents 的 `session/api/connection/cache/record` 定义缓存会话、页面与事件模型，快照和事件保留各自的业务类型；页面起始游标单独存列，供覆盖页面查询使用。缓存写入按实际新增事件和替换页面更新会话的字节预算，重复写入不重复计数，淘汰按最近访问顺序执行。

每个存储入口的 `Store` 管理数据库句柄及事务的开启、提交和回滚。事务内的业务读写集中在对应的 `accessor.rs`：`Accessor` 借用当前 `Transaction`，读写方法通过 `&mut self` 使用它；Agents 的访问器同时携带命名空间和缓存容量限制。多个修改与修改后的快照读取共用当前事务。

应用通过 `Options::new(migrations)` 提供 Toasty 原生 `MigrationSet`，每项迁移为带固定 ID、名称及嵌入 SQL 的 `MigrationFile`。初始化直接调用 `MigrationSet::apply(&db)`；待执行项判断、SQL 执行、事务回滚和 `__toasty_migrations` 记录均由 Toasty 管理。SQL 文件使用原生 `-- #[toasty::breakpoint]` 语句分隔。多个进程通过数据库旁的 `.lock` 文件串行初始化；文件锁使用标准库 API，等待锁在 Tokio blocking pool 执行。

默认 WAL、外键约束及 5 秒忙等待；Agents 缓存保留 DELETE journal 和 FULL auto-vacuum。文件准备使用 Tokio 文件 API，服务创建私有数据库文件，不修改已有目录权限。配置文件继续使用 TOML，缩略图继续使用 freedesktop 标准 PNG 缓存。参考 [Toasty 原生迁移管理](https://tokio-rs.github.io/toasty/0.11.0/guide/schema-management.html)、[SQLite 驱动](https://tokio-rs.github.io/toasty/0.11.0/guide/sqlite.html)和[事务接口](https://tokio-rs.github.io/toasty/0.11.0/guide/transactions.html)。

终端服务入口为 `gds::service::terminal`，界面入口为 `gds::ui::terminal`。服务的 `Terminal::prepare(options)` 返回终端句柄和阻塞启动任务 `Launch`；调用方在后台执行 `launch.run()`，随后可等待 `terminal.ready()`，通过 `Session` 读写、resize、观察状态或订阅合并后的更新通知。`shutdown()` 在调用时请求停止，其 future 等待尚未完成的启动及最终 PTY 清理；最后一个所有者释放时也请求清理。启动任务不会持有进程所有者，等待启动的 future 也不会阻止最后一个所有者释放。

GUI 适配器负责把主题调色板和网格尺寸传给服务；GPUI `Keystroke` / `Modifiers` 在 UI 边界转换为服务自己的输入类型。字体、选区绘制、焦点、IME 和窗口状态留在 `ui/terminal`。应用负责安装退出钩子调用 `gds::service::terminal::shutdown_all()`；`gds::ui::terminal::init(cx)` 只注册终端输入上下文的 Tab 行为，其他快捷键和标签页归应用。

公共界面组件仍通过 `gds::ui::sidebar`、`gds::ui::page`、`gds::ui::settings` 等显式入口使用；实现按视觉职责分组，入口不暴露组件内部文件。外观入口为 `gds::appearance::{theme, typography}`，视觉默认值为 `gds::appearance::tokens`，存储机制为 `gds::persistence`。

### 本应用日志

图形应用的启动参数统一由 clap 管理。Files 与 Agents 默认关闭内存日志和日志面板，只有显式传入 `--enable-logs` 才启用：

```sh
gds-files --enable-logs
gds-files --enable-logs --pick-file --filter 'Text:*.txt,*.md' /path/to/folder
gds-agents --enable-logs
gds-agents --enable-logs --compact
# 开发时也可使用 just dev files --enable-logs / just dev agents --enable-logs
```

启用后，公共 `service::logs` 采集本进程的 tracing 事件，`ui::logs::Viewer` 显示同一份队列。两个应用均以 `Ctrl+Shift+L`（macOS 为 `Cmd+Shift+L`）打开独立日志窗口；Files 主菜单也提供入口。未启用时不创建队列、不显示该菜单项，也不注册快捷键或允许直接打开面板。普通错误通知继续显示。终端输入上下文继续保留该按键给 PTY，不触发应用日志窗口。

日志支持级别、来源和文本筛选、键盘选中、全文详情与复制。关闭窗口不会停止采集，重新打开保留记录；向上滚动时停止自动跟随，回到底部继续跟随。每个进程最多保留 2,000 条、约 2 MiB 消息，单条消息最多 64 KiB；清空只影响本进程，不写磁盘或读取其他应用日志。

应用在启动任务前将 `enable_logs` 传给 `service::logs::init_tracing`；返回 `Some(Store)` 时，通过 `ui::logs::install` 安装，再用 `ui::logs::init` 提供应用身份、窗口样式和根视图组装。禁用时保留 stderr 的基本诊断，`ui::logs::store` 返回 `None`，不会隐式创建队列。服务不依赖 GPUI 或 Tokio runtime；更新通知通过独立的有界异步通道发送给每个查看器。`RUST_LOG` 可覆盖过滤规则，但不会自行启用 GUI 日志功能。控制台输出固定走 stderr，Files 选择器的 stdout 仍只返回结果 JSON。开启采集时，公共操作通知及 Files 后台失败也进入日志队列。

## UI 代码组织

原生元素树使用当前锁定的 `gds-uim` 新语法（`.div`、后缀构造器、属性块及条件 children），不是在大段 builder 外只包一层宏。参考锁定提交的 [语法文档](https://github.com/GanDesktop/gds-uim/blob/90f5424edf6ba9c5ffaacc2b11cb5ffe24960519/docs/grammer.md) 和 [示例](https://github.com/GanDesktop/gds-uim/tree/90f5424edf6ba9c5ffaacc2b11cb5ffe24960519/examples)。

公共 `Frame`、导航条目、键帽与设置控件都在公共库中；Files 专属渲染与具名事件处理器留在二进制模块。明显归属对象的构造、状态同步和呈现方法放在对应 `impl` 中。一次性原生显示检查脚本和数据放在临时目录，不保留到项目；常驻测试只覆盖关键工作流与非显然行为。

### UIM 写法

在使用处导入 `el`、`m` 和翻译宏，分别写 `el!`、`m!`、`tr!` / `t!`。元素 ID 统一用 `# expression`。路径、常量和宏调用直接作为 DSL 值；只有字段访问、Rust 方法链、运算等需要 Rust 语义的表达式才使用 `$()`。

```rust
use crate::shared::i18n::tr;
use gds_uim::el;

el! {
    .div {
        # format!("audio-{id}");
        text_size: crate::shared::ui::SMALL;
        text_color: $(cx.theme().muted_foreground);
    } [tr!("audio-stop")]
}
```

### 界面翻译

Fluent 资源按语言存放在各自的 `i18n/assets/en-US/`、`i18n/assets/zh-CN/` 下。语言目录内按功能命名，例如 `core.ftl`、`terminal.ftl`；Files 和 Agents 的基础文案统一使用 `app.ftl`。

公共翻译宏从 `gds::i18n::t`（也可用 `gds::t`）导入，返回 `SharedString`。`@` 标记立即求值的上下文；具名变量支持同名简写、显式表达式、混用和尾逗号，格式化与复数选择交给 Fluent。

```rust
use gds::i18n::t;

t!(@cx, "tabs");
t!(@cx, "items-count", count); // count = count
t!(@cx, "summary", count, name = user.name.as_str(),);

el! {
    t!(@cx, "tabs").SideBarSectionTitle::new { flex_1 }
}
```

省略 `@cx` 时通过固定签名的工厂生成翻译闭包，参数表达式在闭包调用时求值。UIM 可以将 `App` 或 `Context<T>` 的引用传给这个闭包：

```rust
el! { cx.t!("tabs").SideBarSectionTitle::new }
el! { cx.t!("summary", count, name = user.name.as_str()) }
```

工厂返回的 `FnOnce(&App) -> SharedString` 签名为调用参数提供 Rust 的解引用和重新借用上下文，渲染方法中的 `&mut App`、`&mut Context<T>` 因而都可以直接写 `cx.t!(...)`，调用后仍可继续使用 `cx`。动态调用先构造可调用值，再从左到右求值输入，与普通 Rust 函数调用顺序一致。

### 共享对话框底部操作

确认与命名对话框统一使用公共 `DialogActions`：主操作是图标加文字（`Alt+A`），取消是 X 图标加文字（`Alt+C`），Kit 的 `Enter` / `Esc` 仍然有效。组件只派发 Kit 的 Confirm / Cancel，不接管业务回调；各对话框继续自己保留 `on_ok` / `on_cancel` / `on_close`、返回 `bool` 决定是否关闭、冻结目标、验证失败不关闭与关闭后的焦点恢复，也不另外注册一套 Alt 处理。

```rust
footer: $(DialogActions::default().primary_label(label).primary_icon(IconName::Check).cancel_label(cancel_label));
// 危险操作在同一个组件上加 `.danger()`
```

| 对话框 | 主操作图标 |
| --- | --- |
| 列配置（应用） | `Check` |
| `F2` 重命名 | `Pencil` |
| 新建文件夹 | `FolderPlus` |
| 粘贴改名冲突 | `ClipboardPaste` |
| 普通永久删除确认 | `Trash`（危险色） |
| 回收站永久删除 / 清空确认 | `Trash` / `Broom`（危险色） |

标题图标只说明对象，不代替底部主操作图标；图标库没有 `Trash2`，回收站的“清空”因此用 `Broom` 与单项永久删除的 `Trash` 区分。对话框通过此组件统一设置底部操作，不分别设置 `ok_text` / `cancel_text` / `ok_variant`。

### 共享操作通知

公共接口位于 [notifications](../src/ui/feedback/notifications.rs)。窗口创建时调用 `gds::ui::notifications::bind_window(window, cx)`，组件或后台结果回到 GPUI 上下文后即可调用 `error` / `warning` / `info` / `success`，不需要 Files 的 `Browser` 引用：

```rust
gds::ui::notifications::error("操作失败的原因", cx);
```

带明细的报告使用 `Report`，异步操作的开始 / 结束使用 `Operation` 复用通知槽。消息由套件共享的 Kit 通知队列持有，由应用根视图挂载的覆盖层平铺渲染，不改变文件列表和设置页高度；完整摘要与明细在有界区域内滚动。错误保持到用户关闭，其余沿用 Kit 自动消退。具体位置、材质与交互约定见 [统一通知语言](vistual-language/README.md#通知气泡与共享接口)。

## 设置与状态

- 应用配置（Files）：`$XDG_CONFIG_HOME/gds/files/settings.toml`，默认 `~/.config/gds/files/settings.toml`，与 `state/files` 同层。设置真实变化时立即请求应用并自动保存；重复选择同值不写入，程序回写不再发起应用请求。应用失败恢复上一次有效偏好并报错；持久化失败保留已生效的运行态，明确提示“已生效但未保存”。声明式符号链接或只读目标允许读取，但不被替换。应用与保存失败通过共享右上角通知气泡反馈，不插入设置页内容，也不另设全宽保存底栏。
- 旧套件配置 `$XDG_CONFIG_HOME/gds/settings.toml` 只作为一次性导入源：仅当应用配置不存在时读取，并在新路径原子创建副本；新配置存在时完全以新文件为准。旧文件绝不覆盖或删除；旧配置损坏时报错而不写默认值；复制失败时保留已读取的旧偏好用于本次运行并给出启动提示，下次启动重试；并发创建的新配置不会被导入覆盖。应用设置不跨应用同步。
- Files 状态：`$XDG_STATE_HOME/gds/files/state.sqlite`，默认 `~/.local/state/gds/files/state.sqlite`。`locations` 保存收藏、最近访问和固定位置，远程 URI 使用独立记录；`view_settings` 保存应用默认值与各位置的列布局、列宽、排序、详情栏开关和列表／网格模式。列编辑器默认只作用于当前位置；“本应用默认”在同一 Toasty 事务中更新数据库默认列并清除当前位置的列覆盖，其它视图字段保留。`settings.toml` 保存主题、字体、语言、隐藏文件、侧栏等应用偏好，列与视图状态统一由 SQLite 管理。
- Agents 的连接历史缓存继续位于 `$XDG_CACHE_HOME/gds/agents/history-v1/history.sqlite`（默认 `~/.cache/gds/agents/history-v1/history.sqlite`）。数据库执行接入公共 `service::storage`，窗口边界、连接命名空间和 512 MiB 淘汰策略仍由 Agents 管理；缓存可删除重建，不与 Files 的持久状态共库。
- Qt 来源沿实际 qtct 配置中的 `color_scheme_path` 读取，不硬编码某个桌面生成文件。DMS 来源读取已经生成的 `DankMaterialShell/dms-colors.json`。缺失 / 不支持 / 格式错误会报错，不把内置主题伪装成成功跟随。
- 外部主题在启动或主题 / 明暗选择变化时读取。DMS 的跟随系统模式读取导出的活动模式；内置 / Qt 读取平台明暗偏好。`theme::Observer` 统一监听 DMS、Qt 配置及解析后的色板路径，跟随原子替换与托管符号链接；后台读取成功后才安装，失败保留最后有效主题，旧来源结果不覆盖新选择。透明度变化只更新材质，不重读色板。
- 透明度仅影响表面；窗口允许 alpha，Root 清除不透明垫底。整条左列（顶栏左区、文件或设置侧栏、终端左区、底部留白）各区域仅绘制一次相同侧栏背景，不叠加内容底色；右侧区域独立绘制内容背景。底部左区没有分割顶线。没有启用或宣称支持 Niri 的桌面背板模糊。

## 文件选择器

`gds-files --pick-file [DIRECTORY]` / `--pick-directory` 提供本地文件或目录选择；`--multiple` 支持多选。文件模式支持重复 `--filter` 与 `--read-only`。确认后 stdout 输出一行 JSON，取消无结果输出；诊断使用 stderr。Portal 通过 `--portal-request` 使用独立的结构化请求协议，支持打开与保存。完整参数与结果见 [文件选择器](apps/files/picker.md)。

## 内置终端

Files 和 Agents 已接入共享的本地 PTY 终端。`Ctrl+反引号` 显示／收起面板，有效本地目录的空集合在显式展开时自动新建一个终端；`+`（“新建终端 / New terminal”）继续增加实例，关闭最后一个后重绘不自动重启。Files 在窗口级独立持有集合及活动实例，切换 / 关闭文件标签不改变终端；新建使用当前可用真实目录，否则 Home；F2 重命名实例且自定义名不受 shell 标题影响；Agents 由本地连接和工作目录对应的 workspace 提供集合、实时标题与独立分隔尺寸，终端控制区关联其导航组，PTY 面板只拆分右侧工作内容区。终端输入区的 `Ctrl+C/W/L`、Tab 等交给 shell，复制／粘贴用 `Ctrl+Shift+C/V`，`F6` 返回文件或对话区域。

隐藏、切换标签或打开设置不结束进程；关闭终端或窗口不弹确认，直接终止并等待受管终端清理；Files 的文件标签不拥有终端。Shell、字体、字号和滚屏仍从设置中心的终端页配置，终端区域不重复显示设置齿轮；外部模拟器配置仍独立。远程/TCP Agent 上下文不启动本机 shell，picker 不提供内置终端。

Files 的终端标题栏提供显式“在新终端标签页打开当前文件夹”，新建实例而不改变原 shell、输入和作业；文件导航及目录动作均不向已有 PTY 注入 `cd`。实现边界与配置见 [共享终端](apps/terminal.md)；当前不包含远程 shell、退出后保活或 Agent 执行工具权限。

## 外部终端

优先使用 `GDS_TERMINAL` 指定的**单个可执行程序**，继承用户会话环境并设置工作目录；该变量不是 shell 命令或带参数的字符串。

```sh
GDS_TERMINAL=alacritty cargo run --bin gds-files --features files
```

未设置时调用 `xdg-terminal-exec --dir=…`。Nix 发布包与开发环境均提供该启动器，其它安装方式需要将它加入运行环境；不存在时应用会说明失败，不猜测终端模拟器的参数，也不硬编码用户 shell。

## 检查边界

日常检查使用 `just fmt-check`、`just check` 和 `just test`；只检查某个应用时显式传入其 feature 和 binary target。需要 GPUI 测试支持的回归使用 `gui-tests`。编译、单元测试与无显示服务器的 GPUI 测试不能代替目标桌面的输入、IME、父窗口和材质验收。

文件操作和配置写入的验证使用独立临时目录与 XDG 环境。一次性验证脚本、日志、截图和逐轮结果不纳入项目文档。文档描述当前行为、接口与限制；数据库和配置的运行时迁移机制仍按其兼容契约维护。
