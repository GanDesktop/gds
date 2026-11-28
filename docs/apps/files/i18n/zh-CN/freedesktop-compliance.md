# GDS Files Linux 开放桌面规范（FreeDesktop / XDG）合规性审查报告

[English](../../freedesktop-compliance.md)

- **审查范围**：`bin/files/`、`src/service/`（`desktop` / `file_inspection` / `file_tags` / `remote` / `storage` / `window_id`）、`bin/portal/`、`assets/files/`、`packaging/`
- **审查与整改基线**：基于 `9c30e63` 审计，全部 15 项已有规范缺陷（`D-01` ~ `D-15`）已于 2026-11-28 在 `main` 分支完成修复并验证
- **归类原则**：本报告将发现严格拆分为两大类，**优先推进通用开放桌面标准**，将特定桌面环境或工具链的**“方言”单独归类**按需选做：
  1. **通用开放桌面规范（主线优先）**：基于 FreeDesktop / XDG / Wayland / UDisks2 / IETF 等跨桌面正式规范（或跨应用通用草案），不绑定单一桌面环境。共 **15 项已有实现缺陷（`D-01` ~ `D-15`，已全部清零）**、**5 项尚未接入的通用能力（`G-01` ~ `G-05`）**。
  2. **桌面环境 / 工具链“方言”（单独归类）**：源于 GTK/GNOME/GVfs、KDE/KIO/Dolphin、XFCE 等特定生态的私有格式、非正式惯例或彼此分歧的扩展。共 **5 项（`DE-01` ~ `DE-05`）**。

整体结论：GDS Files 已原生实现大量 Linux 开放桌面核心规范（`org.freedesktop.Application`、`org.freedesktop.FileManager1`、`org.freedesktop.impl.portal.FileChooser`、`shared-mime-info` / `mime.cache` v1.2、FreeDesktop Trash 1.0、Thumbnail Cache & 外部 `.thumbnailer`、`recently-used.xbel`、`org.freedesktop.secrets`、`user.mime_type` / `user.xdg.tags` / `comment` / `origin.url`、`xdg-user-dirs`、`XDG_TEMPLATES_DIR` 等）。通用规范 P1–P3 缺陷均已整改完毕，后续可按需推进通用新能力 `G-01` ~ `G-05`，方言项保持独立 Backlog 选做。

---

## 一、总览矩阵

### 1.1 通用开放桌面规范：已有实现中的偏差与缺陷（`D-01` ~ `D-15`，已全部关闭）

