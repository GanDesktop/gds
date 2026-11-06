# 组件组合

有完整默认值的结构组件通过 `Component::default()` 创建。配置通过属性设置，内容使用普通 children；`new(...)` 用于初始化必需或定义元素内容的参数。每个部分都有自己的样式、ID 和交互方法，调用方的样式覆盖默认值。

## ContentPage

公共入口为 `gds::ui::page::*`：

```rust,ignore
el! {
    .ContentPage::default { # "directory-page" } [
        .PageHeader::default [
            .PageTitle::default [title],
            .PageActions::default [..actions],
        ],
        @if (let Some(notice) = notice)
            .PageNotice::default [notice],
        @end,
        .PageContent::default [body],
    ]
}
```

`PageHeader` 保留自然高度，`PageTitle` 负责标题排版，`PageActions` 承载任意操作；没有操作时可以省略整个操作区。`PageContent` 填满剩余空间，滚动和选择由正文组件管理。标题按应用字号缩放，页头留白与表格内缩继续遵循公共尺寸。

表格使用 `PageTableArea::default().id(id).child(table)`，将内缩放在 `ListSurface` 内，保证表格两侧空白参与取消选择；它也支持原有的上下文菜单方法。

### 加载与入口动作

读取目录时用 Kit `Spinner` 和明确的目标地址呈现加载状态，并提供取消；原位置在成功前保留，但不继续把旧内容展示成新位置的读取结果。取消后恢复原页面，过期响应不提交导航。

加号表示向当前集合添加一个对象；添加成功后留在集合并显示新条目。打开／连接由明确的独立操作触发，新标签只由新标签动作触发。协议选择先于认证字段，仅呈现所选协议和认证方式需要的输入。集合页按保存、连接和挂载等实际状态分组；某一分组没有数据时，不在其它有效条目下方再显示整页空表格。

网络页的添加、刷新、打开／连接和保存操作复用公共 `IconButton`，使用相同的 32 px 点击区、16 px 图标及悬停提示，不另用描边文字按钮。移除保存的位置使用垃圾桶图标，提示明确为“移除保存的位置”；它不删除远程文件。协议选择采用公共幽灵按钮承载整行内容，名称与说明在可伸缩的文字列中左对齐，尾部箭头保持自然宽度；不能让按钮的居中布局决定各项文字的起点。对话框确认与取消继续使用公共 `DialogActions`。这些布局与控件通过 `el!` 组合，遵循仓库的 UIM 约定。

## SettingRow

公共入口为 `gds::ui::settings::*`：

```rust,ignore
el! {
    .SettingRow::default [
        .SettingLabel::default [
            title,
            .SettingDescription::default [description],
        ],
        .SettingActions::default [
            .SettingReset::default [..reset],
            .SettingControl::default [control],
        ],
    ]
}
```

`SettingLabel` 是可伸缩的说明列，`SettingDescription` 提供次级排版，也可用于设置说明。`SettingActions` 按公共间距组合右侧控件；`SettingReset` 即使没有子元素也保留重置按钮的位置，`SettingControl` 统一控件列宽，避免默认值变化时发生位移。

字段值、默认值比较、校验、重置回调与保存继续由应用或类型化编辑器负责。布局组件接受已经构造好的控件。

## Agents ContentRow

这些部分只位于 `bin/agents/src/shared/ui/content_row.rs` 及其子模块，由 Agents 的 `ui` 模块导出：

```rust,ignore
el! {
    .ContentRow::default [
        .ContentRowLeading::default [icon],
        .ContentRowContent::default [
            .ContentRowTitle::default [title],
            .ContentRowDescription::default [description],
        ],
        .ContentRowActions::default [..actions],
    ]
}
```

行本身是被动布局。前置内容与操作保留自然宽度，正文可以收缩；点击行为属于调用方提供的按钮。音频和活动标题使用这个结构，独立资源卡片直接复用 Kit 的 `Attachment` 系列。前置内容和操作区都可以省略，普通 children 不隐含“操作”的含义。

## 导航、窗口和对话框

`gds::ui::navigation` 独立于 Sidebar，提供 `NavItemContent`、`NavItemHeader`、`NavItemLabel`、`NavItemDetails` 和 `NavItemActions`。这些默认容器组合导航条目的实际内容，选中态、快捷键与独立尾部操作继续由 `NavItem` 协调。图钉与悬停动作也位于这个模块，原有 `ui` 入口保留导出。

`Frame::default().sidebar_width(width)` 共用顶栏、正文和底栏的列边界；无侧栏页面仍可用 `Frame::without_sidebar()`。具体布局位于 `ui/frame/toolbar.rs`、`workspace.rs`、`footer.rs`，窗口拖动区域位于 `drag.rs`。`WindowControls` 和 `TitleBar` 统一使用 `default()`。

