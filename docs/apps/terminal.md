# GDS 内置终端

Files 和 Agents 复用 `gds::terminal`，而不是嵌入外部终端窗口。执行在 GDS 所在主机，第一阶段目标是 Linux 本地 PTY；不增加后台 daemon、远程 shell 或 Agent 执行工具权限。

## 分层与依赖

```text
Files：每标签终端集合、页签 / Dock UI、管理快捷键、关闭策略
Agents：每工作目录终端集合、页签 / Dock UI、管理快捷键、关闭策略
                       │ 各自持有多个实例
                       ▼
           terminal::Terminal（单终端）
                       │
             View / Input → Session
                              │
                     Alacritty 屏幕模型
                              │
                     portable-pty / shell
```

- `portable-pty 0.9.0`（MIT）负责 PTY；`alacritty_terminal 0.26.0`（Apache-2.0）负责终端状态与字符网格。只使用一套 PTY I/O，不再启动 Alacritty 自己的 PTY event loop。
- Unix 子进程启动使用 `std::process::Command` 连接该 PTY，并在 exec 时关闭额外描述符。真实失败测试发现 portable-pty 0.9 的 Unix spawn 会提前关闭 Rust 的 exec 错误管道，使无效解释器表现为子进程 abort；应用侧启动适配保留错误管道，不修改依赖源码。
- GPUI 视图使用 GDS 当前 GPUI Kit，未引入第二套 GPUI、vendor、源码补丁或 Zed 的 GPL `terminal` / `terminal_view`。
- 公共模块不依赖 Files 标签类型、Bas 会话类型或 Agents 的 Tokio runtime。应用配置仍分别由 Files 和 Agents 保存。
- [Tty7](https://github.com/l0ng-ai/tty7) 的 I/O / 屏幕 / 视图分层，以及 [Zed 终端](https://github.com/zed-industries/zed/tree/main/crates/terminal_view) 的交互与只读模式是架构参考，不是移植这些应用的 daemon 或上层组件。

## Files

- 窗口持有唯一终端会话集合与活动实例，不属于任何文件标签、Home 或卷页。切换和关闭文件标签不切换或结束终端，关闭终端也不关闭文件标签。
- 底栏终端按钮是唯一窗口级显示 / 收起入口，选中态与 toggle 语义反映显示状态，tooltip 和可访问名称按状态区分“显示终端 / 收起终端”；保留 `Ctrl+反引号`，dock 内不再重复开关。显示 dock 时，有效本地目录的空集合自动新建并聚焦一个终端；已有实例不重复创建。关闭最后一个实例后重绘不重启，下一次显式展开才补建。
- 新建终端使用当前浏览目录；右键“在此打开终端”使用明确目标：目录自身或文件父目录。目录符号链接按其目录目标处理，多选不猜测其中一个。
- 切标签、隐藏 dock、打开设置不结束 shell。文件导航不自动向已有 shell 注入 `cd`，shell 的目录改变也不自动导航文件列表。
- 标题栏的“在新终端标签页打开当前文件夹”新建并激活实例，保留原 shell、输入和作业；绝不向已有 PTY 注入 `cd` 或 Enter。
- 新建实例使用当前可用的真实本地目录；虚拟集合、失效、未挂载或非目录位置回退 Home。picker 不提供内置终端。
- 终端实例支持 F2 重命名，确认后自定义名不被 shell 标题更新覆盖；取消不改变名称。新建按钮与文件新建标签按钮样式一致，位于横跨列表与输出区的统一标题行，紧邻终端 section title。
- 关闭终端或窗口不弹确认，等待受管终端清理后移除宿主；关闭文件标签不触发终端清理。
- 原“在外部终端打开”仍走桌面 launcher，与内置终端分开。`GDS_TERMINAL` 指定外部模拟器可执行文件，不是内置 shell 配置。

完整产品约定见 [Files 概念设计](files/concept.md#7-终端真实-shell而非嵌入外部窗口)。

## Agents

- 每个连接与工作目录对应的 workspace 持有终端集合、活动实例及 dock 分隔尺寸，不存放在会随对话切换而重置的选中对话状态中。切换 workspace 后恢复各自实例与实时标题。
- 终端 section 与实例列表嵌在所属 workspace 的导航组中，和组内会话采用同层缩进；PTY dock 仅拆分右侧工作内容区，不在整个应用侧栏下方另设独立区域。折叠导航组不结束终端；dock 标题栏提供关闭当前实例与新建按钮，有多个实例时再显示切换菜单，零或一个实例不留空占位。即使在历史／收藏视图、侧栏隐藏、导航组折叠或搜索过滤时也可管理已有实例，操作不改变其它 workspace。
- 同 Files，底栏选中态 Terminal 按钮与原快捷键是唯一显示 / 收起入口；dock 不再重复开关或设置齿轮。设置中心的终端配置页仍保留。
- 第一阶段只允许明确的本地 IPC 上下文。TCP（包括 loopback）不会仅因地址像本机就获得本地执行能力；远程 Agent cwd 不会被拿到 GDS 主机执行。
- 紧凑模式打开终端时切换到完整工作区。设置页、连接切换和对话归档不结束工作区 shell。
- 关闭终端、窗口和退出应用均不弹确认。窗口关闭等待所有后端工作区（包括未显示工作区与已关闭页签）的终端清理；应用退出先刷新待保存设置，再等待全部终端清理。
- Agent 工具输出继续是会话历史的只读展示；认证终端仍使用原认证流程。这两者不因内置终端上线获得新的执行或输入权限。

## 宿主控件与提示

- Files 的 dock 位于文件区下方并跨两列，左列与文件侧栏共享宽度；最大化 / 还原仍作用于整个 dock。Agents 不复用这套跨列布局，导航始终归所属 workspace，dock 管理当前 workspace 的活动实例。
- 两应用终端实例和“新建终端 / New terminal”使用公共 `NavItem` 的点击区、图标、内边距、hover 与选中前景；Agents section 与同组会话缩进对齐，不在 section 下重复铺半透明背景。展开行的关闭按钮固定占位，只在本行 hover 或适用的活动实例 Ctrl 提示时显示，显隐不挤动标题；关闭按钮不触发行选择。Files 折叠列仍可选择实例并在终端导航作用域使用 `Ctrl+W`。
- 标题文字与 PTY 共用 `TERMINAL_CONTENT_INSET`；头部使用公共 `TERMINAL_HEADER_HEIGHT`，32 px 动作按钮、16 px 图标、4 px 按钮间距和上下各 4 px 留白。关闭、新建、切换、切目录和最大化均使用已实际嵌入的语义图标，短 tooltip 与可访问名称一致。Agents 切换菜单只在多实例时出现，不能在单实例留下无意义的占位。
- 长标题单行省略；悬停使用公共 `ui::tooltip::Details`，标题、完整启动目录、状态分行呈现。提示随窗口边界限制宽高，长 token 可折行、超高内容内部滚动，不把全部信息拼成一条长行；“启动目录”不冒充 shell 当前目录。按钮提示只保留短动作名和有效快捷键，不拼入路径、状态或大段说明。

## 输入和生命周期

终端输入区与终端页签栏是不同焦点作用域。PTY 输入区的 `Ctrl+C/D/N/W/L/B/R`、Escape、方向键、Tab 等交给 shell / 前台程序，不触发文件操作或 Agents 导航。终端复制、粘贴使用 `Ctrl+Shift+C/V`，`F6` 返回宿主内容区；显式窗口动作 `Ctrl+,` 和 `Ctrl+反引号` 保留。模态对话框优先。详见 [套件焦点契约](../vistual-language/shortcuts.md#4-焦点与-scope-契约)。

进程创建、读取、写入、resize 和等待退出不阻塞 GPUI 主线程。渲染不创建进程；输出以字节流解析，UI 通知合并，滚屏有上限。隐藏不发送零行零列 resize，也不丢弃会话状态。

shell 来源为显式设置、账户默认 shell、有效 `$SHELL` 兜底；默认交互式非 login。Nix 开发环境注入的非交互 Bash 不覆盖账户 shell。GDS 不复制、改写或手工 source 用户 rc。新实例的目录通过子进程启动参数设置，不拼接 `cd` 命令；启动目录失效时报告失败而不是退回 Home。Files 的显式切换操作是唯一允许向已有实例提交转义后 `cd` 命令的宿主行为。

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

- 字体使用系统字体服务解析实际等宽家族，不把 CSS 的 `monospace` 名称直接交给 GPUI 后任其回退到比例字体。网格尺寸来自实际字体 advance / ascent / descent，并对齐设备像素。
- 每行按字形样式批量 shaping；glyph cluster 映射回终端列，保留组合字符内部偏移。中英文、Nerd Font 符号、彩色 emoji、光标和 IME 共享同一列坐标与基线，fallback 字体不改变下一列的位置。
- 默认背景由 Grid 按 GDS 的 content 材质绘制一次。普通默认 cell 不再填背景，应用的多终端布局不在 Grid 下方重复铺底；显式 ANSI 背景、反色、选择和光标是独立绘制层。Dim 改变 RGB，不降低文字 alpha。
- 默认前景、背景、选择和 ANSI 语义色跟随当前主题；OSC 颜色查询与显示使用同一调色板，并在 shell 启动前提供初始值。
- 公共层只提供单终端实例：异步启动、PTY 生命周期、画面、输入、选择与滚屏。终端集合、活动实例、新建 / 关闭 / 切换动作、页签 UI、Dock 和关闭策略均由各应用拥有；布局复用通用 `ui::Frame` / `ui::split`，不提供共用终端工作台。
- 公共层暴露 Copy / Paste / Scroll 能力动作，由应用显式绑定自己的按键；F6 等宿主导航不是终端事件。公共初始化只为 `GdsTerminal` 安装 Tab / Shift+Tab 的 `NoAction` 路由例外，阻止 Kit Root 提前把按键作为焦点遍历处理，让原始事件到达 PTY 编码器；终端外部保留正常遍历。未被宿主消费的按键按终端协议传给前台程序。

## 验收清单

1. 真实 PTY：`tty`、标准流 TTY、中文 UTF-8 / IME、Ctrl+C 中断前台而保留 shell、vim / less alternate screen、resize 后 `stty size`。
2. 输入隔离：预选文件后在终端输入 Ctrl 组合、Delete、Escape、方向键，不复制、删除、收藏或导航文件；Agents 不误切会话或折叠窗口。
3. 目录：空格、引号、非 ASCII、目录符号链接、失效目录、未挂载卷、虚拟页；异步启动期间切页不改变已捕获的目标。
4. 生命周期：隐藏、设置、切标签 / 后端保持进程；关闭终端、宿主标签、WM 关窗和应用退出不弹确认并回收受管资源；晚到事件不串会话。
5. 吞吐与边界：持续输出不无限占内存或卡 UI；多个终端的输入和 resize 不串台；旧配置兼容，远程上下文拒绝本地执行。

## 首轮基础功能记录（2026-10-28，不代表显示质量）

以下受控场景只证明 PTY 与生命周期的基础行为。首轮显式指定测试字体、跳过用户 rc，并使用不透明材质，遗漏了默认字体、真实提示符和透明主题路径；之后用户截图暴露了字距、shell 选择与材质分层问题，因此不能据此宣称终端显示已经达标。

- `cargo test --workspace --features gui-tests --locked --no-fail-fast`：**699 项通过、2 项原有性能测试忽略**（共享库 84、Agents 419、Files 194、derive 2）。覆盖真实 PTY、分片 UTF-8、ANSI、查询回复、resize、Ctrl+C、启动失败、进程回收、输入背压、输出洪流、退出屏障、重复 shutdown、焦点路由和远程上下文拒绝。
- `cargo check --bin gds-files --no-default-features --locked`、`cargo build --bins --features agents --locked`、`cargo fmt --all --check` 和 `git diff --check` 均通过。Files 默认依赖图不包含 Bas；终端新增代码没有编译警告，应用原有 dead-code 警告仍保留。
- 在独立 Xvfb / 私有 D-Bus / 临时 HOME 与 XDG 目录中运行真实二进制：两个应用均成功启动本地 shell，标准输入、输出、错误均是 TTY，cwd 正确。未连接真实 Bas 服务、未调用模型、未修改用户配置。
- Files 原生验证了控制键不误关文件标签、Ctrl+C 中断 `sleep` 后 shell 仍可用、拖动分隔条后 `stty size` 从 12 行更新为 19 行、中文宽字符显示、`less` alternate screen，以及隐藏 / 设置 / 取消关闭后保持相同 shell PID。确认关闭后 PID 消失。
- 最终布局复验确认：上半区缩小时文件标签仍可达、位置组可滚动，终端 header / 分隔区使用对应主题材质，关闭确认不透出底层文字。最终脚本再次验证 Files 取消保活与确认回收，以及 Agents 显式确认退出等待 shell 回收后结束。
- Nix 开发环境的 `xterm-256color` terminfo 查询和 Files 包派生求值通过；本轮未重新执行完整 Nix release 构建。

上述不等于全平台验收：**Wayland 实机 IME 候选窗口、macOS、Windows 未验收**。当前 Windows PTY 后端明确返回不支持。其他 Unix 的后台作业回收弱于 Linux；已脱离 PTY 会话（如主动 `setsid`）或提权的进程不保证由普通用户强制回收。普通窗口关闭会等待清理，GPUI 的 200ms 应用退出 hook 仅作兜底，不能作为崩溃恢复保证。

当前关闭策略已取消 Agents 既有退出确认界面，保留设置刷新和异步终端清理屏障。上述首轮取消 / 确认交互仅为历史记录，不代表当前行为。

## 显示层与宿主边界重构

默认字体改为通过与 GPUI 相同版本的字体服务解析系统实际等宽家族；行 shaping 与列定位分离，以实际度量的设备像素网格绘制，而非将比例字体字形逐个摆在 `M` 的宽度上。组合字符、中文宽字符、Nerd Font 符号、emoji 与 IME 标记区使用同一套列与基线坐标。

默认背景只合成一次，文本不跟随表面透明度变淡。ANSI 显式背景、选择、反色和光标单独绘制；OSC 查询在 shell 启动阶段就使用当前主题。Files 与 Agents 各自组合终端集合和 Dock；列对齐与表面使用通用 Frame / Split，不在单终端组件里预置一套工作台，也不使用绝对定位的背景补丁。

默认 shell 误选的根因是 `$SHELL` 被 Nix 注入为缺少 readline / programmable completion 的 Bash 构建；`progcomp` 错误和字面 `\\[ / \\]` 均由此产生。现在显式配置优先，默认走账户 shell，并正常读取 rc，不通过关闭 rc 或过滤提示符掩盖问题。

宿主统一使用“新建终端 / New terminal”作为按钮与 tooltip 文案。终端区域不重复提供设置齿轮，字体、字号和 Shell 等仍从应用设置中心的终端页配置。侧栏 section 文字使用公共 `SIDEBAR_PADDING` 与 `SIDEBAR_SECTION_PADDING`；终端窗口标题与 PTY 网格共用来自内容行间距的 `TERMINAL_CONTENT_INSET`，不随 UI 字号的 rem 换算产生不同边缘。代码布局检查不等于原生画面对齐验收；第二轮真实 GUI / 实机验证状态应单独记录。

重构后的原生画面使用默认 `monospace` 偏好、账户 shell、正常 rc 和半透明主题，展示了正常字距、中文 / 组合字符 / 彩色符号及无转义残留的提示符。此结果不替代仍未完成的 Wayland 实机 IME 与跨平台支持工作。

## 第二轮焦点与布局验证边界

功能合并提交 `960b2c2`（在上一轮 `641771c` 上继续）的验证结果：

- Nix 开发环境内 `cargo build --bins --features agents --locked` 通过，两应用二进制均构建成功；`cargo test --workspace --features gui-tests --locked` 共 **719 项通过、2 项原有性能测试忽略**（共享库 92、Agents 425、Files 200、derive 2）。`cargo fmt --all --check` 与 `git diff --check` 通过，保留原有 dead-code 警告。
- 关键行为覆盖 workspace 实例／标题／分隔尺寸保留、隐藏时启动取消及清理屏障，以及侧栏缺少终端行时由 dock 菜单切换／关闭同一实例。Files 覆盖实例隔离、显式目录动作新建会话且保留原 shell 输入，以及真实目录与 Home 回退的启动选择。
- Tab 回归先在旧路由上复现“终端失去焦点”，再通过终端专用路由例外修复。GPUI 测试窗口实际分发 Tab / Shift+Tab 到真实 PTY，断言收到 `09` / `1b5b5a`，焦点仍在输入区；外部控件的正向／反向焦点遍历继续有效。另用交互式、启用 readline 的 Bash 验证部分文件名经 Tab 补全后能读取目标内容，不只是单测按键编码函数。
- 布局代码使用宿主公共间距：Files 终端 section 与 Tabs / Locations 均为侧栏外层 10px 加标签内层 6px；Agents 所属 workspace 下的终端 section 与主侧栏 section 同样复用这两层间距。两应用终端窗口标题与网格使用内容行公共 12px 基准，分栏线是覆盖层，不额外占用内容宽度。中文／英文新建入口同为“新建终端 / New terminal”。
- **本轮 Tab 补全和边线对齐未做真实原生 GUI / Wayland 实机验证。** 执行环境未提供 `DISPLAY` / `WAYLAND_DISPLAY`。上述焦点测试是 headless GPUI 测试窗口加真实子进程，不是屏幕录制、像素验收或用户桌面实机通过；历史原生截图也不能代替本轮改动的验收。