| 编号 | 优先级 | 状态 | 涉及通用规范 | 整改摘要 | 核心位置 |
| --- | --- | --- | --- | --- | --- |
| D-01 | **P1** | **已修复** | Desktop Bookmark 1.0 (`recently-used.xbel`) | 修复自闭合 `<bookmark .../>` 与不完整子树更新，保留根节点 `xmlns:*` 与非书签节点，`clear_at` 保留 `<bookmark:private/>`/非 `file://` 条目，并引入跨进程 `flock` | [recent_files.rs](../../../../../src/service/desktop/recent_files.rs) |
| D-02 | **P1** | **已修复*** | XDG Portal (`OpenURI` / `Inhibit`) & Wayland | 以 `O_RDWR`（只读回退 `O_RDONLY`）打开文档并透传 `.writeable(...)`，同时在 `OpenURI`、`Inhibit` 与 Secret Service 接入 `ParentWindow` 与 `ActivationToken`（*UI 侧句柄/令牌导出待上游 `zgui` 补齐） | [portal.rs](../../../../../src/service/desktop/portal.rs)、[client.rs](../../../../../src/service/desktop/client.rs)、[window_id.rs](../../../../../src/service/window_id.rs)、[session.rs](../../../../../src/service/desktop/session.rs) |
| D-03 | **P1** | **已修复** | Shared MIME-Info / 文件打开安全 | 以文件头嗅探 + `shared-mime-info`（`globs2`/`aliases`/`subclasses`）替代仅媒体豁免，FAT/NTFS/SMB 卷 `0755` 文档可直接打开，二进制/脚本/伪装文件继续拦截 | [executable.rs](../../../../../src/service/desktop/executable.rs)、[client.rs](../../../../../src/service/desktop/client.rs) |
| D-04 | **P1** | **已修复** | Common Extended Attributes (`user.mime_type`) | 新增零堆分配栈缓冲 `lookup_mime_type`（`XattrLookup`），在基础目录浏览、监听刷新与 `LinkTarget::from_path_base` 中生效，并对 `EOPNOTSUPP` 目录级短路 | [file_tags.rs](../../../../../src/service/file_tags.rs)、[content.rs](../../../../../bin/files/src/entities/file/api/directory/content.rs)、[directory.rs](../../../../../bin/files/src/entities/file/api/directory.rs) |
| D-05 | **P1** | **已修复** | Secret Service API 0.2 (`org.freedesktop.secrets`) | 非沙盒 `lookup()` 搜索前先解锁锁定状态的默认集合，去除对 GNOME Keyring 宽松行为的隐含依赖（修复 KWallet / KeePassXC），同时保持 `has_saved()` 无弹窗 | [keyring.rs](../../../../../src/service/remote/keyring.rs) |
| D-06 | **P2** | **已修复** | XDG Base Directory Spec | MIME 数据库与外部 thumbnailer 目录解析均按规范忽略 `XDG_DATA_HOME` / `XDG_DATA_DIRS` 中的相对路径并回退标准默认目录 | [mime_db.rs](../../../../../bin/files/src/entities/file/api/directory/content/mime_db.rs)、[external.rs](../../../../../src/service/file_inspection/thumbnail/external.rs) |
| D-07 | **P2** | **已修复** | Shared MIME-Info (`mime.cache` v1.2) | 0 字节无后缀文件归类 `application/x-zerosize`；解析 `NamespaceList`（offset 28）/ `IconsList`（offset 32）与文本 `XMLnamespaces`/`icons` 实现 XML 根元素细化；跨层生效 `__NOGLOBS__` 且不误匹配字面文件名 | [content.rs](../../../../../bin/files/src/entities/file/api/directory/content.rs)、[cache.rs](../../../../../bin/files/src/entities/file/api/directory/content/mime_db/cache.rs)、[text.rs](../../../../../bin/files/src/entities/file/api/directory/content/mime_db/text.rs) |
| D-08 | **P2** | **已修复** | FreeDesktop Trash 1.0 | 补齐 `directorysizes` 列表读取与目录恢复/彻底删除原子清理，并为活跃回收站 `info/` 目录及 `directorysizes` 挂载防抖实时监听 | [recycle.rs](../../../../../bin/files/src/features/file_operations/recycle.rs)、[directory_sizes.rs](../../../../../bin/files/src/features/file_operations/recycle/directory_sizes.rs)、[watch.rs](../../../../../bin/files/src/features/file_operations/recycle/watch.rs)、[tabs.rs](../../../../../bin/files/src/pages/workspace/navigation/tabs.rs) |
| D-09 | **P2** | **已修复** | Thumbnail Managing Standard | 不再将 `NoCover`/缺外部 thumbnailer 写入永久 `fail/`；软链缓存根目录规范化自保护；目录删除/回收站/重命名/移动/撤销重做有界级联失效子项缩略图；新增 `.sh_thumbnails/` 只读仓库及失败回退 `large`/`normal` | [cache.rs](../../../../../src/service/file_inspection/thumbnail/cache.rs)、[thumbnail.rs](../../../../../src/service/file_inspection/thumbnail.rs)、[api.rs](../../../../../bin/files/src/features/file_operations/api.rs)、[transfer.rs](../../../../../bin/files/src/features/file_operations/transfer.rs) |
| D-10 | **P2** | **已移除** | MIME Apps 1.0 & Desktop Entry 1.5 | 删除未接入 UI 的 `src/service/desktop/mime_apps.rs`，文件打开与应用选择统一交由桌面 `OpenURI` / `AppChooser` Portal | [desktop.rs](../../../../../src/service/desktop.rs) |
| D-11 | **P2** | **已修复** | RFC 8089 / `text/uri-list` | 统一本地 `file://` 解析逻辑以接受本机主机名 `file://<hostname>/...`，覆盖 `recently-used.xbel`、剪贴板/拖拽、地址栏 `FileScheme` 与 `FileManager1` | [recent_files.rs](../../../../../src/service/desktop/recent_files.rs)、[protocol.rs](../../../../../bin/files/src/shared/lib/clipboard/protocol.rs)、[scheme.rs](../../../../../bin/files/src/entities/location/scheme.rs) |
| D-12 | **P2** | **已修复** | `org.freedesktop.FileManager1` | `ShowFolders` 改为支持有效网络 URI（`smb://`/`sftp://`）与 GDS 虚拟位置 URI；`ShowItemProperties` 仅打开独立属性窗并传递激活令牌，不再切 tab、改写 Details 或拉起主窗口 | [file_manager.rs](../../../../../src/service/desktop/file_manager.rs)、[browser.rs](../../../../../bin/files/src/pages/workspace/shell/browser.rs)、[windows.rs](../../../../../bin/files/src/app/windows.rs)、[details.rs](../../../../../bin/files/src/pages/workspace/panes/inspection/details.rs) |
| D-13 | **P3** | **已修复** | Desktop Entry 1.5 (`org.freedesktop.Application`) | 在 `/io/github/GanDesktop/GdsFiles` 实现 `org.freedesktop.Application`（`Activate`、`Open`、`ActivateAction("new-window")`），并在 Linux/RPM/DEB/Arch/Nix 打包中提供 `DBusActivatable=true` 与 `.service` | [file_manager.rs](../../../../../src/service/desktop/file_manager.rs)、[windows.rs](../../../../../bin/files/src/app/windows.rs)、[install.py](../../../../../packaging/linux/install.py)、[package.nix](../../../../../packaging/nix/apps/files/package.nix) |
| D-14 | **P3** | **已修复**** | AppStream MetaInfo 1.0 | 补齐 `<categories>`、中英文 `<keywords>` 与 `<provides>`（`<binary>`、`<dbus type="user">`），修正 `<developer id="io.github.gandesktop">`，通过 `appstreamcli validate --no-net`（**`<screenshots>` 待托管截图就绪后补入） | [io.github.GanDesktop.GdsFiles.metainfo.xml](../../../../../assets/files/io.github.GanDesktop.GdsFiles.metainfo.xml) |
| D-15 | **P3** | **已修复** | XDG User Directories (`user-dirs.dirs`) | 实现纯函数 `user-dirs.dirs` 解析、槽位追踪原地迁移（保留用户排序与移除记录）以及 `user-dirs.dirs` 变更实时监听同步 | [user_dirs.rs](../../../../../bin/files/src/entities/location/api/places/user_dirs.rs)、[places.rs](../../../../../bin/files/src/entities/location/api/places.rs)、[accessor.rs](../../../../../bin/files/src/entities/location/api/places/accessor.rs)、[entities/location/places.rs](../../../../../bin/files/src/entities/location/places.rs)、[context.rs](../../../../../bin/files/src/pages/workspace/state/context.rs) |