`DialogFooter` 是可以接受任意 children 的默认操作容器；`DialogDecision::new(DialogCommand)` 定义实际的确认／取消动作。常用组合通过以下方式创建：

```rust,ignore
DialogActions::default()
    .primary_label(confirm_label)
    .primary_icon(IconName::Check)
    .cancel_label(cancel_label)
```

省略文案时，在渲染时读取当前语言的确认／取消翻译。快捷键仍派发当前对话框的原有动作，校验和关闭回调由宿主拥有。

`VerticalSplit::new(state)` 只接收初始化必需的状态实体，其余内容通过 `.top(...)`、`.bottom(...)`、`.label(...)` 和 `.divider_background(...)` 设置。状态、分隔柄和布局分别组织在 `ui/split/` 中；分隔柄复用 Kit `resize_handle`，项目保留高度约束、偏好恢复、方向键步进和释放时的保存事件。

## 直接使用 Kit 的结构

空状态使用 `Empty` 的媒体、标题、说明和操作部分；消息使用 `Message` / `MessageContent`；资源卡片使用 `Attachment` 的媒体、正文、标题、说明和操作槽位。它们沿用库的组合 API，项目通过样式覆盖套件尺寸与材质，不再另写相同职责的公共骨架。

文本复制使用 Kit `Clipboard`，反馈、消息、资源及 Markdown 的复制仍保留原始文本，不把显示时的换行或截断结果写入剪贴板。键帽呈现由 `Kbd` 承担，快捷键解析与按住修饰键的省略规则仍由 `ui::shortcut` 管理。

## 折叠内容

展开和隐藏使用 `gpui_kit::base::Collapsible`。应用保留状态、触发按钮和滚动定位；普通 children 始终显示，`content` 指定折叠正文：

```rust,ignore
el! {
    .Collapsible::default { open: expanded } [header] {
        @if (expanded)
            content: $(render_body(cx));
        @end
    }
}
```

这里的条件同时推迟正文构造，避免在收起状态创建 Markdown 和工具结果。Agents 的活动面板只给这个 Kit 组件应用现有边框与留白。压缩错误作为普通子元素保留，正文随展开状态显示。

### File browser status controls

The footer starts with the item count, followed by background tasks, selection,
and clipboard controls. All three controls open their list only on click, above
the footer; hovering shows just the control name. In the compact layout, nonempty
controls show a count badge, including a count of one, without summary text.
In the expanded layout, they show summary text without a badge. Empty controls
show only their icon; explanatory empty states stay inside the popup. Switching
layouts animates text width and opacity and fades the badge over 160 ms, respecting
reduced motion. Compact controls sit directly beside each other in 32 px square
targets. Icons stay centered, including when a popover is open; badges overlay
the corner without participating in layout. Labels retain a full line box so
descenders are not clipped by the width animation.

Lists reuse Kit `ListItem` with 32 px rows and full-row hover feedback. The
trailing action appears while its row is hovered, with its space reserved. The
popup uses the same surface and 4 px outer padding as menus, without an extra
popover padding layer. File names and category icons share `FileLabel`
with the drag preview; full paths remain available in tooltips. Text truncates
on one line instead of expanding row spacing.
The task action cancels work, the selection action deselects that entry, and the
clipboard action removes that entry from the platform clipboard while preserving
the remaining entries and copy/cut mode. Cancellation remains pending until the
worker finishes cleanup. Completed tasks leave the list and report their outcome
through notifications.

### Directory pickers

Remote directory selection uses the shared `ui::picker::directory::DirectoryPicker`.
Its anatomy is `Picker`, `PickerToolbar`, `PickerLocation`, `PickerContent` and
`PickerStatus`; the host supplies the existing alert dialog and `DialogActions`.
Only the content region scrolls, so navigation and the decision footer remain visible.

Use Kit `Breadcrumb`, `Input` and `DataTable`, shared `IconButton` and `ListSurface`,
and `DataListStyle` row, typography and selection tokens. Folder names align left;
folders are rows, not full-width action buttons. A click selects a folder, a double
click or Enter enters it, and the primary footer action accepts the selection or
the current directory. Hidden folders are initially hidden. Ctrl+L edits the path,
Ctrl+F focuses the filter, Ctrl+H toggles hidden folders, Ctrl+R refreshes,
Alt+Left/Right traverses history, and Alt+Up opens the parent.

Loading, failure and empty results use Kit feedback components. Pending or failed
navigation cannot be confirmed. The host cancels outstanding reads when the dialog
closes; replacing a request discards its late response.
