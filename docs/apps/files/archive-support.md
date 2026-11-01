# Files 压缩包支持

2026-11-01 决定：Files 首版将压缩包交给外部压缩软件处理，目标为 7-Zip。内置压缩包浏览、解压、创建和修改放到首版之后统一规划，尚未排期。

## 首版打开方式

ZIP、7z、tar、tar.gz 等文件沿用普通文档的打开流程：双击或 Enter 通过系统默认应用打开，右键“打开方式…”可选择外部压缩软件。Files 保留类型识别与图标，首版工具栏和主要菜单不增加包内浏览、压缩或解压操作。

7-Zip 的图形桌面入口和 MIME 关联由桌面环境配置，Files 沿用现有 OpenURI Portal 打开通道。官方 Linux 发行物是命令行版，`7zz` 本身不提供图形归档窗口；在 NixOS 上部署时，需要区分命令行工具与实际可启动的图形压缩软件入口。[7-Zip 官方下载说明](https://www.7-zip.org/download.html)

现有入口：[文件激活](../../../bin/files/src/pages/workspace/ui.rs)、[桌面打开服务](../../../src/service/desktop.rs)、[OpenURI Portal](../../../src/service/desktop/portal.rs)。

## 后续内置功能范围

第一阶段建议支持 ZIP、7z、tar、tar.gz 的只读浏览、进入包内子目录、全部或选中项目解压，以及按需打开包内文件。密码输入与错误提示应与后台读取流程统一设计。包内增删改、拖入文件后重打包和修改后写回另行安排。

主要工作包括：

1. **位置与条目模型。** 保存压缩包本地位置、包内路径和稳定的条目标识，接入面包屑、标签页、前进后退、排序、选择及列表／网格。包内条目不能冒充独立的本地文件路径；重复名称等情况需要通过条目标识区分。
2. **统一后台接口。** 提供格式识别、目录索引、读取条目和解压操作，格式适配集中在后台。加载、进度、取消、失败和过期读取结果复用 Files 的任务与视图状态规则。
3. **外部应用与文件操作。** 打开包内文件时按需生成临时文件，并管理其生命周期。复制、粘贴和拖出需要转换条目来源；系统文件剪贴板与外部拖放需要真实可访问的文件路径。
4. **解压语义。** 保持重名不覆盖的约定，处理取消和部分失败；验证中文及非 UTF-8 名称、符号链接和包内路径边界，避免写出目标目录。

当前实现以本地 `PathBuf` 为文件操作对象。已有虚拟位置模型、文件表格、后台任务、通知与重名处理可以复用；压缩包的位置身份和条目来源需要补齐。相关入口：[位置模型](../../../bin/files/src/entities/location/model/address.rs)、[目录读取](../../../bin/files/src/entities/file/api/directory.rs)、[类型识别](../../../bin/files/src/entities/file/api/directory/content.rs)、[文件传输](../../../bin/files/src/features/file_operations/api/transfer.rs)、[文件剪贴板](../../../bin/files/src/shared/lib/clipboard.rs)。

## 后端候选与格式差异

| 格式 | Rust 库候选 | 接入时需要验证 |
| --- | --- | --- |
| ZIP | [`zip`](https://docs.rs/zip/latest/zip/read/struct.ZipArchive.html) | 目录索引、文件名编码、密码及不同压缩方法；通用整包解压 API 默认可覆盖已有文件，需要落实 Files 自身的目标策略。 |
| 7z | [`sevenz-rust2`](https://docs.rs/sevenz-rust2/latest/sevenz_rust2/) | 固实压缩、密码、加密文件名和压缩方法兼容性；该库提供密码读取与自定义解压接口。 |
| tar | [`tar`](https://docs.rs/tar/latest/tar/struct.Archive.html) | 顺序条目读取、目录索引、链接和权限；解压到已有目录默认涉及内容合并，需要明确应用策略。 |
| tar.gz | `tar` 与 [`flate2`](https://docs.rs/flate2/latest/flate2/) | gzip 压缩流包裹 tar，需处理顺序读取、重复访问开销与缓存占用。 |

可优先评估 Rust 库组合，并在上层封装为同一套压缩包接口。依赖及特性在实现时重新确认，不将这些候选直接视为已验证支持。

另一个候选是 [libarchive](https://github.com/libarchive/libarchive/wiki/LibarchiveFormats)，可统一处理多种归档与压缩格式；Rust 封装 [`compress-tools`](https://docs.rs/compress-tools/latest/compress_tools/) 提供列目录、整包解压和单文件提取。不过，核查到的 libarchive 3.8.11 实现仍拒绝加密 7z 内容或头部；选择该方案时需另行解决这一能力，不能仅按格式清单承诺密码包支持。[对应版本源码](https://github.com/libarchive/libarchive/blob/v3.8.11/libarchive/archive_read_support_format_7zip.c)

tar 的读取按条目顺序进行；固实压缩的 7z 读取后面的文件可能需要先解码同一压缩块前面的数据。界面应采用后台索引与按需解压，并管理缓存，不能将“支持列目录”等同于任意文件都能即时打开。[tar 读取规则](https://docs.rs/tar/latest/tar/struct.Archive.html)、[7z 固实压缩读取说明](https://docs.rs/sevenz-rust2/latest/sevenz_rust2/struct.ArchiveReader.html)

## 工作量粗估

以下按一名熟悉项目的开发者、包括回归验证估算，各档为累计范围，不作为首版排期承诺。

| 交付范围 | 工作量 |
| --- | --- |
| 常见未加密格式解压到当前目录或指定目录 | 2–4 人日 |
| 内置只读浏览、选取解压、打开包内文件及基础密码交互 | 约 1–2 周 |
| 包内增删改、拖入重打包、修改后写回及复杂分卷 | 约 3–5 周以上，按具体范围重估 |

后续启动时，先用普通 ZIP、中文名称 ZIP、密码 7z、固实 7z 和大 tar.gz 验证候选后端，再确定依赖和实现顺序。首版继续使用外部压缩软件，不以这些后续功能作为发布条件。
