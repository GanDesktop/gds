# 公共 About 组件

Files 与 Agents 共用 `gds::ui::about::About`。组件只呈现内容和内部导航，由应用决定放进页面、对话框还是独立窗口。

## 应用登记

版本沿用 Cargo 的 `package.version`。网站、开发者和许可证标识分别读取 `package.homepage`、`package.authors` 与 `package.license`；当前两个二进制属于同一个 Cargo package，因此版本一致。

`package.metadata.gds.about` 保存反馈链接、许可证文本路径及公共 Credits。需要 About 的图形应用在 `applications` 下登记自己的名称和图标：

```toml
[package.metadata.gds.about.applications.gds-example]
name = "GDS Example"
icon = "icons/folder.svg"
```

名称必须对应 `[[bin]].name`，构建脚本验证所有登记的应用目标。Portal 等无界面目标无需登记。图标路径由应用的 AssetSource 提供；公共开源项目及其链接只维护一份。所有应用嵌入根目录的 `LICENSE`，不在应用目录重复保存许可证。

应用内的身份图标使用 `assets/<应用>/icon-solid.svg`，由各应用嵌入，用于 About 和主菜单入口。桌面入口使用同目录的 `icon-desktop.svg`：Nix 包安装到 hicolor 图标目录，开发版直接引用工作区中的桌面版资源。

## 构建标识

`build.rs` 在编译时写入 `GDS_BUILD_TAG`，取 Git commit 的前九位。普通 Cargo 构建从当前仓库读取，追踪 HEAD 和引用变化；Nix 发布构建从 flake revision 传入，源码进入没有 `.git` 的沙箱后仍保留标识。发布包版本也读取 Cargo manifest。

其它源码归档构建可显式设置 `GDS_BUILD_TAG=<commit hash>`。既没有 Git 信息也没有显式 revision 时，显示 `unknown` 并给出构建警告，不把版本号当成 hash。运行时不需要 Git 或源码目录。

## 使用

```rust
use gds::ui::about::{About, Application};

let about = cx.new(|cx| {
    About::new(Application::cargo(env!("CARGO_BIN_NAME")), cx)
});
```

布局复用公共 `ContentPage`、`PageHeader`、`PageTitle` 与 `PageContent`。身份信息使用 Kit `DescriptionList`，复制使用 `Clipboard`，网站和反馈使用 `Link`，操作使用套件公共按钮；不再另外实现标题、复制控件或导航条目。

标题是图标和软件名。版本与 build tag 分开显示，各自的复制按钮复制原始值并显示完成反馈。网站与反馈打开外部链接；Credits 显示开发者和公共项目；License 显示随程序嵌入的完整许可证，可选择文本复制。详情通过返回按钮、Esc 或 Alt+Left 回到 About 首页；返回按钮沿用公共快捷键提示。

Files 对话框与 Agents 独立窗口使用公共 `DialogFooter`、`DialogDecision` 关闭，支持 Alt+C 和 Esc，并共用按钮的手形指针、悬停状态和快捷键提示。嵌入 Agents 页面时沿用宿主的返回按钮和快捷键，不重复放置关闭控件。

Files 使用 Kit `WindowExt::open_alert_dialog` 在当前 Root 中挂载 About，不创建新原生窗口。Dialog 使用原生主题背景；GDS 只让 Root 根表面透明，避免主窗口的材质设置使弹层也变透明。
