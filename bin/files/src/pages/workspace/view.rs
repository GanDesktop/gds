use super::geometry::{GRID_GAP, GRID_INSET, GRID_PADDING, GridMotion};
use super::model::Row;
use super::{model::*, ui::Browser};
use crate::entities::{
    file::{ViewMode, columns::Column, ui::EntryPresentation},
    location::Location,
};
use crate::shared::ui::actions::Command;
use anti_webui::prelude::*;
use gds::{
    appearance::tokens::DataListStyle,
    i18n,
    ui::{
        self, ContentPage, Frame, Icon, IconButton,
        page::{PageActions, PageContent, PageHeader, PageTableArea, PageTitle},
    },
};
use gds_uim::el;

const ROW_HEIGHT: f32 = DataListStyle::ROW_HEIGHT;
pub(super) const WORKSPACE_MOTION: Transition = Transition::ms(160).ease_out();
const DETAILS_WIDTH: f32 = 280.0;

#[derive(Clone, PartialEq)]
enum FileBody {
    Loading(Location),
    Empty(&'static str),
    RemoteUnavailable,
    Network,
    Collection(super::surfaces::CollectionBodyState),
    List,
    Grid,
}

impl Browser {
    pub fn view(&self) -> impl IntoView + use<> {
        let keyboard = self.clone();
        let mouse = self.clone();
        let prompt = self.clone();
        let dismiss = self.clone();
        let body = self.clone();
        let width = self.0.window_width;
        let hint_context = self.clone();
        let keytips = ui::keytips::Keytips::install(move || {
            hint_context.pane() == Pane::Files
                && matches!(hint_context.0.prompt.get(), Prompt::Closed)
                && matches!(
                    hint_context.0.menus.surface.get(),
                    super::menus::Surface::Closed
                )
                && !hint_context.0.connector.is_connecting()
        });
        let help = memo({
            let browser = self.clone();
            move || browser.pane() == Pane::Help
        });
        let browser = self.clone();
        let content = dyn_view(move || {
            let help = help.get();
            untrack(|| {
                if help {
                    browser.help_view().into_view()
                } else {
                    browser.workspace_view().into_view()
                }
            })
        });
        let prompt = ui::dialog::Dialog::show_alert(
            move || !matches!(prompt.0.prompt.get(), Prompt::Closed),
            move || dismiss.cancel_prompt(),
            move || body.prompt_view(),
        );
        el! { .div.{
            relative; flex; flex_col; size_full; min_w: 0.0_f32; min_h: 0.0_f32; focusable;
            node_ref: $(self.0.root_focus);
            on_resized: move (value, _) => $(width.set_if_changed(value));
            on_key_down: move event => ${keytips.key_down(event); keyboard.key_down(event);};
            on_key_up: move event => $(keytips.key_up(event));
            on_aux_click: move event => $(mouse.mouse_navigation(event));
        }.[
            content,
            $(self.0.notifications.view()),
            $(self.0.connector.view()),
            $(self.menu_overlay()),
            prompt,
        ] }
    }

    fn help_view(&self) -> impl IntoView {
        let frame = Frame::without_sidebar();
        let back = self.control(
            "return-to-files",
            Icon::Back,
            "page-return",
            Browser::back_page,
        );
        let toolbar = frame.toolbar(back, text(move || i18n::text("help")));
        let content = frame.workspace(div(), crate::widgets::help::FilesHelp);
        let footer = frame.footer(div(), div(), div());
        el! { frame.{node_ref: $(self.0.settings_node); focusable}.[toolbar, content, footer] }
    }