### 1.2 通用开放桌面规范：尚未接入的标准能力（`G-01` ~ `G-05`）

| 编号 | 涉及通用规范 | 缺失内容摘要 | 相关模块 |
| --- | --- | --- | --- |
| G-01 | Desktop Entry Spec 1.5 (`.desktop`) | 目录内 `.desktop` 启动器（`Type=Application` / `Type=Link`）解析、本地化名称/图标展示与基于 `+x`/用户确认的安全运行（`.directory` 与 DE 信任标记已单列至 `DE-02`） | [client.rs](../../../../../src/service/desktop/client.rs)、[directory.rs](../../../../../bin/files/src/entities/file/api/directory.rs) |
| G-02 | Icon Theme & Icon Naming Spec | 仅使用内置固定图标枚举，未接入 `$XDG_DATA_HOME/icons` / `$XDG_DATA_DIRS/icons` 图标主题与 `hicolor` 回退链（配合 MIME `icons` 及 UDisks2 `HintIconName`） | [icon.rs](../../../../../bin/files/src/entities/file/ui/icon.rs) |
| G-03 | XDG `FileTransfer` Portal & Wayland DnD `ASK` | 剪贴板/拖拽未支持 `application/vnd.portal.filetransfer`（Flatpak 沙盒互通）及 Wayland `DragActions::ASK`（复制/移动/链接菜单） | [protocol.rs](../../../../../bin/files/src/shared/lib/clipboard/protocol.rs)、[files.rs](../../../../../bin/files/src/pages/workspace/panes/drag/files.rs) |
| G-04 | UDisks2 (`Encrypted` / `HintIconName`) | 未实现 `org.freedesktop.UDisks2.Encrypted.Unlock`/`Lock` 加密卷解锁，未读取标准 `HintIconName`（`x-gvfs-*` 已单列至 `DE-03`） | [udisks.rs](../../../../../bin/files/src/entities/location/api/volumes/udisks.rs) |
| G-05 | Common Extended Attributes | 尚未在属性/详情面板展示 `user.xdg.referrer.url`、`user.xdg.origin.email.*`、`user.charset` | [file_tags.rs](../../../../../src/service/file_tags.rs) |

