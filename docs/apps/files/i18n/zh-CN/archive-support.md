# GDS Files 压缩包支持

GDS Files 当前将压缩包交由外部压缩软件（目标为 7-Zip）处理。内置压缩包浏览、解压、创建和修改留待后续版本统一规划。

## 外部压缩软件打开方式

ZIP、7z、tar、tar.gz 等文件沿用普通文档的打开流程：双击或 Enter 通过系统默认应用打开，右键“打开方式…”可选择外部压缩软件。GDS Files 保留类型识别与图标，工具栏和主要菜单不增加包内浏览、压缩或解压操作。

7-Zip 的图形桌面入口和 MIME 关联由桌面环境配置，GDS Files 沿用现有 OpenURI Portal 打开通道。官方 Linux 发行物提供命令行工具 `7zz`，本身不包含图形归档窗口；在 NixOS 上部署时需区分命令行工具与图形压缩软件桌面入口（[7-Zip 官方下载说明](https://www.7-zip.org/download.html)）。

相关入口：[`pages/workspace/ui.rs`](../../../../../bin/files/src/pages/workspace/ui.rs)、[`service/desktop.rs`](../../../../../src/service/desktop.rs)、[`service/desktop/portal.rs`](../../../../../src/service/desktop/portal.rs)。

## 后续内置功能范围

第一阶段规划支持 ZIP、7z、tar、tar.gz 的只读浏览、进入包内子目录、全部或选中项目解压，以及按需打开包内文件。密码输入与错误提示与后台读取流程统一设计。包内增删改、拖入文件后重打包和修改后写回另行安排。

关键设计边界：

1. **位置与条目模型。** 保存压缩包本地位置、包内路径和稳定的条目标识，接入面包屑、标签页、前进后退、排序、选择及列表／网格。包内条目不能冒充独立的本地 `PathBuf` 文件路径；重复名称等情况通过条目标识区分。
2. **统一后台接口。** 提供格式识别、目录索引、读取条目和解压操作，格式适配集中在后台服务层。加载、进度、取消、失败和过期读取结果复用 GDS Files 的任务与视图状态规则。
3. **外部应用与文件操作。** 打开包内文件时按需生成临时文件并管理其生命周期。复制、粘贴和拖出时转换条目来源；系统文件剪贴板与外部拖放需要真实可访问的文件路径。
4. **解压语义。** 保持重名不覆盖的约定，处理取消和部分失败；校验中文及非 UTF-8 名称、符号链接和包内路径边界，防止越界写入目标目录之外。

相关入口：[`entities/location/model/address.rs`](../../../../../bin/files/src/entities/location/model/address.rs)、[`entities/file/api/directory.rs`](../../../../../bin/files/src/entities/file/api/directory.rs)、[`entities/file/api/directory/content.rs`](../../../../../bin/files/src/entities/file/api/directory/content.rs)、[`features/file_operations/api/transfer.rs`](../../../../../bin/files/src/features/file_operations/api/transfer.rs)、[`shared/lib/clipboard.rs`](../../../../../bin/files/src/shared/lib/clipboard.rs)。

## 后端候选与格式差异

| 格式 | Rust 库候选 | 接入时需验证项 |
| --- | --- | --- |
| ZIP | [`zip`](https://docs.rs/zip/latest/zip/read/struct.ZipArchive.html) | 目录索引、文件名编码、密码及不同压缩方法；通用整包解压 API 默认可覆盖已有文件，需适配 GDS Files 的不覆盖策略。 |
| 7z | [`sevenz-rust2`](https://docs.rs/sevenz-rust2/latest/sevenz_rust2/) | 固实压缩、密码、加密文件名和压缩方法兼容性。 |
| tar | [`tar`](https://docs.rs/tar/latest/tar/struct.Archive.html) | 顺序条目读取、目录索引、链接和权限；解压到已有目录需遵循应用冲突策略。 |
| tar.gz | `tar` 与 [`flate2`](https://docs.rs/flate2/latest/flate2/) | gzip 压缩流包裹 tar，需处理顺序读取、重复访问开销与缓存占用。 |

另一个统一后端候选是 [libarchive](https://github.com/libarchive/libarchive/wiki/LibarchiveFormats)（例如通过 [`compress-tools`](https://docs.rs/compress-tools/latest/compress_tools/)）。但 libarchive 3.8.11 仍拒绝加密 7z 内容或头部（[`archive_read_support_format_7zip.c`](https://github.com/libarchive/libarchive/blob/v3.8.11/libarchive/archive_read_support_format_7zip.c)），选型时需单独解决加密 7z 支持。

由于 `tar` 按条目顺序读取，而固实压缩的 `7z` 读取靠后的文件可能需先解码同一压缩块前面的数据（[`tar::Archive`](https://docs.rs/tar/latest/tar/struct.Archive.html)、[`sevenz_rust2::ArchiveReader`](https://docs.rs/sevenz-rust2/latest/sevenz_rust2/struct.ArchiveReader.html)），界面应采用后台索引与按需解压并管理缓存，不能假定所有包内条目都能零开销随机访问。
