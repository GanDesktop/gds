# GDS Files 路线图与互操作

本文档记录 GDS Files 与 GDS Portal 当前已实现的能力边界及后续待办事项。

## 回收站解析与还原

- [x] 遵循 FreeDesktop Trash 1.0 规范的 `.trashinfo` 解析与还原语义。

当前行为：

- 重复 `Path` 与 `DeletionDate` 字段使用第一次出现的值；注释、空行和未知扩展行按规范忽略，枚举不会因此 panic。
- Home 回收站的相对路径以 `$XDG_DATA_HOME` 为基准；卷回收站分别处理 `.Trash-$uid` 和 `.Trash/$uid`，保留原始路径字节并拒绝规范不允许的相对 `..`。
- 还原操作不覆盖已有目标，部分失败时保留原记录，并在清理前检测记录变更。

入口：[`features/file_operations/recycle.rs`](../../../../../bin/files/src/features/file_operations/recycle.rs)、[`features/file_operations/recycle/tests.rs`](../../../../../bin/files/src/features/file_operations/recycle/tests.rs)。规范：[Trash 1.0 §6](https://specifications.freedesktop.org/trash/1.0/)。

## 原生 FileChooser 与 GDS Portal

- [x] 通过 GDS Portal 提供原生 `FileChooser` 参数、窗口语义及打开／保存流程。

当前行为：

- 请求模型包含 `title`、`parent_window`、`modal` 与 `accept_label`（`src/service/desktop/protocol`），通过 stdin JSON 跨进程无损保留原始 Unix 路径字节。
- 支持缺省 `modal=true`、显式 `false`、空父窗口，以及 Wayland（`xdg-foreign-v2`、`xdg-dialog`）与 X11（`WM_TRANSIENT_FOR`）父窗口标识；跨会话或父窗口不可用时报告具体限制并继续显示原生选择器。
- 原生实现 `OpenFile`、`SaveFile` 和 `SaveFiles`，支持文件名与 MIME 过滤器、当前过滤器选择、调用方 `choices`、覆盖确认（`SaveFile`）以及不预先创建文件的唯一名称分配（`SaveFiles`）。
- GDS Portal 处理请求 `Close`、用户取消、并发请求隔离与子进程回收。

入口：[`bin/portal/src/chooser.rs`](../../../../../bin/portal/src/chooser.rs)、[`bin/portal/src/protocol.rs`](../../../../../bin/portal/src/protocol.rs)、[`bin/files/src/app/cli.rs`](../../../../../bin/files/src/app/cli.rs)、[`bin/files/src/pages/workspace/ui/picker.rs`](../../../../../bin/files/src/pages/workspace/ui/picker.rs)。规范：[FileChooser Portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.FileChooser.html)、[窗口标识](https://flatpak.github.io/xdg-desktop-portal/docs/window-identifiers.html)。

## `org.freedesktop.FileManager1` 服务

- [x] 标准 `org.freedesktop.FileManager1` 接口与可配置的 D-Bus 激活服务。

当前行为：

- 在标准总线名与对象路径上导出 `ShowFolders`、`ShowItems` 和 `ShowItemProperties`（`src/service/desktop/file_manager`）。
- 接收本地 `file://` URI 数组，处理目录、根目录、百分号编码及非 UTF-8 本地名称；`ShowItems` 对目录也打开其父目录并选中目标，`ShowItemProperties` 打开原生属性窗口。
- 按父目录组合选中请求，复用已有窗口与目录页签，并传递调用方的 `StartupId` 窗口激活信息。
- `programs.gds-files.fileManager1.enable` 显式安装 D-Bus 激活入口（`gds-files --dbus-service`），与选择器模式分离，并在已有其他 provider 占用总线名时明确报告。

入口：[`bin/files/src/app/startup.rs`](../../../../../bin/files/src/app/startup.rs)、[`bin/files/src/app/cli.rs`](../../../../../bin/files/src/app/cli.rs)、[`bin/files/src/pages/workspace/ui.rs`](../../../../../bin/files/src/pages/workspace/ui.rs)、[`packaging/nix/apps/files/`](../../../../../packaging/nix/apps/files/)。规范：[File Manager D-Bus Interface](https://wiki.freedesktop.org/www/Specifications/file-manager-interface/)。

## 桌面互操作验证

- [ ] 持续验证 GDS Files、GDS Portal 与 Nautilus、Dolphin 在 Wayland、XWayland 及 X11 会话下的实际互通：
  - 双向文件复制／剪切／粘贴、部分移动成功后的剪贴板清理、剪贴板替换以及 URI 文件拖放（含空格、换行、非 UTF-8 名称和多项选择）。
  - 跨文件管理器的回收站记录枚举与还原。
  - `FileManager1` 冷启动激活与窗口选择、经 `xdg-desktop-portal` 前端路由到 GDS Portal，以及目标合成器上的模态／父子窗口表现。

## 内置压缩包支持

- [ ] 规划内置压缩包浏览、解压、创建与修改（尚未排期）。

GDS Files 当前将 `.zip`、`.7z`、`.tar`、`.tar.gz` 等压缩包交由外部压缩软件（目标为 7-Zip）通过系统默认打开方式或“打开方式…”处理。后续内置能力的范围与候选后端见 [压缩包支持](archive-support.md)。

## SFTP / SMB 网络位置

- [x] 原生 Rust SFTP/SMB 协议后端、SSH 主机密钥确认、URI 导航、远程列表、最近记录与收藏。
- [x] 递归上传下载、复制粘贴、新建文件夹、重命名、永久删除、进度反馈、取消及不覆盖发布。
- [x] 分步协议连接表单、保存位置与访问历史分离、可取消的目录加载状态。
- [ ] 远程剪切／移动、冲突改名对话框与拖放。
- [ ] SMB 根共享枚举在 Samba、Windows 与 NAS 域／签名／加密策略下的兼容性。
- [ ] 远程 CLI / `FileManager1` / GDS Portal 衔接、SSH 配置读取、凭据持久化、自动发现与远程预览。

实现位置与协议限制见 [网络功能](../../network.md)。