### 1.3 桌面环境 / 工具链“方言”清单（`DE-01` ~ `DE-05`，单独归类 Backlog）

| 编号 | 所属生态方言 | 方言内容摘要 | 为何归类为方言 | 相关模块 |
| --- | --- | --- | --- | --- |
| DE-01 | GTK / GNOME & KDE / KIO | 共享书签文件：`~/.config/gtk-3.0/bookmarks` 与 `$XDG_DATA_HOME/user-places.xbel` | 并非 XDG 正式标准，GTK 系与 KDE 系各自维护互不兼容的两套格式 | [places.rs](../../../../../bin/files/src/entities/location/api/places.rs) |
| DE-02 | KDE / Dolphin、GNOME / GVfs & XFCE | 目录内 `.directory` 文件夹元数据、GVfs `metadata::custom-icon` / `metadata::trusted`、XFCE `metadata::xfce-exe-checksum` | Desktop Entry 的 `Type=Directory` 本意用于应用菜单；目录内 `.directory` 是 KDE 方言，而 GNOME 用 GVfs 元数据、XFCE 用校验和 | [directory.rs](../../../../../bin/files/src/entities/file/api/directory.rs)、[client.rs](../../../../../src/service/desktop/client.rs) |
| DE-03 | GNOME / GVfs | `/etc/fstab` 与 `/proc/self/mountinfo` 中的 `x-gvfs-hide` / `x-gvfs-show` / `x-gvfs-name` / `x-gvfs-icon` | 源于 GLib/GVfs 的挂载选项命名空间，非通用内核/UDisks2 协议标准 | [network.rs](../../../../../bin/files/src/entities/location/api/volumes/network.rs) |
| DE-04 | GTK / Nautilus & KDE / Dolphin | `XDG_TEMPLATES_DIR` 内部组织惯例：Nautilus/Thunar 嵌套子目录子菜单 vs KDE `.desktop` 模板描述 | `xdg-user-dirs` 仅定义了模板目录路径，未规范内部结构；GTK 系与 KDE 系做法不同 | [templates.rs](../../../../../bin/files/src/features/file_operations/api/templates.rs) |
| DE-05 | GNOME / GVfs & KDE / KIO | 虚拟目录与回收站子路径 URI 方案：GVfs `trash:///name` / `recent:///` vs KIO `trash:/0-name` / `recentlyused:/` / `tags:/` | Trash 1.0 仅规范磁盘布局，未定义 URI 语法；GVfs 与 KIO 在多卷子路径及虚拟目录 scheme 上不统一 | [scheme.rs](../../../../../bin/files/src/entities/location/scheme.rs)、[file_manager.rs](../../../../../src/service/desktop/file_manager.rs) |