    fn workspace_view(&self) -> impl IntoView {
        let sidebar = self.clone();
        let sidebar_width = Binding::new(
            move || {
                if sidebar.sidebar_expanded() {
                    gds::appearance::tokens::SIDEBAR_WIDTH
                } else {
                    gds::appearance::tokens::COLLAPSED_SIDEBAR_WIDTH
                }
            },
            |_| {},
        );
        let presentation = ui::sidebar::SidebarPresentation::new(sidebar_width);
        let sidebar_width = presentation.width_binding();
        let frame = Frame::default().sidebar_width_binding(sidebar_width);
        let toolbar = frame.toolbar(self.menu_controls(), self.toolbar());
        let browser = self.clone();
        let content = dyn_view(move || match browser.pane() {
            Pane::Files => untrack(|| browser.file_content().into_view()),
            Pane::Help => div().into_view(),
            Pane::Settings => div()
                .node_ref(browser.0.settings_node)
                .focusable()
                .flex_1()
                .min_h(0.0_f32)
                .child(browser.0.settings.content())
                .into_view(),
        });
        let browser = self.clone();
        let sidebar = dyn_view(move || match browser.pane() {
            Pane::Files => untrack(|| browser.sidebar(presentation).into_view()),
            Pane::Help => div().into_view(),
            Pane::Settings => browser.0.settings.sidebar(presentation).into_view(),
        });
        let workspace = frame.workspace(sidebar, content);
        let size = self.clone();
        let terminal = self.clone();
        let workspace = el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32;
            on_resized: move (width, height) => $(size.terminal_workspace_resized(width, height));
        }.[
            .div.{flex; flex_col; flex_1; min_h: 0.0_f32; overflow_hidden}.[workspace],
            $(terminal.terminal_view(presentation)),
        ] };
        let status = self.clone();
        let footer_status = dyn_view(move || {
            if status.pane() == Pane::Files {
                let count = status.clone();
                el! { .div.{flex; items_center; gap: 12.0_f32; min_w: 0.0_f32}.[
                    $(move || count.status()).text.{
                        whitespace_nowrap;
                        dyn_style: style => $(style.text_size(gds::app::Gds::current().appearance().typography().scaled(12.0_f32)));
                    },
                    $(status.status_controls()),
                ] }
                .into_view()
            } else {
                el! { .div.{flex; items_center; min_w: 0.0_f32; gap: 12.0_f32}.[
                    $(status.0.settings.footer_text()), $(status.status_controls()),
                ] }
                .into_view()
            }
        });
        let footer = frame.footer(div(), footer_status, self.footer_controls());
        el! { frame.[toolbar, workspace, footer] }
    }

    pub(super) fn control(
        &self,
        id: &'static str,
        icon: Icon,
        label: &'static str,
        action: fn(&Browser),
    ) -> IconButton {
        let browser = self.clone();
        let button = el! {
            id.IconButton::new.{
                icon: icon;
                label_fn: move () => $(i18n::text(label));
                on_click: move _ => $(action(&browser));
            }
        };
        let command = match id {
            "sidebar" => Command::Sidebar,
            "back" => Command::Back,
            "forward" => Command::Forward,
            "up" => Command::Parent,
            "edit-path" => Command::EditLocation,
            "terminal" => Command::Terminal,
            "view" => Command::ViewMode,
            "favorite" => Command::Favorite,
            "hidden" => Command::Hidden,
            "details" => Command::Details,
            "refresh" => Command::Refresh,
            _ => return button,
        };
        button.keytip(command.default_stroke())
    }
    fn file_content(&self) -> impl IntoView {
        let title = self.clone();
        let content = self.clone();
        let body = memo({
            let browser = self.clone();
            move || browser.file_body()
        });
        let details = self.clone();
        let actions = self.clone();
        let notice = self.clone();
        let width = self.0.content_width;
        let focus_visible = self.0.list_focus_visible;
        let focus = self.clone();
        let keys = self.clone();
        let search = self.clone();
        let tab_id = memo({
            let browser = self.clone();
            move || browser.read(|state| state.tab().id)
        });
        let previous_tab = std::cell::Cell::new(untrack(|| tab_id.get()));
        let opacity = animated(1.0_f32);
        effect(move || {
            let tab = tab_id.get();
            if previous_tab.replace(tab) != tab && gds::appearance::motion::enabled() {
                opacity.set(0.88);
                opacity.to(1.0, WORKSPACE_MOTION);
            }
        });
        let details_width = follow(
            {
                let browser = self.clone();
                move || {
                    if !browser.compact() && browser.0.details.get() {
                        DETAILS_WIDTH
                    } else {
                        0.0
                    }
                }
            },
            WORKSPACE_MOTION,
        );
        el! { .div.{relative; flex; flex_1; min_w: 0.0_f32; min_h: 0.0_f32;
            dyn_style: move style => $(style.opacity(opacity.get())
                .outline(if focus_visible.get() {1.0_f32} else {0.0_f32})
                .outline_offset(-1.0_f32).outline_color(theme().focus_ring));
        }.[
            .ContentPage::default.[
                .PageHeader::default.[
                    .PageTitle::default.[
                        $(move || title.read(|state| title.location_title(state.tab().navigation.location()))).text.{ellipsis; min_w: 0.0_f32; w_full},
                    ],
                    .PageActions::default.{min_w: 0.0_f32}.[
                        $(self.folder_filter()),
                        $(move || actions.page_actions()).dyn_view,
                    ],
                ],
                (move () => $(notice.collection_notice())).dyn_view,
                .PageContent::default.{on_resized: move (value, _) => $(width.set_if_changed(value))}.[
                    .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32; focusable;
                        node_ref: $(self.0.content_focus);
                        on_focus_change: move focused => $(focus.0.list_focused.set(focused));
                        on_focus_visible: move visible => $(focus_visible.set_if_changed(visible));
                        text_input_target_when: $(self.0.filter_node), move () => $(search.filter_available() && search.0.preferences.snapshot().files.auto_focus_search);
                        on_key_down: move event => $(if keys.0.list_focused.get() { keys.content_key(event); });
                    }.[
                    (move () => ${
                        let body = body.get();
                        if body == FileBody::Grid {
                            let _ = gds::app::Gds::current().appearance().typography().scaled(1.0_f32);
                        }
                        untrack(|| content.render_file_body(body))
                    }).dyn_view,
                    ],
                    $(self.picker_footer()),
                ],
            ],
            .div.{flex; flex_col; flex_none; min_h: 0.0_f32; overflow_hidden;
                dyn_style: move style => $(style.w(details_width.get()));
            }.[ $(show(move || details_width.get() > 0.0, move || details.details_view())) ],
            $(ui::keytips::Anchor::new(Command::FocusContent.default_stroke())),
        ] }
    }

    fn file_body(&self) -> FileBody {
        if let Some(location) = self.read(|state| {
            let tab = state.tab();
            tab.navigation
                .pending_location()
                .filter(|location| {
                    matches!(
                        location,
                        Location::Directory(_) | Location::Volume { .. } | Location::Remote(_)
                    ) && (*location != tab.navigation.location() || tab.rows.is_empty())
                })
                .cloned()
        }) {
            return FileBody::Loading(location);
        }
        if self.read(|state| matches!(state.tab().navigation.location(), Location::Network)) {
            return FileBody::Network;
        }
        if let Some(collection) = self.collection_body_state() {
            return FileBody::Collection(collection);
        }
        self.read(|state| {
            let tab = state.tab();
            if matches!(tab.contents, Contents::Directory) {
                if matches!(tab.navigation.location(), Location::Remote(_)) {
                    return FileBody::RemoteUnavailable;
                }
                if tab.navigation.directory().is_none() {
                    return FileBody::Empty("load-failed");
                }
                if tab.rows.is_empty() {
                    let label = if !tab.navigation.view().filter.is_empty() {
                        "empty-filter"
                    } else if state
                        .picker
                        .as_ref()
                        .is_some_and(|picker| picker.projection().narrows())
                    {
                        "picker-empty-files"
                    } else {
                        "empty-directory"
                    };
                    return FileBody::Empty(label);
                }
            }
            match tab.mode {
                ViewMode::List => FileBody::List,
                ViewMode::Grid => FileBody::Grid,
            }
        })
    }

    fn render_file_body(&self, body: FileBody) -> View {
        let restores_view = matches!(body, FileBody::List | FileBody::Grid);
        let view = match body {
            FileBody::Loading(location) => self.location_loading(location).into_view(),
            FileBody::Empty(label) => self.empty_view(label).into_view(),
            FileBody::RemoteUnavailable => self.remote_unavailable().into_view(),
            FileBody::Network => self.network_content().into_view(),
            FileBody::Collection(state) => state.view(self).into_view(),
            FileBody::List => self.list_view().into_view(),
            FileBody::Grid => self.grid_view().into_view(),
        };
        if restores_view {
            let (id, position) =
                self.read(|state| (state.tab().id, state.tab().navigation.view().scroll));
            let weak = self.downgrade();
            request_frame(move |_| {
                if let Some(browser) = weak.upgrade()
                    && browser.pane() == Pane::Files
                    && browser.read(|state| state.tab().id == id)
                {
                    browser.0.list.scroll_to(0.0, position);
                }
            });
        }
        view
    }

    fn location_loading(&self, location: Location) -> Div {
        let destination = self
            .read(|state| Self::location_address(&location, state.tab().navigation.pending_path()));
        let cancel = self.clone();
        let connector = self.0.connector.clone();
        el! { .div.{flex; flex_col; flex_1; size_full; items_center; justify_center; gap: 16.0_f32; px: 24.0_f32}.[
            .spinner,
            (move () => $(i18n::text(if connector.is_connecting() {"remote-connecting"} else {"loading"}))).text.{font_weight: FontWeight::MEDIUM},
            destination.text.{max_w: 100.0_f32.pct; ellipsis; whitespace_nowrap; text_size: 12.0_f32;
                dyn_style: style => $(style.text_color(theme().text_muted));
            },
            $(i18n::text("operation-cancel")).quiet_button.{flex_row_reverse;
                on_click: move _ => ${cancel.cancel_location_load(); cancel.focus_content_after_mount();};
            }.[ $(Icon::Close.view(16.0)) ],
        ] }
    }

    fn remote_unavailable(&self) -> Div {
        let destination = self.read(|state| state.tab().navigation.location().display_address());
        let retry = self.clone();
        let network = self.clone();
        el! { .div.{flex; flex_col; flex_1; size_full; items_center; justify_center; gap: 16.0_f32; px: 24.0_f32}.[
            $(i18n::text("remote-not-open")).text.{font_weight: FontWeight::MEDIUM},
            destination.text.{max_w: 100.0_f32.pct; ellipsis; whitespace_nowrap; text_size: 12.0_f32;
                dyn_style: style => $(style.text_color(theme().text_muted));
            },
            .div.{flex; flex_wrap; justify_center; gap: 8.0_f32}.[
                $(i18n::text("retry")).quiet_button.{flex_row_reverse; on_click: move _ => $(retry.refresh());}.[ $(Icon::Refresh.view(16.0)) ],
                $(i18n::text("remote-back-network")).quiet_button.{flex_row_reverse; on_click: move _ => $(network.open(Location::Network));}.[ $(Icon::Back.view(16.0)) ],
            ],
        ] }
    }

    fn empty_view(&self, label: &'static str) -> Div {
        let target = el! { .div.{flex; flex_1; min_h: 0.0_f32; min_w: 0.0_f32; items_center; justify_center; px: 24.0_f32}.
        [ (move () => $(i18n::text(label))).text ] };
        self.directory_menu(self.background_drop_target(target))
    }

    fn column_width(&self, column: Column) -> f32 {
        if column == Column::Name
            && let Some(columns) = self.collection_columns()
        {
            return columns[0].1;
        }
        self.0.column_widths.with(|widths| {
            widths
                .get(&column)
                .copied()
                .unwrap_or_else(|| match column {
                    Column::Name => {
                        (self.0.content_width.get() * 0.45_f32).clamp(180.0_f32, 500.0_f32)
                    }
                    Column::Modified | Column::Accessed => 160.0_f32,
                    Column::Type => 110.0_f32,
                    Column::Size => 100.0_f32,
                    Column::Permissions => 96.0_f32,
                    Column::Owner | Column::Group | Column::DetailedType => 130.0_f32,
                })
        })
    }
    fn table_width(&self) -> f32 {
        let viewport = self.table_viewport_width();
        if let Some(columns) = self.collection_columns() {
            return columns
                .iter()
                .map(|(_, width)| width)
                .sum::<f32>()
                .max(viewport);
        }
        self.read(|state| {
            state
                .tab()
                .columns
                .columns()
                .iter()
                .map(|column| self.column_width(*column))
                .sum::<f32>()
        })
        .max(viewport)
    }
    fn column_header(&self, column: Column) -> Div {
        let width = self.clone();
        let sort = self.clone();
        let resize = self.clone();
        let start = std::cell::Cell::new(self.column_width(column));
        let sortable = self.sortable_columns().contains(&column);
        let sorting = self.read(|state| state.tab().sort);
        let active = sorting.key == column;
        let ascending = sorting.direction == crate::entities::file::sorting::Direction::Ascending;
        el! {
            .div.{
                flex; flex_none; items_center; min_w: 0.0_f32; h_full;
                dyn_style: move style => $(style.w(width.column_width(column)));
            }.[
                .div.{
                    flex; flex_1; items_center; min_w: 0.0_f32; h_full; pr: 8.0_f32;
                    pl: $(if column == Column::Name {40.0_f32} else {12.0_f32});
                    cursor: CursorIcon::Pointer; focusable;
                    disabled: move () => $( !sortable );
                    on_click: move _ => $(sort.sort_by(column));
                    hover: style => $(style.bg(theme().hover));
                }.[
                    $(i18n::text(column.label_id())).text.{ellipsis; flex_1; min_w: 0.0_f32; text_color: $(theme().text_muted); whitespace_nowrap;
                        dyn_style: style => $(style.text_size(gds::app::Gds::current().appearance().typography().scaled(DataListStyle::HEADER_SIZE)));
                    },
                    if sortable [
                        .div.{flex; flex_col; items_center; justify_center; w: 12.0_f32; h: 18.0_f32;
                            text_color: $(if active {theme().text} else {theme().text_muted});
                        }.[
                            if $( !active || ascending ) [$(Icon::ChevronUp.view(8.0_f32))],
                            if $( !active || !ascending ) [$(Icon::ChevronDown.view(8.0_f32))],
                        ],
                    ],
                ],
                .div.{
                    w: 5.0_f32; h_full; cursor: CursorIcon::EwResize;
                    on_grab: move event => ${
                        match event.phase {
                            GrabPhase::Start => start.set(resize.column_width(column)),
                            GrabPhase::Move => resize.0.column_widths.update(|widths| {
                                widths.insert(column, (start.get() + event.total.x).clamp(60.0_f32, 900.0_f32));
                            }),
                            GrabPhase::Cancel => resize.0.column_widths.update(|widths| {
                                widths.insert(column, start.get());
                            }),
                            GrabPhase::End => {
                                let widths = resize.0.column_widths.get_untracked().into_iter()
                                    .map(|(column, width)| (column.id().to_owned(), width)).collect();
                                resize.update(|state| state.places.set_widths(
                                    state.tab().navigation.location().clone(), widths,
                                ));
                                resize.save_places();
                            },
                        }
                    };
                },
            ]
        }
    }
    fn list_view(&self) -> Div {
        let count = self.clone();
        let rows = self.clone();
        let scroll = self.clone();
        let header = self.clone();
        let table = self.clone();
        let viewport = node_ref();
        let empty = memo({
            let browser = self.clone();
            move || browser.collection_empty_label()
        });
        let list = virtual_list(
            move || count.read(|state| state.tab().rows.len()),
            ROW_HEIGHT,
            move |index| rows.visible_row(index, None),
        );
        let target = el! {
            .PageTableArea::default.[
                .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32; overflow_x_scroll; node_ref: viewport}.[
                    .div.{flex; flex_col; flex_1; min_h: 0.0_f32; dyn_style: move style => $(style.min_w(table.table_width()))}.[
                $(move || {
                    if let Some(header) = header.collection_header() {
                        return header;
                    }
                    let columns = header.read(|state| state.tab().columns.clone());
                    let cells = columns.columns().iter().map(|column| header.column_header(*column)).collect::<Vec<_>>();
                    el! {.div.{flex; flex_none; h: 30.0_f32; border_b: 1.0_f32; border_color: $(theme().border)}.[..cells]}
                }).dyn_view,
                .div.{relative; flex; flex_col; flex_1; min_h: 0.0_f32}.[list.{
                    node_ref: $(self.0.list);
                    on_scrolled: move point => $(scroll.viewport_changed(point));
                }, (move () => ${empty.get().map(|label| el! { .div.{absolute; inset: 0.0_f32; flex; items_center; justify_center; pointer_events_none}.[
                    $(i18n::text(label)).text.{dyn_style: style => $(style.text_color(theme().text_muted))},
                ] })}).dyn_view],
                    ],
                ],
            ]
        };
        let target = el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32}.[target] };
        self.directory_menu(self.marquee_surface(
            self.background_drop_target(target),
            super::marquee::Layout::List { viewport },
        ))
    }
    fn grid_view(&self) -> Div {
        let count = self.clone();
        let groups = self.clone();
        let scroll = self.clone();
        let geometry = self.grid_geometry();
        let motion = GridMotion::new(self);
        let grid = virtual_list(
            move || {
                count
                    .read(|state| state.tab().rows.len())
                    .div_ceil(count.grid_columns())
            },
            geometry.row_height,
            move |group| {
                let browser = groups.clone();
                let layout = memo({
                    let browser = browser.clone();
                    move || {
                        (
                            browser.read(|state| state.tab().rows.len()),
                            motion.target.get().columns,
                        )
                    }
                });
                dyn_view(move || {
                    let (count, columns) = layout.get();
                    let cells = (group * columns..((group + 1) * columns).min(count))
                        .map(|index| browser.visible_row(index, Some(motion)))
                        .collect::<Vec<_>>();
                    el! {.div.{flex; items_center; h: geometry->row_height; gap: GRID_GAP; pl: GRID_INSET}.[..cells]}
                })
            },
        );
        let target = el! {
            .PageTableArea::default.{px: 0.0_f32}.[
            grid.{
                node_ref: $(self.0.list);
                on_scrolled: move point => $(scroll.viewport_changed(point));
            },
            ]
        };
        let target = el! { .div.{flex; flex_col; flex_1; min_h: 0.0_f32; min_w: 0.0_f32}.[target] };
        self.directory_menu(self.marquee_surface(
            self.background_drop_target(target),
            super::marquee::Layout::Grid(motion),
        ))
    }
    fn visible_row(&self, index: usize, motion: Option<GridMotion>) -> impl IntoView + use<> {
        let browser = self.clone();
        let content = memo(move || {
            let revision = browser.0.revision.get();
            let state = browser.0.state.borrow();
            let tab = state.tab();
            let entry = match tab.rows.get(index) {
                Some(Row::File(entry)) => Ok(entry.clone()),
                _ => Err(revision),
            };
            (tab.id, entry, tab.columns.clone())
        });
        let browser = self.clone();
        dyn_view(move || {
            content.with(|_| ());
            untrack(|| browser.row_view(index, motion))
        })
    }
    fn row_view(&self, index: usize, motion: Option<GridMotion>) -> Div {
        let grid = motion.is_some();
        let Some((row, columns)) = untrack(|| {
            self.read(|state| {
                state
                    .tab()
                    .rows
                    .get(index)
                    .map(|row| (row.clone(), state.tab().columns.clone()))
            })
        }) else {
            return div();
        };
        let select = self.clone();
        let open = self.clone();
        let context = self.clone();
        let key = row.key();
        let cut = row.path().map(std::path::Path::to_owned);
        let label = row.label();

        let geometry = self.grid_geometry();
        let selected = Binding::new(
            {
                let browser = self.clone();
                let key = key.clone();
                move || browser.read(|state| state.tab().navigation.view().selection.contains(&key))
            },
            |_| {},
        );
        let selection_progress = follow(
            move || if selected.get() { 1.0_f32 } else { 0.0_f32 },
            WORKSPACE_MOTION,
        );
        let appearance = Binding::new(move || selection_progress.get(), |_| {});
        let hidden = match &row {
            Row::File(entry) => entry.is_hidden,
            Row::Remote(entry) => entry.name.starts_with('.'),
            _ => false,
        };
        let hovered = signal(false);
        let name_width = self.clone();
        let cells = if grid {
            vec![
                el! { .div.{flex; items_center; justify_center; flex_none; w_full; h: geometry->artwork_height}.[
                    $(self.artwork(&row, geometry.icon_size, appearance)),
                ] }.into_view(),
                el! { .div.{w_full; min_w: 0.0_f32; h: geometry->name_height; flex_none; py: 1.0_f32;
                    text_align: TextAlign::Center; overflow_hidden;
                }.[label.text.{line_clamp: 2; overflow_wrap_anywhere; w_full; min_w: 0.0_f32;
                    text_size: geometry->name_size; line_height: $(LineHeight::Px(geometry.line_height));
                }] }.into_view(),
            ]
        } else {
            let tooltip = label.clone();
            let name = el! { .div.{flex; flex_none; items_center; h_full; gap: 10.0_f32; px: 12.0_f32; min_w: 0.0_f32;
                dyn_style: move style => $(style.w(name_width.column_width(Column::Name)));
            }.[
                $(self.row_artwork_with_selection(&row, 18.0_f32, appearance)),
                label.text.{ellipsis; flex_1; min_w: 0.0_f32; whitespace_nowrap; overflow_hidden;
                    dyn_style: style => $(style.text_size(gds::app::Gds::current().appearance().typography().scaled(DataListStyle::NAME_SIZE)));
                },
            ] };
            let name = ui::tooltip::tooltip(name, tooltip);
            if let Some(metadata) = self.collection_cells(&row, selected) {
                std::iter::once(name.into_view()).chain(metadata).collect()
            } else {
                let mut name = Some(name);
                columns
                    .columns()
                    .iter()
                    .map(|column| {
                        if *column == Column::Name {
                            name.take().expect("one name column").into_view()
                        } else {
                            self.cell(&row, *column, appearance).into_view()
                        }
                    })
                    .collect::<Vec<_>>()
            }
        };
        let target = el! { .div.{
            flex; items_center; flex_none; h_full; rounded: 6.0_f32;
            relative; select_none; cursor: CursorIcon::Pointer;
            on_hover: move inside => $(hovered.set_if_changed(inside));
            if grid {w: geometry->width; h: geometry->tile_height; flex_col; p: GRID_PADDING; gap: GRID_GAP; border: 1.0_f32; rounded: 8.0_f32};
            dyn_style: move style => ${
                let progress = selection_progress.get();
                let colors = gds::app::Gds::current().appearance().theme().colors();
                let ghosted = cut.as_ref().is_some_and(|path| {
                    crate::shared::lib::clipboard::Clipboard::current().is_cut(path)
                        || super::drag::FileDrag::is_source(path)
                });
                let background = if progress == 0.0 && hovered.get() {colors.hover()}
                    else {colors.selection.with_alpha(colors.selection.a * progress)};
                let style = style.bg(background)
                    .text_color(colors.foreground.mix(colors.selection_foreground, progress))
                    .border_color(if grid {colors.primary.with_alpha(colors.primary.a * progress)} else {Color::TRANSPARENT})
                    .opacity(if ghosted {0.35_f32} else if hidden {0.65_f32} else {1.0_f32});
                if let Some(motion) = motion {motion.tile_style(style, index)} else {style}
            };
            on_click: move event => ${
                select.select_row(index, event.modifiers);
                select.focus_content();
                if event.click_count >= 2 {open.open_row(index, false);}
            };
            on_aux_click: move event => $(if event.button == Some(PointerButton::Secondary) {
                context.select_context(index);
            });
        }.[..cells] };
        let target = if grid {
            ui::tooltip::tooltip(target, row.label())
        } else {
            target
        };
        let target = self.file_row_drag(target, &row, motion.map(|_| geometry));
        let target = untrack(|| self.row_menu(target, index));
        if let Some(motion) = motion {
            el! { .div.{flex_none; h: geometry->tile_height;
                dyn_style: move style => $(style.w(motion.target.get().width));
            }.[target] }
        } else {
            target
        }
    }
    fn cell(&self, row: &Row, column: Column, selection: Binding<f32>) -> Div {
        let current_owner = matches!((row, column), (Row::File(entry), Column::Owner)
            if EntryPresentation::owner(entry).current);
        let label = |value: String| {
            el! {
                value.text.{ellipsis; w_full; min_w: 0.0_f32; whitespace_nowrap; overflow_hidden}
            }
            .into_view()
        };
        let content = match (row, column) {
            (Row::File(entry), Column::Modified) => {
                gds::ui::time::Timestamp::new(entry.modified).into_view()
            }
            (Row::File(entry), Column::Accessed) => {
                gds::ui::time::Timestamp::new(entry.details.accessed).into_view()
            }
            (Row::Trash(entry), Column::Modified) => {
                gds::ui::time::Timestamp::new(entry.deleted_at).into_view()
            }
            (Row::File(entry), Column::Name) => label(entry.display_name.clone()),
            (Row::File(entry), Column::Type) => label(EntryPresentation::type_label(entry)),
            (Row::File(entry), Column::Size) => label(EntryPresentation::size_label(entry)),
            (Row::File(entry), Column::Owner) => label(EntryPresentation::owner(entry).text),
            (Row::File(entry), Column::Group) => label(EntryPresentation::group(entry)),
            (Row::File(entry), Column::Permissions) => {
                entry.details.permissions.as_ref().map_or_else(
                    || label("—".into()),
                    |permissions| {
                        ui::tooltip::tooltip(
                            div().min_w(0.0).child(label(permissions.symbols())),
                            permissions.octal(),
                        )
                        .into_view()
                    },
                )
            }
            (Row::File(entry), Column::DetailedType) => {
                label(EntryPresentation::detailed_type(entry))
            }
            (Row::Remote(entry), Column::Size) => label(
                entry
                    .size
                    .map(EntryPresentation::format_size)
                    .unwrap_or_default(),
            ),
            (Row::Trash(entry), Column::Size) => label(
                entry
                    .size
                    .map(EntryPresentation::format_size)
                    .unwrap_or_default(),
            ),
            _ => div().into_view(),
        };
        let width = self.clone();
        el! { .div.{
            flex; flex_none; items_center; h_full; min_w: 0.0_f32; px: 12.0_f32;
            if $(column == Column::Size) {justify_end; text_align: TextAlign::End};
            if current_owner {font_weight: FontWeight::SEMIBOLD};
            dyn_style: move style => ${
                let gds = gds::app::Gds::current();
                let colors = gds.appearance().theme().colors();
                let foreground = if current_owner {colors.foreground} else {colors.muted_foreground};
                style.w(width.column_width(column)).text_size(gds.appearance().typography().scaled(DataListStyle::META_SIZE))
                    .text_color(foreground.mix(colors.selection_foreground, selection.get()))
            };
        }.[content] }
    }
    fn footer_controls(&self) -> Div {
        let browser = self.clone();
        el! { .div.[$(move || {
            if browser.pane() != Pane::Files || browser.read(|state| state.picker.is_some()) || !browser.0.preferences.snapshot().terminal_enabled() {
                return div();
            }
            let visible = browser.0.terminal.get();
            let available = browser.read(|state| state.tab().navigation.path().is_some());
            let label = if visible { "files-terminal-hide" } else { "files-terminal-show" };
            el! { .div.{flex; items_center}.[
                $(browser.control("terminal", Icon::Terminal, label, Browser::toggle_terminal).toggled(visible).disabled(!available)),
            ] }
        }).dyn_view] }
    }
    fn page_actions(&self) -> Div {
        if self.compact() {
            return self.page_menu();
        }
        if let Some(actions) = self.special_page_actions() {
            return actions;
        }
        let grid = self.read(|state| state.tab().mode == ViewMode::Grid);
        let favorite =
            self.read(|state| state.places.is_favorite(state.tab().navigation.location()));
        let hidden = self.0.preferences.snapshot().files.show_hidden;
        let details = self.0.details.get();
        el! { .div.{flex; items_center; gap: 8.0_f32}.[
            $(self.control("view", if grid {Icon::List} else {Icon::Grid}, "view-toggle", Browser::toggle_view)),
            $(self.control("favorite", Icon::Star, "favorite-toggle", Browser::favorite).active(favorite)),
            $(self.column_menu()),
            $(self.control("hidden", Icon::Hidden, "show-hidden", |browser| browser.edit_preferences(|preferences| {
                preferences.files.show_hidden = !preferences.files.show_hidden;
            })).active(hidden)),
            $(self.control("details", Icon::Details, "details-toggle", Browser::toggle_details).active(details)),
            $(self.control("refresh", Icon::Refresh, "refresh", Browser::refresh)),
        ] }
    }
    fn status(&self) -> String {
        self.read(|state| match &state.operation {
            Operation::Running { label, .. } => label.clone(),
            Operation::Idle => {
                let tab = state.tab();
                if matches!(tab.contents, Contents::Volumes) {
                    return i18n::count("volume-count", tab.rows.len());
                }
                if tab.navigation.is_loading() {
                    i18n::text("loading")
                } else {
                    i18n::count("item-count", tab.rows.len())
                }
            }
        })
    }
    fn prompt_view(&self) -> View {
        let prompt = self.0.prompt.get();
        let cancel = self.clone();
        match prompt {
            Prompt::Closed => div().into_view(),
            Prompt::About => {
                let about = ui::about::About::new(ui::about::Application::cargo("gds-files"));
                el! { .div.{flex; flex_col; w: ui::about::WIDTH; h: ui::about::HEIGHT}.[about] }
                    .into_view()
            }
            Prompt::Confirm {
                title,
                message,
                primary,
                accept,
            } => {
                let browser = self.downgrade();
                let apply = move || {
                    if let Some(browser) = browser.upgrade() {
                        browser.0.prompt.set(Prompt::Closed);
                        accept();
                    }
                };
                el! {
                    .ui::dialog::Dialog::default.{title: title;
                        decisions: .ui::dialog::DecisionActions::default.[
                            (move () => &cancel.{cancel_prompt}).ui::dialog::DialogDecision::cancel,
                            apply.ui::dialog::DialogDecision::confirm.{label:primary},
                        ];
                    }.[message.text]
                }
                .into_view()
            }
            Prompt::Name { form, submit, .. } => form.view(self, submit),
        }
    }
    pub(super) fn select_row(&self, index: usize, modifiers: Modifiers) {
        self.update_selection(|state| {
            let multiple = state.allows_multiple();
            let tab = state.tab_mut();
            let Some(key) = tab.rows.get(index).map(Row::key) else {
                return;
            };
            let keys = tab.backing.clone();
            let anchor = tab
                .navigation
                .view()
                .anchor
                .as_ref()
                .and_then(|anchor| tab.rows.iter().position(|row| row.key() == *anchor))
                .unwrap_or(index);
            let view = tab.navigation.view_mut();
            if modifiers.shift && multiple {
                view.selection
                    .set_range(&keys, anchor.min(index), anchor.max(index));
            } else if modifiers.primary() && multiple {
                view.selection.toggle(key.clone());
                view.anchor = Some(key.clone());
            } else {
                view.selection.set_single(key.clone());
                view.anchor = Some(key.clone());
            }
            view.cursor = Some(key);
        });
    }
    pub(super) fn select_all(&self) {
        self.update_selection(|state| {
            if !state.allows_multiple() {
                return;
            }
            let allowed = state.picker.as_ref().map(|picker| {
                state
                    .tab()
                    .rows
                    .iter()
                    .filter_map(|row| match row {
                        Row::File(entry) if picker.accepts_entry(entry) => Some(row.key()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            });
            let tab = state.tab_mut();
            let view = tab.navigation.view_mut();
            if let Some(allowed) = allowed {
                view.selection.replace(allowed);
            } else {
                let backing = tab.backing.clone();
                if !backing.is_empty() {
                    let last = backing.len() - 1;
                    view.selection.set_range(&backing, 0, last);
                }
            }
        });
    }
}
