# GDS 内置终端

Files 和 Agents 复用 `gds::service::terminal` 服务和 `gds::ui::terminal` 界面。执行在 GDS 所在主机，支持 Linux 本地 PTY；不增加后台 daemon、远程 shell 或 Agent 执行工具权限。

## 分层与依赖

See [Terminal backend boundary](../development/terminal-backend.md) for the audited upstream reuse decision, retained host contracts and validation limits.

```text
Files / Agents：终端集合、标签页、Dock、工作目录和快捷键
                          │
                          ▼
             ui::terminal::Terminal（GPUI 适配器）
               焦点 / 字体 / 网格绘制 / IME
                          │ 普通输入类型、状态与通知
                          ▼
             service::terminal::Terminal（服务所有者）
               启动任务 / 会话 / 关闭与等待
                          │
                  Session / Alacritty 状态
                          │
                  portable-pty / shell
```

- `portable-pty 0.9.0` (MIT) owns PTY allocation/handles; `alacritty_terminal 0.26.0` (Apache-2.0) supplies the terminal model, grid and parser. One worker uses Tokio readiness/control/child-exit notifications; no parallel Alacritty event loop or fixed-interval PTY polling is added.
- Unix 子进程启动使用 `std::process::Command` 连接该 PTY，并在 exec 时关闭额外描述符。启动适配保留 Rust 的 exec 错误管道，以便正确报告无效解释器等启动错误。
- GPUI 视图使用 GDS 当前 GPUI Kit，终端实现不包含另一套 GPUI 或 Zed 的 GPL `terminal` / `terminal_view`。
- 服务模块不引用 GPUI、界面组件、Files 标签类型或 Bas 会话类型，也不绑定 Agents 的 Tokio runtime。调用方选择启动任务的后台执行器。
- `ui::terminal` 保存界面偏好和观察状态，通过服务接口启动、读取和停止进程。shell / argv / scrollback 转换成服务的 `SpawnOptions`；GPUI 输入事件转换成服务输入类型。
- 应用分别保存配置，并在应用退出时安装 `service::terminal::shutdown_all()` 清理钩子。

## Files

- 窗口持有唯一终端会话集合与活动实例，不属于任何文件标签、Home 或卷页。切换和关闭文件标签不切换或结束终端，关闭终端也不关闭文件标签。
- 底栏终端按钮是唯一窗口级显示 / 收起入口，选中态与 toggle 语义反映显示状态，tooltip 和可访问名称按状态区分“显示终端 / 收起终端”；保留 `Ctrl+反引号`，dock 内不再重复开关。显示 dock 时，有效本地目录的空集合自动新建并聚焦一个终端；已有实例不重复创建。关闭最后一个实例后重绘不重启，下一次显式展开才补建。
- 新建终端使用当前浏览目录；右键“在此打开终端”使用明确目标：目录自身或文件父目录。目录符号链接按其目录目标处理，多选不猜测其中一个。
- 切标签、隐藏 dock、打开设置不结束 shell。文件导航不自动向已有 shell 注入 `cd`，shell 的目录改变也不自动导航文件列表。
- 标题栏的“在新终端标签页打开当前文件夹”新建并激活实例，保留原 shell、输入和作业；绝不向已有 PTY 注入 `cd` 或 Enter。
- 新建实例使用当前可用的真实本地目录；虚拟集合、失效、未挂载或非目录位置回退 Home。picker 不提供内置终端。
- 终端实例支持 F2 重命名，确认后自定义名不被 shell 标题更新覆盖；取消不改变名称。新建按钮与文件新建标签按钮样式一致，位于横跨列表与输出区的统一标题行，紧邻终端 section title。
- 关闭终端或窗口不弹确认，等待受管终端清理后移除宿主；关闭文件标签不触发终端清理。
- “在外部终端打开”走桌面 launcher，与内置终端分开。`GDS_TERMINAL` 指定外部模拟器可执行文件，不是内置 shell 配置。

文件浏览与导航约定见 [Files](files/README.md)。

## Agents