---

## 二、已闭环缺陷整改详情（`D-01` ~ `D-15`）

### Phase 1（P1 高危缺陷）

- **D-01（`recently-used.xbel`）**：在 [recent_files.rs](../../../../../src/service/desktop/recent_files.rs) 引入 `DocumentEnvelope` 保留根节点原有属性、第三方 `xmlns:*` 及非 `<bookmark>` 子节点；完整支持自闭合 `<bookmark .../>`、`<info/>`、`<metadata/>`、`<bookmark:applications/>` 的原地展开与补写；`clear_at` 改为保留 `<bookmark:private/>` 与非本地条目；写入路径增加 `<xbel>.lock` 跨进程 `flock`（`O_NOFOLLOW | O_CLOEXEC`，`0o600`）。
- **D-02（`OpenURI` / `Inhibit` / Secret Service 上下文）**：[client.rs](../../../../../src/service/desktop/client.rs) 优先以 `O_RDWR` 打开文档并在只读/受限/目录时回退 `O_RDONLY`，通过 [portal.rs](../../../../../src/service/desktop/portal.rs) 向 `OpenFileRequest::writeable(...)` 透传可写状态；新增跨 feature 共享的 [window_id.rs](../../../../../src/service/window_id.rs)（`ParentWindow`）与 `OpenContext`，贯通 `Desktop::open_file_with_context`、`InhibitGuard::delayed_with_window`（[session.rs](../../../../../src/service/desktop/session.rs)）和 `keyring::{lookup,store,delete}_with_window`（[keyring.rs](../../../../../src/service/remote/keyring.rs)）。
  - **上游挂起项**：从活动 UI 窗口导出实时 `zxdg_exporter_v2` 父窗口句柄及申请出站 `xdg_activation_v1` 令牌，需待上游 `GanDesktop/zgui` 在 `WindowHandle` 补齐对应能力后在 UI 调用点接入。
- **D-03（可执行 `+x` 打开门禁）**：新增 [executable.rs](../../../../../src/service/desktop/executable.rs)（`ExecutableClassifier`），先做文件头检查（`#!`、`\x7fELF`、`[Desktop Entry]`、`infer` 识别的可执行二进制并排除 `.pem`/`.der` 证书）和 Linux `user.mime_type` 判定，再结合 XDG `shared-mime-info`（`globs2`/`globs`、`aliases`、`subclasses`）放行 FAT/NTFS/SMB 卷带 `0755` 的普通文档，继续安全拦截可执行文件与伪装脚本。
- **D-04（常规目录浏览 `user.mime_type`）**：在 [file_tags.rs](../../../../../src/service/file_tags.rs) 新增零堆分配栈缓冲的 `lookup_mime_type`（`XattrLookup`），并在 [content.rs](../../../../../bin/files/src/entities/file/api/directory/content.rs) 与 [directory.rs](../../../../../bin/files/src/entities/file/api/directory.rs) 的 `read_directory_base`、监听增量更新及 `LinkTarget::from_path_base` 路径统一优先读取 `user.mime_type`，遇到 `EOPNOTSUPP`/`ENOSYS` 自动按目录短路，不破坏免 8 KiB 头部嗅探的性能约束。
- **D-05（Secret Service 锁定集合解锁）**：在 [keyring.rs](../../../../../src/service/remote/keyring.rs) 非沙盒 `lookup()` 中于 `search_items` 前检查 `collection.is_locked()` 并先触发集合解锁（取消返回 `Error::Dismissed`），与 `store()` / `delete()` 及沙盒分支对齐，消除对 GNOME Keyring 宽松行为的隐含依赖，同时保持 `has_saved()` 无弹窗被动探测。

