# Files TODO

更新：2026-11-01。

P1 表示已有功能的正确性问题；P2 表示互操作能力与覆盖范围。FileManager1 是“系统默认文件管理器”里程碑的必要能力，可按该里程碑提高产品优先级。

按 FM-01、FM-02、FM-03 推进，FM-04 随这些任务积累验收结果。FileChooser 以完整原生实现为交付目标；fallback 只在查明原生确实无法实现的限制后考虑。FM-01、FM-02、FM-03 的实现已落地，自动验证和未实测项见文末。桌面互通验收用于记录和报告，不作为本轮交付门槛；未通过项由开发者决定是否继续投入，不据此自行扩展功能。

## FM-01 回收站解析与还原位置（P1）

- [x] 修正真实 `.trashinfo` 读取流程中的规范偏差。

要求：

- 重复 `Path` / `DeletionDate` 使用第一次出现的值；注释、空行和未知扩展行按规范忽略，枚举不因这些行 panic。
- home trash 的相对路径以 `$XDG_DATA_HOME` 为基准；卷回收站分别正确处理 `.Trash-$uid` 和 `.Trash/$uid`。保留原始路径字节，拒绝规范不允许的相对 `..`。
- 优先采用已有上游修复或聚焦的依赖补丁，明确无效记录与整个存储读取失败的处理。
- 保留恢复不覆盖现有目标、部分失败保留源、记录变更检测等已有行为。

验收：使用隔离 XDG 目录中的实际 `info/*.trashinfo` 与 `files/*`，从生产枚举入口读取，再确认或执行还原。覆盖重复键、相对路径、注释、非 UTF-8 名称、同名冲突与混合有效／无效记录。手工构造 `TrashItem` 的单元测试作为补充。

