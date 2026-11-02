# Sidebar anatomy

公共组件位于 `gds::ui::sidebar`，结构参考 [shadcn Sidebar](https://ui.shadcn.com/docs/components/sidebar#composition)。应用用普通 children 组合这些部分，可以放多个分区，也可以插入分隔线、分割布局或自定义控件。

```text
SideBar
├─ SideBarHeader
├─ SideBarContent
│  ├─ SideBarSection
│  │  ├─ SideBarSectionHeader
│  │  │  └─ SideBarSectionTitle
│  │  └─ SideBarSectionContent
│  └─ SideBarSection …
└─ SideBarFooter
```

| 组件 | 默认布局 |
| --- | --- |
| `SideBar` | 纵向排列、填满高度，默认展开，根据 `SidebarState` 选择展开或折叠宽度 |
| `SideBarHeader` / `SideBarFooter` | 纵向排列，保留内容的自然高度，位于滚动区外 |
| `SideBarContent` | 填满剩余高度，默认纵向滚动 |
| `SideBarSection` | 纵向分组，默认按内容高度排列 |
| `SideBarSectionHeader` | 横向居中对齐，承载标题、动作或分组折叠控件 |
| `SideBarSectionTitle` | 统一的小标题排版，接受可翻译文本 |
| `SideBarSectionContent` | 纵向承载任意条目，默认随内容展开 |

所有结构容器都支持 `ParentElement`、`Styled`、交互方法和 `.id(...)`，可直接用于 UIM。它们只提供布局默认值，内边距、间距、边框和颜色由调用方设置；侧栏材质继续由 `Frame` 绘制。

组件有完整默认值时，优先通过 `Component::default()` 创建，再用属性设置状态、宽度和 ID 等配置。`SideBar`、头部、内容、底部和分区容器统一遵循这一约定；`new(...)` 留给初始化必需或定义组件内容的参数，例如 `SideBarSectionTitle::new(label)`。

使用滚动时，通过 `.id(...)` 提供稳定身份，或通过 `.track_scroll(...)` 传入显式的滚动句柄；关闭滚动的容器不为此要求 ID。

下面的 `header`、`tabs`、`locations` 和 `footer` 是应用已经渲染好的元素：

```rust,ignore
use gds::{appearance::tokens, i18n::t, ui::sidebar::*};
use gds_uim::el;
use gpui_kit::prelude::*;

let state = if collapsed {
    SidebarState::Collapsed
} else {
    SidebarState::Expanded
};
let sidebar = el! {
    .SideBar::default { state: state } [
        .SideBarHeader::default [header],
        .SideBarContent::default {
            # "navigation";
            px: tokens::SIDEBAR_PADDING.px;
            gap_2;
        } [
            .SideBarSection::default [
                .SideBarSectionHeader::default { h: tokens::ICON_BUTTON_SIZE.px;
                    @if (collapsed) invisible; @end;
                } [cx.t!("tabs").SideBarSectionTitle::new],
                .SideBarSectionContent::default [..tabs],
            ],
            .SideBarSection::default [
                .SideBarSectionHeader::default { h: tokens::ICON_BUTTON_SIZE.px;
                    @if (collapsed) invisible; @end;
                } [cx.t!("locations").SideBarSectionTitle::new],
                .SideBarSectionContent::default [..locations],
            ],
        ],
        .SideBarFooter::default [footer],
    ]
};
let frame = gds::ui::Frame::default().sidebar_width(sidebar.width());
```

The application owns the collapsed target state; toggles only update that state and notify the view. `SideBar` defaults to 222 px expanded and 58 px collapsed, with `expanded_width` / `collapsed_width` overrides. Static callers (including settings navigation) may continue using `state` and `width()` without opting into motion.

## Continuous presentation

Workspace hosts sample **once per render**, before building any column parts:

```rust,ignore
let sidebar = SideBar::default().state(state);
let presentation = sidebar.sample("workspace-sidebar", window, cx);
let frame = Frame::default().sidebar_width(presentation.width());
let sidebar = sidebar.presentation(presentation);
```

Pass the same `SidebarPresentation` to the header, navigation rows, footer, separators, and Files terminal left column. `NavItem::presentation(...)` clips and fades its label, moves its icon horizontally, and removes hidden trailing hit targets. Custom rows use `interpolate(expanded, collapsed)` for local horizontal measurements and `label_opacity()` for text only. Never start a transition per row or sample another clock in a child. An explicit presentation takes precedence over `SideBar`'s target width.

- The shared policy uses Kit's target-value transition: **160 ms ease-out cubic**, without a spring or overshoot. A stable host-local ID is required.
- First display adopts the current target immediately. Retargeting during motion starts from the current displayed width. Reduced motion immediately adopts the target, including when the preference changes during motion. Settled transitions stop requesting animation frames.
- Keep one content tree, stable IDs and one set of scroll handles. Narrowing does not expand sections or change row order, row heights, title slots, auxiliary-line heights, or split sizes. No expanded/rail tree swap.
- Labels are single-line and clipped (with optional text-only fading), never scaled or wrapped into taller rows. Main icons interpolate to the narrow column axis. Header/menu/new controls follow the same progress rather than changing alignment at an endpoint.
- Trailing controls are only interactive when fully expanded (`actions_visible()`). Keep narrow-state commands available through row/context menus or stable primary controls. Opacity alone is not sufficient to disable a hit target.
- Do not animate glass alpha, colors, or background layer count. The existing `Frame` surface remains a single stable surface; do not cross-fade duplicate navigation trees.

Headless tests advance GPUI's deterministic clock for reversal, reduced motion and frame scheduling. They do not establish FPS or replace a Linux + Wayland visual review.

## 展开与折叠的空间约定

侧栏折叠是同一导航结构的横向收窄。对相同的窗口高度、内容、分区展开状态和滚动位置，同一条目的点击区与主图标中心保持相同的纵坐标，切换后指针在原来的高度仍能命中原来的对象。

- **竖向节奏保持一致。** 两态使用相同的条目顺序、行高、行间距、分区标题高度及分隔线位置。普通导航行沿用 38 px；Files 的双行标签在两态均为 44 px。文字收起不作为压缩行高、挪动分区或重新分配可拖动区域的理由。
- **文字隐藏时保留空间。** 分区标题隐藏后保留标题槽位；标题带折叠动作时，收起态在原槽位居中保留折叠箭头和提示。卷的容量文字、未挂载状态等辅助行隐藏时仍保留原高度，容量条保持原来的纵向位置。不能因为删除一行文字而让之后的所有卷向上移动。
- **横向对齐有明确基准。** 展开态主图标按相同前缘对齐；折叠态在侧栏的对称 10 px 内缩区域中居中，点击区也随之居中。顶部主菜单、新建按钮、标签、位置和终端图标使用同一条折叠中轴。
- **辅助控件不参与主图标居中。** 窄列的关闭按钮、状态与快捷键角标放在独立覆盖层。隐藏控件不保留一个带 flex gap 的空子项；展开态的行尾动作可保留固定槽位，显示或隐藏不移动标签文字。
- **导航状态保持一致。** 两态复用同一份选中项、分区展开状态、滚动句柄及分割高度。侧栏收起不自动展开一个原本收起的分区；窗口缩放、用户拖动分界、主动展开分区或内容实际增删导致的位置变化按各自操作处理。

实现优先保留同一布局中的槽位，使用 Kit 的 `invisible` 隐藏非交互文字；只隐藏文字时，不给空白区域添加交互。不要为两个宽度分别维护一套行高、分区高度或滚动位置。

For independently scrolling sections, use `overflow_hidden` on `SideBarContent`, with stable IDs, `flex_1`, `min_h_0` and `overflow_y_scroll` on each `SideBarSectionContent`. Files retains its draggable tabs/devices split and bottom locations section. Agents uses the same grouped list and scroll handle at every presentation width.
