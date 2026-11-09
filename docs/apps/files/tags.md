# Files 标签与 URI scheme

Files 通过 `user.xdg.tags` 与 Dolphin 共享文件标签，使用系统 Baloo 查询标签目录和关联文件。自建后端尚未实现。

## 配置与依赖

标签功能默认关闭，在设置 → Files 中通过后端检测后启用。设置即时生效并自动保存；后端不可用时显示具体错误，保留已保存的启用偏好。

依赖 Baloo 的 `tags:` KIO worker，以及以下命令：

- `balooctl6` / `balooctl`：检查索引状态。
- `kioclient6` / `kioclient` / `kioclient5`：查询标签，通常由 `kde-cli-tools` 提供。

命令默认从 PATH 查找，也可指定路径。检测包括命令版本、`balooctl status` 和 `kioclient ls tags:/`；命令失败、禁用索引或缺少 worker 时不能新启用功能。

```toml
[tags]
enabled = false
backend = "baloo"
# control = "/custom/bin/balooctl6"
# kio_client = "/custom/bin/kioclient"
```

GDS 不启动或配置索引器，不维护扫描范围、排除规则或索引数据库。全局查询依赖 Baloo 的索引状态和范围，支持手动刷新；已打开的本地目录通过现有属性监听重读文件标签。

## 浏览与编辑

- `tags:/` 显示标签目录，`tags:/工作` 显示标签内容，`/` 表示层级。支持表格、网格、筛选、键盘导航、面包屑和历史；标签项使用 Tag 图标。鼠标前进、后退侧键与工具栏按钮共用导航历史，Alt+↑ 返回父级。
- 纯标签目录只有名称列，没有详情入口或侧栏。包含真实文件时显示名称、修改时间、类型和大小，并提供文件详情。完整地址通过悬停查看。
- 右键菜单按打开、复制、整理和刷新分组。文件支持复制、定位到所在目录和编辑标签；标签集合不接受粘贴或新建文件夹，批量文件操作在真实目录中进行。
- 选中标签后，顶部按钮区、右键菜单和窄窗口菜单提供“删除标签”，与 Delete、Shift+Delete 共用确认流程。删除通过 Baloo/KIO 移除该标签及子标签在已索引文件上的关联，保留文件和其他标签，不进入回收站，也不提供撤销。收藏中的删除仍只移除收藏。
- 标签、最近、收藏、远端和卷位置均可固定为页签，重启后恢复顺序。
- 文件详情和属性窗口显示标签。只读标签与其他属性使用相同字号、文字行高，无额外上下内边距或描边；多标签换行，过长名称省略并可悬停查看。
- 编辑器使用 gpui-kit `Tag`：标签与行内输入共用边框、高度一致并自动换行。回车添加，空输入时 Backspace 删除末项；Ctrl+Z 撤销，Ctrl+Shift+Z / Ctrl+Y 重做。文字与标签增删共用历史，撤销添加恢复输入和选区。保存才写入，取消不修改文件。
- 后端配置和不可用错误在对应界面呈现；普通浏览页不显示索引实现提示。

## 数据与抽象边界

`gds::service::file_tags` 负责单文件标签读写和复制保留，由独立 Cargo feature `file-tags` 启用。它依赖 `desktop`，但仅启用 `desktop` 不包含标签能力；`files` 显式启用 `file-tags`。运行时开关与编译 feature 独立。

文件的 `user.xdg.tags` 是数据来源：UTF-8、英文逗号分隔、无 CSV 转义，清空时删除属性。标签保留大小写、空格和完整层级名称；不能包含英文逗号或 NUL，编辑器不接受换行。

保存前重新读取属性，只应用相对编辑器打开时的增删，保留其他应用追加的标签。xattr 没有 CAS，不能保证并发写入时的原子合并。本地复制和跨文件系统移动在恢复只读权限前保留标签；保留失败会报错，跨盘移动不会因此删除源文件。符号链接读取目标标签，编辑需先打开目标，复制链接不修改目标标签。原生 SFTP/SMB 不提供标签读写。

scheme 负责地址解析、导航与浏览语义；`TagIndex` 负责标签枚举、查询和全局标签删除。二者通过显式分发连接，不依赖动态插件 ABI。导航保留位置身份，标签条目与真实文件目标分开表示，不转换成虚构的本地路径。

| 地址 | 含义 |
| --- | --- |
| `file://…` | 本地文件或目录 |
| `sftp://…`、`smb://…` | 远端位置 |
| `location:/recent`、`/favorites`、`/volumes`、`/network`、`/trash` | 内置集合（均使用 `location:` scheme） |
| `location:/volume/<stable\|attachment>/<id>?path=<relative-path>` | 保留设备身份的卷位置 |
| `tags:/`、`tags:/工作` | 标签目录与内容 |

`location://…` 和 `tags://名字` 不作为兼容地址。地址栏显示可读 Unicode，保留 `%`、`?`、`#` 等必要转义；持久化和命令参数使用规范编码。内置位置的数据库键保存结构化类型、卷标识和相对路径，URI 拼写不改变这些记录的身份。

## 查询与限制

Baloo 后端通过 `kioclient ls` 枚举，再逐条 `stat` 区分标签与真实 `LOCAL_PATH`；标签删除调用 `kioclient remove tags:/…`，使用 Dolphin 相同的 tags worker。命令使用参数数组，不经过 shell；单次超时 15 秒，导航或配置变化取消旧查询，旧查询结果不覆盖新导航。没有链接 Qt/C++ 库。

- KDE Baloo 6.31 的 tags worker 对含双引号的标签查询返回错误的空集，GDS 对这类查询及全局删除明确报错；这些标签仍可通过文件属性读取、编辑和复制保留。
- CLI 逐行输出不能无损表示换行名称，不支持这类标签浏览。
- 查询没有分页，逐条 `stat` 不适合大型标签集合；KIO CLI 文本格式存在版本差异。

测试命令、真实后端环境要求和桌面检查方法见 [Files testing](testing.md)。

## 上游依据

- [KFileMetaData 标签编码](https://github.com/KDE/kfilemetadata/blob/master/src/usermetadata.cpp)
- [Baloo tags KIO 实现](https://github.com/KDE/baloo/blob/master/src/kioworkers/tags/kio_tags.cpp)
- [Dolphin 的 Baloo 集成](https://github.com/KDE/dolphin/blob/master/CMakeLists.txt)
- [URI 语法（RFC 3986）](https://www.rfc-editor.org/rfc/rfc3986.html#section-3)与 [Unicode 地址（RFC 3987）](https://www.rfc-editor.org/rfc/rfc3987.html)