入口：[回收站后端](../../../bin/files/src/features/file_operations/recycle.rs)、[现有测试](../../../bin/files/src/features/file_operations/recycle/tests.rs)。规范：[Trash §6](https://specifications.freedesktop.org/trash/1.0/)。

## FM-02 原生 FileChooser 与窗口语义（P1 / P2）

- [x] 完成原生 FileChooser 的参数、窗口语义及打开／保存能力。

参数与窗口语义的正确性问题为 P1，原生功能补齐为 P2。实现已移除 FileChooser 的过渡 GTK 分支。

要求：

- 请求模型包含 `title`、`parent_window`、`modal`、`accept_label`；协议转换、进程边界和窗口呈现全链路保留它们。
- 正确处理缺省 `modal=true`、显式 `false`、空父窗口，以及 Wayland / X11 父窗口标识。建立真实 transient / 模态关系；确认标签处理调用方的助记符语义。
- 原生实现 OpenFile、SaveFile 和 SaveFiles，包括保存位置、文件名建议、同名确认与多文件保存结果。
- 原生支持文件名与 MIME 过滤器、当前过滤器，以及调用方传入的额外 choices，并准确返回选择结果；请求参数采用可无损传递的结构化表示。
- 用原生实现替换目前因 Save、过滤器或 choices 尚未实现而转交 GTK 的分支，不以过渡回退替代功能交付。
- 只有核实平台能力、现有库及必要的 GPUI 适配仍无法满足请求时，才考虑 fallback，并记录具体限制与触发场景。尚未实现、缺少现成封装或实现工作量本身不构成回退依据。
- 保留请求 Close、取消响应、并发请求隔离和子进程回收，避免完成结果与取消竞争。

验收：既测试直接调用后端，也测试应用经 `xdg-desktop-portal` 前端调用。上述功能在原生路径完成，覆盖单选／多选、打开／保存、过滤器、choices、自定义标题与按钮、父窗口有效／为空／不可用、默认模态与非模态、取消及并发请求。窗口关系需要在目标会话上实测；确有不可实现限制而增加的 fallback 单独验收。

入口：[Portal 请求](../../../bin/portal/src/chooser.rs)、[参数转换](../../../bin/portal/src/protocol.rs)、[Files CLI](../../../bin/files/src/app/cli.rs)、[选择器](../../../bin/files/src/pages/workspace/ui/picker.rs)。规范：[FileChooser](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.FileChooser.html)、[窗口标识](https://flatpak.github.io/xdg-desktop-portal/docs/window-identifiers.html)。

## FM-03 FileManager1 与激活入口（P2）

- [x] 实现 `org.freedesktop.FileManager1` 及可配置的 D-Bus 激活服务。

要求：

- 导出标准总线名与对象路径，支持 `ShowFolders`、`ShowItems`、`ShowItemProperties`。
- 接收 URI 数组，正确处理目录、根目录、百分号编码及非 UTF-8 本地名称。`ShowItems` 对目录也显示父目录并选中它；`ShowItemProperties` 实际显示目标属性。
- 按父目录组合选中请求，明确复用已有窗口／页签的策略，在目录加载后应用选择。保留并处理调用方的 `StartupId` / 适用的窗口激活信息。
- Files 应用进程承接请求，使用现有 GPUI 窗口与选择状态；正常启动、D-Bus 激活和已运行实例共享命令入口，选择器模式与浏览实例的职责明确。
- 通过已有 Nix 打包与桌面集成配置安装、启用和撤销服务。分别展示 MIME 目录关联、FileManager1 和 Portal 的状态，报告已有 provider / 总线名占用。
- 首期只承接已支持的位置；不支持的远程 URI 有明确结果，位置模型为后续网络能力保留边界。

验收：独立会话总线下测试服务未启动时激活、已运行实例分派、同目录多选、跨目录请求、目录本身被 reveal、根目录、属性展示、激活信息及已有 provider；再验证目标桌面中的“在文件夹中显示”。

入口：[Files 启动](../../../bin/files/src/app/startup.rs)、[CLI](../../../bin/files/src/app/cli.rs)、[工作区](../../../bin/files/src/pages/workspace/ui.rs)、[Nix 集成](../../../packaging/nix/apps/files/)。规范：[File Manager D-Bus Interface](https://wiki.freedesktop.org/www/Specifications/file-manager-interface/)。

## FM-04 桌面互通记录（非阻塞）

- [ ] 持续记录 Files 与 Nautilus、Dolphin 的实际互通结果，Strata 作为补充参考。

覆盖范围：

- 双向文件复制／剪切／粘贴、部分移动成功后清理、剪贴板被后来操作替换、URI 文件拖放；包括空格、换行、非 UTF-8 名称和多项选择。
- 其它 FM 写入的回收站记录由 Files 枚举并还原，Files 写入的记录也可被其它 FM 读取。
- FileManager1 激活与窗口选择、Portal 前端路由与原生选择器、桌面入口和运行中窗口标识；确有必要的特殊 fallback 单独记录。
- 首要环境 NixOS / Niri / Wayland；GTK、Qt、XWayland 和原生 X11 分别记录适用项与限制，不把一种会话的结果扩大到所有环境。
- 区分“源码支持”“协议样例通过”“直接后端测试通过”“经前端或实际 FM 验收通过”；未验收项目明确标记。

使用私有 D-Bus、临时 XDG 目录和专用文件／卷。现有剪贴板协议样例测试已经通过。用户已报告跨管理器复制实测通过；对端应用、方向及文件样例尚未细分记录，剪切与其它互通场景继续独立记录。

## 本轮实现与验证记录

- FM-01：`trash` 5.2.9 使用 [GanDesktop/trash-rs 的已审阅分支](https://github.com/GanDesktop/trash-rs/tree/fix/freedesktop-trashinfo-reviewed)，修复 freedesktop 记录与路径解析；不再内置 vendor 源码。
  实际 `info/*.trashinfo` / `files/*` 工作流覆盖重复键、空行、扩展行、相对路径、
  等号与非 UTF-8 名称、非法父路径、混合无效记录、还原及同名不覆盖。
- FM-02：`src/service/desktop/protocol` 提供轻量共享请求，stdin JSON 跨进程保留原生字节；
  三种模式均走原生界面，支持 MIME／文件名过滤、choices、标题、按钮及助记符。
  SaveFile 确认覆盖，SaveFiles 分配唯一名称且不预先创建文件。
  GPUI 平台补丁集中在 GanDesktop 的 `gpui-fast` / `gpui-fast-linux` 分支 的窗口 API、Wayland import 和 X11 提示。
  协议缺失、失效或跨 Wayland/X11 的父窗口关系会报告具体限制，原生窗口继续显示。
- FM-03：`service/desktop/file_manager` 处理标准接口，工作区复用已有目录页签、延迟应用组选择；
  属性使用已有详情组件打开原生窗口。Files 自身实例名与标准 provider 名分开，选择器不注册它们。
  `programs.gds-files.fileManager1.enable` 显式安装 D-Bus 激活入口，关闭后撤销。
- 自动验证：本轮 Files 209 项测试通过；此前公共库 142 项、Portal 6 项测试通过；
  `cargo check --locked --workspace --all-targets --features gui-tests` 通过；Nix 模块语法检查通过。
  私有总线覆盖全部 FileManager1 方法、URI 分组、启动标识、已有实例及其他 provider；
  Portal 覆盖结构化参数、原生模式结果、错误、Close、并发隔离和关停回收。
- 未实测：真实 Niri / X11 父窗口与模态效果、窗口激活策略、经 `xdg-desktop-portal` 前端调用、
  Nix 部署后的冷启动激活，以及 Nautilus / Dolphin 的细分互通场景。开发版 Files 与 Portal 已安装，Portal 已重启，新版 Files 已打开并取得运行时 FileManager1 总线名；本机的 FileManager1 冷启动激活入口未切换。
  这些项目按 FM-04 报告结果，由开发者决定后续工作，不自动追加功能。

用户反馈（2026-11-01）：跨管理器复制实测通过。该结果不推定剪切、回收站、Portal 或冷启动激活也已通过。

## 保存界面与统一位置导航（2026-11-01）

- 文件名输入移到底栏确认按钮左侧；覆盖确认使用独立背景与单行文件名。
- Home、最近、收藏、网络、回收站、卷列表及卷内目录使用同一套每页签导航。
  侧栏普通点击在当前页签打开，Ctrl/Cmd+单击新建页签；前进／后退恢复相应视图。
- 删除独立 Home 状态、固定卷页容器、固定来源类型和对应旧测试；导航模型移动到位置实体。
  当前回归包含工作区“目录→最近→收藏→卷→后退／前进”的实际交互，以及过期读取、失败保留与重挂载身份。
- 209 项 Files 测试通过，全部工作区目标检查通过。桌面互通仍按非阻塞记录处理。

## 首版之后的压缩包支持

- [ ] 统一规划内置压缩包浏览、解压、创建与修改，尚未排期。

首版压缩包由外部压缩软件处理，目标为 7-Zip，通过系统默认打开方式或“打开方式…”接入；内置能力不作为首版发布条件。后续范围、后端候选、格式限制与工作量见 [压缩包支持](archive-support.md)。

## SFTP / SMB 网络位置（2026-11-01）

- [x] 原生 Rust 协议后端、连接与主机确认、URI 导航、远程列表、最近与收藏。
- [x] 递归上传下载、复制粘贴、新建／重命名／永久删除，进度、取消及不覆盖提交。
- [x] 协议分步表单、添加后留在网络页、保存位置与访问历史分离、目录加载提示与取消。
- [ ] 远程移动／剪切、冲突改名界面与拖放。
- [ ] SMB 共享枚举的 Samba 兼容、Windows / NAS 域和只读／加密策略验收。
- [ ] 远程 CLI / FileManager1 / Portal 交付、SSH 配置、凭据库、发现、预览与编辑回传按需规划。

实现位置、固定版本、真实服务验证及已知限制见 [网络功能](network.md)。原先的全量工时估算保留为历史记录，不作为当前剩余量。

本轮网络验证：公共库 146 项、Files 221 项测试通过，`cargo check --locked --workspace --all-targets --features gui-tests` 与原生构建通过。真实 OpenSSH / Samba 访客共享的协议工作流通过；独立 X11 窗口确认连接、主机密钥、新建／重命名和远程到本地复制粘贴。Samba 共享枚举失败单独列为已知限制。

页面反馈调整后，Files 225 项测试通过，覆盖添加位置不跳转、不产生访问历史，以及取消后拒绝过期结果和新标签退出加载。独立 X11 窗口使用 delta 的真实 SFTP 验证添加、主机确认、同标签打开、加载目标与取消、中文子目录浏览；返回网络页只保留显式保存的位置。