- 每个连接中由侧栏菜单选中的 workspace 持有终端集合、活动实例及 dock 分隔尺寸，不存放在会随对话切换而重置的选中对话状态中。切换 workspace 后恢复各自实例与实时标题；同一 workspace 内切换会话或会话目录变化不切换终端集合。
- 终端 dock 位于工作区下方并跨两列，与 Files 一致：左列放当前 workspace 的终端页签，与应用侧栏共享宽度和折叠状态；右列显示活动终端。新建按钮位于左列标题行，右列标题栏提供关闭当前实例与多实例切换菜单。历史／收藏视图、导航组折叠或搜索过滤不影响底部终端页签；切换 workspace 恢复各自实例，操作不改变其它 workspace。
- 同 Files，底栏选中态 Terminal 按钮与 `Ctrl+反引号`是唯一显示 / 收起入口；dock 不再重复开关或设置齿轮。设置中心的终端配置页仍保留。
- 只允许明确的本地 IPC 上下文。TCP（包括 loopback）不会仅因地址像本机就获得本地执行能力；远程 Agent cwd 不会被拿到 GDS 主机执行。
- 紧凑模式打开终端时切换到完整工作区。设置页、连接切换和对话归档不结束工作区 shell。
- 关闭终端、窗口和退出应用均不弹确认。窗口关闭等待所有后端工作区（包括未显示工作区与已关闭页签）的终端清理；应用退出先刷新待保存设置，再等待全部终端清理。
- Agent 工具输出继续是会话历史的只读展示；认证终端仍使用原认证流程。这两者不因内置终端获得新的执行或输入权限。

## 宿主控件与提示

- 两应用的 dock 均位于工作内容区下方并跨两列，左列页签与应用侧栏共享宽度。Files 的最大化 / 还原作用于整个 dock；Agents 的 dock 管理当前 workspace 的实例。
- 两应用终端实例使用公共 `NavItem` 的点击区、图标、内边距、hover 与选中前景；新建动作均使用终端标题行图标按钮，页签列与应用侧栏对齐，不重复铺半透明背景。展开行的关闭按钮固定占位，只在本行 hover 或适用的活动实例 Ctrl 提示时显示，显隐不挤动标题；关闭按钮不触发行选择。折叠列的实例在右上角显示与 Files 文件页签共用的关闭小按钮，hover 或活动快捷键提示时可操作；仍可选择实例并在终端导航作用域使用 `Ctrl+W`。
- 标题文字与 PTY 共用 `TERMINAL_CONTENT_INSET`；头部使用公共 `TERMINAL_HEADER_HEIGHT`，32 px 动作按钮、16 px 图标、4 px 按钮间距和上下各 4 px 留白。关闭、新建、切换、切目录和最大化均使用已实际嵌入的语义图标，短 tooltip 与可访问名称一致。Agents 切换菜单只在多实例时出现，不能在单实例留下无意义的占位。
- 长标题单行省略；悬停使用公共 `ui::tooltip::Details`，标题、完整启动目录、状态分行呈现。提示随窗口边界限制宽高，长 token 可折行、超高内容内部滚动，不把全部信息拼成一条长行；“启动目录”不冒充 shell 当前目录。按钮提示只保留短动作名和有效快捷键，不拼入路径、状态或大段说明。

## 输入和生命周期