### Phase 2（P2 规范一致性）

- **D-06（XDG Base Dir 相对路径）**：在 [mime_db.rs](../../../../../bin/files/src/entities/file/api/directory/content/mime_db.rs) 与 [external.rs](../../../../../src/service/file_inspection/thumbnail/external.rs) 抽取纯函数路径解析，按规范忽略相对 `XDG_DATA_HOME` / `XDG_DATA_DIRS` 并回退默认目录。
- **D-07（Shared MIME-Info & `mime.cache` v1.2）**：将无后缀 0 字节文件归类为 `application/x-zerosize`（在 [mime-types.toml](../../../../../bin/files/src/entities/file/api/directory/content/assets/mime-types.toml) 映射为可预览文本类）；补齐二进制 `mime.cache` v1.2 `NamespaceList`/`IconsList` 与文本 `XMLnamespaces`/`icons` 解析，支持在 8 KiB 头部上做 XML 根元素/命名空间细化；跨目录层实现 `0:<mime>:__NOGLOBS__` 屏蔽且不误匹配字面文件名 `__NOGLOBS__`。
- **D-08（Trash 1.0 `directorysizes` 与实时监听）**：新增 [directory_sizes.rs](../../../../../bin/files/src/features/file_operations/recycle/directory_sizes.rs) 在列表时校验 `.trashinfo` mtime 读取目录体积、在目录恢复/彻底删除后原子清理失效条目；新增 [watch.rs](../../../../../bin/files/src/features/file_operations/recycle/watch.rs)（`TrashWatch`）并在 [tabs.rs](../../../../../bin/files/src/pages/workspace/navigation/tabs.rs) 挂载防抖监听，覆盖主回收站与活跃挂载卷 `info/` 目录及 `directorysizes`。
- **D-09（Thumbnail Managing Standard）**：`fail/` 不再持久化 `Error::NoCover` / 缺外部 thumbnailer 并忽略旧 `no-cover` 标记；`Cache::contains` 通过规范化路径抵御软链 `~/.cache` 漏判；新增有界不跟随目录软链的 `ThumbnailInvalidation` 接入删除/回收站/重命名/移动/撤销重做；补齐 `.sh_thumbnails/` 只读共享仓库查找以及用户缓存生成失败时的 `large`/`normal` 回退。
- **D-10（`mime_apps.rs`）**：移除未接入 UI 的 `src/service/desktop/mime_apps.rs` 及其在 [desktop.rs](../../../../../src/service/desktop.rs) 中的导出，打开与应用选择统一交由系统 Portal。
- **D-11（RFC 8089 本机主机名 URI）**：在 [recent_files.rs](../../../../../src/service/desktop/recent_files.rs) 统一本机主机名匹配并在剪贴板/拖拽（[protocol.rs](../../../../../bin/files/src/shared/lib/clipboard/protocol.rs)）、`FileScheme`（[scheme.rs](../../../../../bin/files/src/entities/location/scheme.rs)）和 `FileManager1`（[file_manager.rs](../../../../../src/service/desktop/file_manager.rs)）中一致接受 `file://<local-hostname>/...`。
- **D-12（`org.freedesktop.FileManager1`）**：`ShowFolders` 改为支持有效网络 URI（`smb://`/`sftp://`）与 GDS 内置虚拟位置 URI；`ShowItemProperties` 仅打开独立 `Properties` 窗口并传递激活令牌，不再切主窗口 tab、聚焦主窗口、持久化改写 Details 面板或拉起多余主窗口。

### Phase 3（P3 激活、分发元数据与动态用户目录）

- **D-13（`org.freedesktop.Application` 与 `DBusActivatable=true`）**：在 `/io/github/GanDesktop/GdsFiles` 实现并注册标准 `org.freedesktop.Application`（`Activate`、`Open`、`ActivateAction("new-window")`），在 [windows.rs](../../../../../bin/files/src/app/windows.rs) 与 [browser.rs](../../../../../bin/files/src/pages/workspace/shell/browser.rs) 接入 `Request::Activate { new_window }`，并在 [install.py](../../../../../packaging/linux/install.py)、[gds.spec](../../../../../packaging/fedora/gds.spec)、[gds-files.install](../../../../../packaging/debian/gds-files.install)、[package.nix](../../../../../packaging/nix/apps/files/package.nix) 与 [module.nix](../../../../../packaging/nix/apps/files/module.nix) 同步启用 `DBusActivatable=true` 与 `io.github.GanDesktop.GdsFiles.service`。
- **D-14（AppStream MetaInfo）**：在 [io.github.GanDesktop.GdsFiles.metainfo.xml](../../../../../assets/files/io.github.GanDesktop.GdsFiles.metainfo.xml) 补齐 `<categories>`、中英文 `<keywords>`、`<provides>`（`<binary>gds-files</binary>` 与两个 `<dbus type="user">` 声明）并将 `<developer id>` 规范为 `io.github.gandesktop`，通过 `appstreamcli validate --no-net` 零错误零警告校验；`<screenshots>` 待正式托管截图地址就绪后补充。
- **D-15（XDG User Dirs 动态同步）**：新增 [user_dirs.rs](../../../../../bin/files/src/entities/location/api/places/user_dirs.rs) 纯函数解析与槽位对齐，复用 `places_meta` 中的 `xdg_user_dirs_snapshot` 原地迁移目录路径同时保留用户自定义排序与手动移除记录；在 [entities/location/places.rs](../../../../../bin/files/src/entities/location/places.rs) 启动 `$XDG_CONFIG_HOME/user-dirs.dirs` 防抖监听，并通过 `Context::bind_places_sync`（[context.rs](../../../../../bin/files/src/pages/workspace/state/context.rs)、[browser.rs](../../../../../bin/files/src/pages/workspace/shell/browser.rs)）实时同步到所有打开窗口。

---

## 三、仓库现有外围方言兼容层（边界参考）

- **剪贴板剪切/复制方言**：[protocol.rs](../../../../../bin/files/src/shared/lib/clipboard/protocol.rs) 在标准 `text/uri-list` 外同时兼容 GNOME `x-special/gnome-copied-files` 与 KDE `application/x-kde-cutselection`。
- **隐藏文件清单 `.hidden`**：源于 GLib/Nautilus 惯例并已被主流文件管理器普遍采用，常规目录浏览已实现。
- **KDE Baloo / KIO 标签索引桥接**：在 [外部工具集成决策](../../../../decisions/external-integrations.md) 中已明确记录折衷与退出条件。

---

## 四、后续路线

| 轨道 | 包含条目 | 目标收益 |
| --- | --- | --- |
| **通用标准演进** | `G-01` ~ `G-05`、上游 `zgui` 句柄/令牌导出（`D-02` UI 侧闭环）、托管 `<screenshots>`（`D-14`） | 补齐目录内 `.desktop` 启动器、系统 Icon Theme、Flatpak `FileTransfer` Portal + Wayland DnD `ASK`、UDisks2 LUKS 解锁与 `HintIconName`、其余 `user.xdg.*` 属性及实时 Portal 窗口上下文 |
| **方言 Backlog（选做）** | `DE-01` ~ `DE-05` | 独立于通用主线，按实际目标桌面占比（GTK/GNOME vs KDE/Plasma）选择性引入，避免将核心状态耦合到单一桌面环境 |