终端输入区与终端页签栏是不同焦点作用域。PTY 输入区的 `Ctrl+C/D/N/W/L/B/R`、Escape、方向键、Tab 等交给 shell / 前台程序，不触发文件操作或 Agents 导航。终端复制、粘贴使用 `Ctrl+Shift+C/V`，`F6` 返回宿主内容区；显式窗口动作 `Ctrl+,` 和 `Ctrl+反引号` 保留。模态对话框优先。详见 [套件焦点契约](../vistual-language/shortcuts.md#4-焦点与-scope-契约)。

进程创建、读取、写入、resize 和等待退出不阻塞 GPUI 主线程。渲染不创建进程；输出以字节流解析，UI 通知合并，滚屏有上限。隐藏不发送零行零列 resize，也不丢弃会话状态。

shell 来源为显式设置、账户默认 shell、有效 `$SHELL` 兜底；默认交互式非 login。Nix 开发环境注入的非交互 Bash 不覆盖账户 shell。GDS 不复制、改写或手工 source 用户 rc。新实例的目录通过子进程启动参数设置，不拼接 `cd` 命令；启动目录失效时报告失败而不是退回 Home。Files 的显式目录操作同样新建实例，不向已有 PTY 注入目录命令。

普通 PTY 不是沙箱，shell 具有当前桌面用户的权限。终端输出也不是可信控制命令：不自动执行链接、不会因为 OSC 52 请求就读写系统剪贴板。终端输入和原始输出不自动写入聊天历史或持久日志。

## 配置和打包

共享终端配置类型嵌入各应用自己的设置，旧配置通过默认值兼容。shell / 参数修改仅影响新会话；显示偏好可以应用于当前会话。终端字体应使用等宽字体，与普通 UI 字体独立。

Nix 包和开发环境提供 `xterm-256color` 的 terminfo，并保留用户的 `TERMINFO_DIRS` 搜索路径；不硬编码 `/bin/bash` 或依赖某个终端模拟器已安装。Files 默认依赖图仍不需要 Bas。

## 暂不包含

- SSH 终端、远程 Bas Terminal RPC、跨应用接管同一个 shell。
- GDS 退出后的持续运行和崩溃恢复；有此需求应显式使用 tmux 等独立机制。
- 自动将命令发送给 Agent，或把整个终端记录上传到对话。
- 自动跟随文件导航切换 shell 目录；向前台程序注入切换命令；未经 shell integration 声称能可靠检测提示符是否空白。
- Kitty 图形协议等扩展，以及未经实机验收的 Windows / macOS 支持承诺。

## 显示与集成原则

Terminal palette sources, fallback and live synchronization: [Terminal appearance contract](../development/terminal-theme.md).

- 字体使用系统字体服务解析实际等宽家族，不把 CSS 的 `monospace` 名称直接交给 GPUI 后任其回退到比例字体。网格尺寸来自实际字体 advance / ascent / descent，并对齐设备像素。
- 每行按字形样式批量 shaping；glyph cluster 映射回终端列，保留组合字符内部偏移。中英文、Nerd Font 符号、彩色 emoji、光标和 IME 共享同一列坐标与基线，fallback 字体不改变下一列的位置。
- 默认背景由 Grid 按 GDS 的 content 材质绘制一次。普通默认 cell 不再填背景，应用的多终端布局不在 Grid 下方重复铺底；显式 ANSI 背景、反色、选择和光标是独立绘制层。Dim 改变 RGB，不降低文字 alpha。
- 默认前景、背景、选择和 ANSI 语义色跟随当前主题；OSC 颜色查询与显示使用同一调色板，并在 shell 启动前提供初始值。
- 公共层只提供单终端实例：异步启动、PTY 生命周期、画面、输入、选择与滚屏。终端集合、活动实例、新建 / 关闭 / 切换动作、页签 UI、Dock 和关闭策略均由各应用拥有；布局复用通用 `ui::Frame` / `ui::split`，不提供共用终端工作台。
- 公共层暴露 Copy / Paste / Scroll 能力动作，由应用显式绑定自己的按键；F6 等宿主导航不是终端事件。公共初始化只为 `GdsTerminal` 安装 Tab / Shift+Tab 的 `NoAction` 路由例外，阻止 Kit Root 提前把按键作为焦点遍历处理，让原始事件到达 PTY 编码器；终端外部保留正常遍历。未被宿主消费的按键按终端协议传给前台程序。

## 平台边界

Windows PTY 后端返回不支持；macOS 和 Wayland 实机 IME 的完整支持尚未确立。其他 Unix 的后台作业回收弱于 Linux；已脱离 PTY 会话或提权的进程不保证由普通用户回收。普通窗口关闭等待清理，应用退出钩子只作兜底，不提供崩溃恢复。
