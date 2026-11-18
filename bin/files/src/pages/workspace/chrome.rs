//! Files chrome keeps address navigation, folder filtering, and sidebar regions distinct.
use super::{
    keyboard::{SidebarRegion, SidebarRow},
    model::{AddressPresentation, Pane},
    ui::Browser,
};
use crate::entities::{
    file::ui::EntryPresentation,
    location::{Collection, Location, LocationTarget, scheme::TagAddress, volumes},
};
use anti_webui::prelude::*;
use gds::{
    appearance::tokens::{SIDEBAR_ITEM_HEIGHT, SIDEBAR_PADDING},
    i18n,
    ui::{
        Icon, IconButton,
        capacity::{CapacityBar, CapacityStatus},
        menu::{context_menu, menu_item},
        sidebar::*,
    },
};
use gds_uim::el;
use std::path::{Component, Path, PathBuf};

const MIN_TABS_HEIGHT: f32 = 94.0;
const MIN_VOLUMES_HEIGHT: f32 = 108.0;
const SIDEBAR_GROUP_GAP: f32 = 6.0;
const SIDEBAR_ROW_GAP: f32 = 4.0;
const CAPACITY_BOTTOM_PADDING: f32 = 6.0;
const UNMOUNTED_STATUS_HEIGHT: f32 = 17.0;

struct Breadcrumb {
    label: String,
    location: Location,
    current: bool,
}
impl Breadcrumb {
    fn for_location(location: &Location, path: Option<&Path>) -> Vec<Self> {
        if let Some(path) = path {
            let components = path
                .components()
                .filter(|part| !matches!(part, Component::CurDir))
                .collect::<Vec<_>>();
            let mut target = std::path::PathBuf::new();
            return components
                .iter()
                .enumerate()
                .map(|(index, part)| {
                    target.push(part.as_os_str());
                    let current = index + 1 == components.len();
                    let name = part.as_os_str().to_string_lossy();
                    let label =
                        if current || matches!(part, Component::RootDir | Component::Prefix(_)) {
                            name.into_owned()
                        } else {
                            format!("{name}/")
                        };
                    Self {
                        label,
                        location: Location::from(target.as_path()),
                        current,
                    }
                })
                .collect();
        }
        if let Location::Tags(address) = location {
            let mut result = vec![Self {
                label: i18n::text("tags"),
                location: Location::Tags(TagAddress::new("").expect("root tag address")),
                current: address.tag().is_empty(),
            }];
            let mut prefix = String::new();
            let parts = address
                .tag()
                .split('/')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            for (index, part) in parts.iter().enumerate() {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                let current = index + 1 == parts.len();
                result.push(Self {
                    label: (*part).into(),
                    location: Location::Tags(TagAddress::new(&prefix).expect("valid tag ancestor")),
                    current,
                });
            }
            return result;
        }
        vec![Self {
            label: location.display_address(),
            location: location.clone(),
            current: true,
        }]
    }
}

impl Browser {
    pub(super) fn menu_controls(&self) -> Div {
        let browser = self.clone();
        el! { .div.{flex; items_center; justify_center; size_full}.[
            $(move || if browser.pane() == Pane::Files {
                browser.app_menu().into_view()
            } else {
                browser.control("return-to-files", Icon::Back, "page-return", Browser::back_page).into_view()
            }).dyn_view,
        ] }
    }

    pub(super) fn toolbar(&self) -> Div {
        let browser = self.clone();
        let secondary = self.clone();
        let secondary_title = self.clone();
        el! { .div.{flex; flex_1; items_center; gap: 4.0_f32; min_w: 0.0_f32}.[
            $(move || {
                let collapsed = browser.compact();
                let sidebar = browser.control("sidebar", Icon::Sidebar, "sidebar-toggle", |browser| {
                    browser.0.sidebar.update(|open| *open = !*open);
                });
                el! { .div.{flex; dyn_style: move style => $(if collapsed {style.hidden()} else {style.flex()})}.[sidebar] }
            }).dyn_view,
            .div.{flex; flex_1; min_w: 0.0_f32;
                dyn_style: move style => $(if secondary.pane() == Pane::Files {style.flex()} else {style.hidden()});
            }.[
                $(self.navigation_buttons()),
                $(self.address_bar()),
            ],
            .div.{flex_1; min_w: 0.0_f32; px: 8.0_f32;
                dyn_style: move style => $(if secondary_title.pane() == Pane::Settings {style.flex()} else {style.hidden()});
            }.[$(move || i18n::text("settings")).text],
        ] }
    }

    fn navigation_buttons(&self) -> View {
        let history = memo({
            let browser = self.clone();
            move || {
                browser.read(|state| {
                    let tab = state.tab();
                    let navigation = &tab.navigation;
                    let current = navigation.history_index();
                    let mut backward = Vec::new();
                    let mut forward = Vec::new();
                    if navigation.can_go_back() {
                        backward.extend(
                            navigation.history()[..current]
                                .iter()
                                .enumerate()
                                .rev()
                                .map(|(index, visit)| (index, visit.location.clone())),
                        );
                    }
                    if navigation.can_go_forward() {
                        forward.extend(
                            navigation
                                .history()
                                .iter()
                                .enumerate()
                                .skip(current + 1)
                                .map(|(index, visit)| (index, visit.location.clone())),
                        );
                    }
                    (tab.id, backward, forward)
                })
            }
        });
        let browser = self.clone();
        dyn_view(move || {
            if browser.compact() {
                return div();
            }
            let (id, backward, forward) = history.get();
            el! { .div.{flex; gap: 4.0_f32; mr: 4.0_f32}.[
                $(browser.history_button(id, backward, false)),
                $(browser.history_button(id, forward, true)),
                $(browser.control("up", Icon::Up, "parent", Browser::parent)),
            ] }
        })
    }

    fn history_button(&self, tab: u64, visits: Vec<(usize, Location)>, forward: bool) -> Div {
        let arrow = if forward {
            self.control("forward", Icon::Forward, "forward", |browser| {
                browser.history(true)
            })
        } else {
            self.control("back", Icon::Back, "back", |browser| browser.history(false))
        };
        let disabled = visits.is_empty();
        let button = div().flex().flex_none().child(arrow.disabled(disabled));
        if disabled {
            return button;
        }
        let entries = visits
            .into_iter()
            .map(|(index, location)| {
                let label = self.resolve_path(&location).map_or_else(
                    || self.location_title(&location),
                    |path| path.display().to_string(),
                );
                let browser = self.downgrade();
                menu_item(label)
                    .icon(location.icon())
                    .on_click(move || {
                        if let Some(browser) = browser.upgrade() {
                            browser.visit_history(tab, index, &location);
                            browser.focus_content_after_mount();
                        }
                    })
                    .into()
            })
            .collect();
        context_menu(button, entries)
    }

    fn address_bar(&self) -> Div {
        let editing = self.0.address_presentation;
        let submit = self.clone();
        let blur = self.clone();
        let keys = self.clone();
        let crumbs = self.clone();
        let edit = self.clone();
        let destination = memo({
            let browser = self.clone();
            move || {
                browser.read(|state| {
                    let navigation = &state.tab().navigation;
                    let (location, path) = if let Some(pending) = navigation.pending_location() {
                        (pending, navigation.pending_path())
                    } else {
                        (navigation.location(), navigation.path())
                    };
                    (
                        location.clone(),
                        path.map(Path::to_owned),
                        navigation.is_loading(),
                    )
                })
            }
        });
        el! { .div.{flex; flex_1; items_center; min_w: 0.0_f32; h: 34.0_f32;
            rounded: 6.0_f32; border: 1.0_f32;
            dyn_style: move style => $(style.border_color(
                if editing.get() == AddressPresentation::Breadcrumbs {theme().border} else {theme().focus_ring}
            ));
            on_click: move event => ${
                if event.button == Some(PointerButton::Primary)
                    && editing.get_untracked() == AddressPresentation::Breadcrumbs
                {
                    edit.start_address_edit();
                }
            };
        }.[
            .text_input.{
                value: $(self.0.address); node_ref: $(self.0.address_node);
                flex_1; min_w: 0.0_f32; h_full; border: 0.0_f32; bg: Color::TRANSPARENT;
                dyn_style: move style => $(if matches!(editing.get(), AddressPresentation::Editing {..}) {style.flex()} else {style.hidden()});
                on_submit: move _ => $(submit.open_address());
                on_focus_change: move focused => $(if !focused {blur.finish_address_edit();});
                on_key_down: move event => $(keys.address_key(event));
            },
            .div.{flex; flex_1; items_center; min_w: 0.0_f32;
                dyn_style: move style => $(if matches!(editing.get(), AddressPresentation::Breadcrumbs) {style.flex()} else {style.hidden()});
            }.[
                $(move || {
                    let (location, path, loading) = destination.get();
                    untrack(|| {
                        if loading {
                            let address = Self::location_address(&location, path.as_deref());
                            el! { .div.{flex_1; min_w: 0.0_f32; px: 8.0_f32}.[address.text.{ellipsis; whitespace_nowrap; w_full}] }
                        } else {
                            el! { .div.{flex; flex_1; min_w: 0.0_f32; items_center}.[
                                $(crumbs.breadcrumbs(&location, path.as_deref())),
                                $(crumbs.control("edit-path", Icon::Edit, "path-edit", Browser::start_address_edit)),
                            ] }
                        }
                    })
                }).dyn_view,
            ],
        ] }
    }

    fn breadcrumbs(&self, location: &Location, path: Option<&Path>) -> Div {
        let crumbs = Breadcrumb::for_location(location, path)
            .into_iter()
            .map(|crumb| {
                let open = self.clone();
                let label = crumb.label.clone();
                el! { .div.{
                    flex; flex_none; items_center; h: 32.0_f32; px: 4.0_f32;
                    rounded: 4.0_f32; cursor: CursorIcon::Pointer;
                    text_color: $(if crumb.current {theme().text} else {theme().text_muted});
                    hover: style => $(style.bg(theme().hover));
                    if $(!crumb.current) {
                        focusable;
                        access: move node => $(node.role(Role::Button).label(label.clone()));
                        on_click: move event => ${
                            event.stop_propagation();
                            open.open(crumb.location.clone());
                            open.focus_content_after_mount();
                        };
                    };
                }.[crumb->label.text.{whitespace_nowrap}] }
            })
            .collect::<Vec<_>>();
        let node = node_ref();
        request_frame(move |_| node.scroll_to(f32::MAX, 0.0));
        el! { .div.{flex; flex_1; items_center; min_w: 0.0_f32; gap: 4.0_f32; overflow_x_scroll;
            node_ref: node; on_resized: move (_, _) => $(node.scroll_to(f32::MAX, 0.0));
        }.[..crumbs] }
    }

    pub(super) fn start_address_edit(&self) {
        let value = self.read(|state| {
            let navigation = &state.tab().navigation;
            if let Some(pending) = navigation.pending_location() {
                Self::location_address(pending, navigation.pending_path())
            } else {
                Self::location_address(navigation.location(), navigation.path())
            }
        });
        batch(|| {
            self.0.address.set(value.clone());
            self.0
                .address_presentation
                .set(AddressPresentation::Editing {
                    original: value.clone(),
                });
        });
        let node = self.0.address_node;
        node.focus();
        node.set_text(&value, (0, value.len()));
    }

    pub(super) fn finish_address_edit(&self) {
        let AddressPresentation::Editing { original } = self.0.address_presentation.get_untracked()
        else {
            return;
        };
        self.0
            .address_presentation
            .set(AddressPresentation::Breadcrumbs);
        self.0.address.set(original);
    }

    fn address_key(&self, event: &KeyboardEvent) {
        if event.key == Key::Named(NamedKey::Escape) {
            self.finish_address_edit();
            self.focus_content_after_mount();
            event.prevent_default();
            event.stop_propagation();
        }
    }

    pub(super) fn folder_filter(&self) -> Div {
        let filter = self.clone();
        let keys = self.clone();
        let clear = self.clone();
        let value = self.0.filter;
        let node = self.0.filter_node;
        let browser = self.clone();
        el! { .div.{relative; flex; items_center; w: 200.0_f32; min_w: 96.0_f32; h: 26.0_f32;
            gap: 4.0_f32; px: 7.0_f32; rounded: 5.0_f32; border: 1.0_f32;
            dyn_style: move style => $(if browser.filter_available() { style.flex().border_color(theme().text_muted) } else { style.hidden() });
        }.[
            $(Icon::Search.view(14.0_f32)),
            .text_input.{
                value: $(self.0.filter); node_ref: $(self.0.filter_node);
                flex_1; min_w: 0.0_f32; h_full; p: 0.0_f32; border: 0.0_f32; bg: Color::TRANSPARENT;
                placeholder: $(i18n::text("filter-placeholder"));
                on_key_down: move event => $(keys.search_key(event));
                on_input: move value => $(filter.filter(value.to_owned()));
            },
            .div.{flex; items_center; justify_center; size: 18.0_f32; flex_none; rounded: 4.0_f32;
                cursor: CursorIcon::Pointer;
                dyn_style: move style => $(if value.with(String::is_empty) {style.hidden()} else {style.flex()});
                hover: style => $(style.bg(theme().hover));
                on_click: move _ => ${value.set(String::new()); clear.filter(String::new()); node.focus();};
            }.[ $(Icon::Close.view(14.0)) ],
            $(gds::ui::keytips::Anchor::new(crate::shared::ui::actions::Command::Filter.default_stroke()).trailing()),
        ] }
    }

    pub(super) fn filter_available(&self) -> bool {
        self.pane() == Pane::Files
            && self.read(|state| match state.tab().navigation.location() {
                Location::Recent | Location::Favorites | Location::Tags(_) => !self.compact(),
                Location::Remote(_) => self.compact(),
                Location::Directory(_) | Location::Volume { .. } => true,
                Location::Network | Location::Volumes | Location::Trash => false,
            })
    }

    pub(super) fn sidebar(&self, presentation: SidebarPresentation) -> impl IntoView {
        let browser = self.clone();
        let sections = memo({
            let browser = self.clone();
            move || {
                let preferences = browser.0.preferences.snapshot();
                (
                    preferences.files.sidebar.show_volumes
                        || browser.0.sidebar_reveal_volumes.get(),
                    preferences.tags.enabled,
                )
            }
        });
        let location_progress = follow(
            {
                let browser = self.clone();
                move || {
                    if browser
                        .0
                        .preferences
                        .snapshot()
                        .files
                        .sidebar
                        .locations_collapsed
                        && browser.0.sidebar_region.get() != Some(SidebarRegion::Locations)
                    {
                        0.0_f32
                    } else {
                        1.0_f32
                    }
                }
            },
            super::view::WORKSPACE_MOTION,
        );
        let volume_progress = follow(
            {
                let browser = self.clone();
                move || {
                    if browser
                        .0
                        .preferences
                        .snapshot()
                        .files
                        .sidebar
                        .volumes_collapsed
                        && browser.0.sidebar_region.get() != Some(SidebarRegion::Volumes)
                    {
                        0.0_f32
                    } else {
                        1.0_f32
                    }
                }
            },
            super::view::WORKSPACE_MOTION,
        );
        dyn_view(move || {
            let (show_volumes, show_tags) = sections.get();
            let home = untrack(|| browser.read(|state| state.home.clone()));
            let sidebar = browser.clone();
            let tabs = memo(move || {
                sidebar.read(|state| {
                    state
                        .tabs
                        .iter()
                        .map(|tab| {
                            let location = tab.navigation.location();
                            let path = tab
                                .navigation
                                .path()
                                .map(Path::to_owned)
                                .or_else(|| sidebar.resolve_path(location));
                            (tab.id, location.clone(), path, tab.pinned)
                        })
                        .collect::<Vec<_>>()
                })
            });
            let tab_rows = for_each(move || tabs.get(), |tab| tab.clone(), {
                let browser = browser.clone();
                move |(id, location, path, pinned)| {
                    browser.sidebar_tab(id, location, path, pinned, presentation)
                }
            });
            let locations = LocationTarget::ALL
                .into_iter()
                .filter(|target| {
                    !matches!(target, LocationTarget::Collection(Collection::Tags)) || show_tags
                })
                .map(|target| {
                    let destination = target.location(&home);
                    browser.sidebar_location(target, destination, presentation)
                })
                .collect::<Vec<_>>();
            let location_height = (SIDEBAR_ITEM_HEIGHT + SIDEBAR_ROW_GAP) * locations.len() as f32;
            let active_volume = memo({
                let browser = browser.clone();
                move || {
                    browser.read(|state| {
                        let navigation = &state.tab().navigation;
                        match navigation.location() {
                            Location::Volume { identity, .. } => {
                                browser.volume_at_location(identity).map(|volume| volume.id)
                            }
                            _ => navigation.path().and_then(|path| {
                                browser
                                    .0
                                    .volumes
                                    .snapshot()
                                    .at_path(path)
                                    .map(|volume| volume.id.clone())
                            }),
                        }
                    })
                }
            });
            let volume_list = browser.clone();
            let default_volumes_height = memo({
                let volumes = browser.0.volumes.clone();
                move || {
                    let content_height: f32 = volumes
                        .snapshot()
                        .volumes
                        .iter()
                        .filter(|volume| volume.visible)
                        .map(|volume| {
                            let details = if volume.mount_points.is_empty() {
                                UNMOUNTED_STATUS_HEIGHT + SIDEBAR_ROW_GAP
                            } else {
                                CapacityBar::HEIGHT + CAPACITY_BOTTOM_PADDING
                            };
                            SIDEBAR_ITEM_HEIGHT + details + SIDEBAR_ROW_GAP
                        })
                        .sum();
                    SideBarSectionHeader::HEIGHT + content_height
                }
            });
            let volume_rows = dyn_view(move || {
                let snapshot = volume_list.0.volumes.snapshot();
                let rows = snapshot
                    .volumes
                    .iter()
                    .filter(|volume| volume.visible)
                    .map(|volume| {
                        volume_list.sidebar_volume(volume, &snapshot, active_volume, presentation)
                    })
                    .collect::<Vec<_>>();
                if rows.is_empty() {
                    el! { .div.{px: 6.0_f32; min_w: 0.0_f32; overflow_hidden;
                        dyn_style: move style => $(style.opacity(presentation.progress()).text_color(theme().text_muted));
                    }.[ $(i18n::text("device-none")).text.{text_size: 12.0_f32; ellipsis; whitespace_nowrap} ] }.into_view()
                } else {
                    fragment(rows)
                }
            });
            let new_tab = browser.clone();
            let locations_toggle = browser.clone();
            let volumes_toggle = browser.clone();
            let height = browser.0.sidebar_tabs_height;
            let region = browser.0.sidebar_region;
            let resize = browser.clone();
            let resize_keys = browser.clone();
            let drag_height = std::cell::Cell::new((None, 0.0_f32));
            let available = signal(0.0_f32);
            let header = SideBarSectionHeader::HEIGHT;
            let divider_height = gds::ui::split::DIVIDER_HEIGHT;
            let clamp_tabs = move |preferred: Option<f32>| {
                let usable = (available.get() - divider_height).max(0.0_f32);
                let minimum = MIN_TABS_HEIGHT.min(usable / 2.0_f32);
                let maximum = (usable - MIN_VOLUMES_HEIGHT).max(minimum);
                preferred
                    .unwrap_or_else(|| usable - default_volumes_height.get())
                    .clamp(minimum, maximum)
            };
            let minimum = move || {
                MIN_TABS_HEIGHT
                    + SIDEBAR_GROUP_GAP
                    + if show_volumes {
                        header
                            + (MIN_VOLUMES_HEIGHT + divider_height - header) * volume_progress.get()
                    } else {
                        0.0_f32
                    }
            };
            el! { .SideBar::default.{
                width_binding: $(presentation.width_binding());
                pr: 1.0_f32;
            }.[
                .SideBarContent::default.{px: SIDEBAR_PADDING; pt: SIDEBAR_GROUP_GAP; gap: 0.0_f32; overflow_hidden;
                    dyn_style: move style => $(style.min_h(minimum()));
                    on_resized: move (_, height) => $(available.set_if_changed((height - SIDEBAR_GROUP_GAP).max(0.0_f32)));
                }.[
                    .SideBarSection::default.{gap: 0.0_f32; overflow_hidden;
                        dyn_style: move style => ${
                            if !show_volumes { return style.flex_1(); }
                            let expanded = clamp_tabs(height.get());
                            let collapsed = (available.get() - header).max(0.0_f32);
                            style.flex_none().h(collapsed + (expanded - collapsed) * volume_progress.get())
                        };
                    }.[
                        .SideBarSectionHeader::default.{
                            overflow_hidden;
                            dyn_style: move style => $(style.pr(2.5_f32 * (1.0_f32 - presentation.progress())));
                        }.[
                            .SideBarSectionTitle::default.{overflow_hidden;
                                dyn_style: move style => $(style.opacity(presentation.progress()).px(6.0_f32 * presentation.progress()));
                            }.[ (move () => $(i18n::text("tabs"))).text.{whitespace_nowrap} ],
                            "new-tab".IconButton::new.{icon: Icon::Add; label: $(i18n::text("new-tab"));
                                keytip: $(crate::shared::ui::actions::Command::NewTab.default_stroke());
                                on_click: move _ => ${new_tab.new_tab(None); new_tab.focus_content_after_mount();};
                            },
                        ],
                        $(browser.sidebar_region(SidebarRegion::Tabs, "alt-t")).{flex_1}.[tab_rows],
                    ],
                    if show_volumes [
                        $(gds::ui::split::handle(
                            "sidebar-resize",
                            move |event| {
                                match event.phase {
                                    GrabPhase::Start => {
                                        let preferred = height.get_untracked();
                                        drag_height.set((preferred, clamp_tabs(preferred)));
                                    }
                                    GrabPhase::Move => height.set(Some(clamp_tabs(Some(drag_height.get().1 + event.total.y)))),
                                    GrabPhase::Cancel => height.set(drag_height.get().0),
                                    GrabPhase::End => resize.persist_sidebar_height(),
                                }
                            },
                            move |step| {
                                height.set(Some(clamp_tabs(Some(clamp_tabs(height.get_untracked()) + step))));
                                resize_keys.persist_sidebar_height();
                            },
                        )).{dyn_style: move style => $(if volume_progress.get() > 0.0 {
                            style.flex().h(volume_progress.get() * divider_height)
                        } else {style.hidden()})},
                    ],
                    if show_volumes [
                        .SideBarSection::default.{flex_1; min_h: 0.0_f32; overflow_hidden;
                        }.[
                            $(browser.sidebar_heading("volumes", volume_progress, presentation, move || volumes_toggle.edit_preferences(|preferences| {
                                preferences.files.sidebar.volumes_collapsed = !preferences.files.sidebar.volumes_collapsed;
                            }))),
                            $(browser.sidebar_region(SidebarRegion::Volumes, "alt-v")).{flex_1; pb: SIDEBAR_ROW_GAP;
                                dyn_style: move style => $(if volume_progress.get() > 0.0 || region.get() == Some(SidebarRegion::Volumes) {style.flex()} else {style.hidden()});
                            }.[volume_rows],
                        ],
                    ],
                ],
                .SideBarFooter::default.{p: 0.0_f32; flex_shrink: 1.0_f32; min_h: $(header + SIDEBAR_PADDING); gap: 0.0_f32; overflow_hidden;
                    dyn_style: move style => $(style.h(header + SIDEBAR_PADDING + location_height * location_progress.get()));
                }.[
                    .SideBarSection::default.{px: SIDEBAR_PADDING; pb: SIDEBAR_PADDING; flex_1; min_h: 0.0_f32; overflow_hidden;
                        dyn_style: move style => $(style.gap(SIDEBAR_ROW_GAP * location_progress.get()));
                    }.[
                        $(browser.sidebar_heading("locations", location_progress, presentation, move || locations_toggle.edit_preferences(|preferences| {
                            preferences.files.sidebar.locations_collapsed = !preferences.files.sidebar.locations_collapsed;
                        }))),
                        $(browser.sidebar_region(SidebarRegion::Locations, "alt-l")).{flex_shrink: 1.0_f32;
                            dyn_style: move style => $(if location_progress.get() > 0.0 || region.get() == Some(SidebarRegion::Locations) {
                                style.flex().h((location_height - SIDEBAR_ROW_GAP).max(0.0_f32) * location_progress.get())
                            } else {style.hidden()});
                        }.[..locations],
                    ],
                ],
            ] }
        })
    }

    fn persist_sidebar_height(&self) {
        let height = self.0.sidebar_tabs_height.get_untracked();
        self.edit_preferences(|preferences| preferences.files.sidebar.tabs_height = height);
    }

    fn sidebar_region(&self, region: SidebarRegion, shortcut: &'static str) -> Div {
        let node = self.sidebar_node(region);
        let focus = self.clone();
        let keys = self.clone();
        el! { $(gds::ui::region::surface(node, shortcut)).{
            flex; flex_col; gap: SIDEBAR_ROW_GAP; overflow_y_scroll;
            on_focus_change: move focused => $(focus.sidebar_focus_changed(region, focused));
            on_key_down: move event => $(keys.sidebar_key(event, region));
        } }
    }

    fn sidebar_volume(
        &self,
        volume: &volumes::Volume,
        snapshot: &volumes::Snapshot,
        active: Memo<Option<volumes::VolumeId>>,
        presentation: SidebarPresentation,
    ) -> Div {
        let icon = if volume.locked {
            Icon::Lock
        } else {
            match volume
                .drive
                .as_deref()
                .and_then(|id| snapshot.drive(id))
                .map(|drive| drive.kind)
            {
                Some(volumes::DeviceKind::Usb) => Icon::Usb,
                Some(volumes::DeviceKind::Optical) => Icon::Disc,
                _ => Icon::Drive,
            }
        };
        let action = self.clone();
        let device = volume.id.clone();
        let candidate = SidebarRow::Volume(volume.id.clone());
        let cursor = self.0.sidebar_cursor;
        let label = volume.name.clone();
        let name = volume.name.clone();
        let selected = memo({
            let id = volume.id.clone();
            move || active.with(|active| active.as_ref() == Some(&id))
        });
        let status = self.0.volumes.capacity(&volume.id);
        let volume_busy = self.0.volumes.is_busy(&volume.id);
        let mut hint = vec![volume.name.clone(), volume.filesystem.clone()];
        if volume.system_mount {
            hint.push(i18n::text("device-system-volume"));
        }
        let details = if volume.mount_points.is_empty() {
            el! { .div.{px: 6.0_f32; pb: SIDEBAR_ROW_GAP; min_w: 0.0_f32; overflow_hidden;
                dyn_style: move style => $(style.opacity(presentation.progress()).text_color(theme().text_muted));
            }.[ $(i18n::text("device-not-mounted")).text.{h: UNMOUNTED_STATUS_HEIGHT; text_size: 12.0_f32; ellipsis; whitespace_nowrap} ] }.into_view()
        } else {
            let (summary, status) = match status {
                Some(volumes::CapacityState::Ready(capacity)) => {
                    let available = EntryPresentation::format_size(capacity.available);
                    let total = EntryPresentation::format_size(capacity.total);
                    (
                        gds::t!("device-capacity", available = available, total = total),
                        CapacityStatus::Used(capacity.unavailable_fraction()),
                    )
                }
                Some(volumes::CapacityState::Unavailable(error)) => {
                    hint.push(error);
                    (
                        i18n::text("device-capacity-unavailable"),
                        CapacityStatus::Unavailable,
                    )
                }
                _ => (
                    i18n::text("device-capacity-loading"),
                    CapacityStatus::Loading,
                ),
            };
            hint.push(summary.clone());
            let width = presentation.width_binding();
            let compact = Binding::new(move || width.get() < 120.0, |_| {});
            let selected = Binding::new(move || selected.get(), |_| {});
            el! { .div.{px: 6.0_f32; pb: CAPACITY_BOTTOM_PADDING; min_w: 0.0_f32; overflow_hidden}.[
                (summary, status).CapacityBar::new.{compact: compact; selected: selected},
            ] }
            .into_view()
        };
        let row = el! { .div.{flex; flex_col; flex_none; min_w: 0.0_f32;
            rounded: 6.0_f32; cursor: CursorIcon::Pointer; focusable; overflow_hidden;
            access: move node => $(node.role(Role::Button).label(label.clone()).selected(selected.get()));
            dyn_style: move style => ${
                let colors = gds::app::Gds::current().appearance().theme().colors();
                style.bg(if selected.get() {colors.selection} else {Color::TRANSPARENT})
                    .text_color(if selected.get() {colors.selection_foreground} else {colors.sidebar_foreground})
                    .outline(if cursor.with(|row| row.as_ref() == Some(&candidate)) {1.0_f32} else {0.0_f32})
                    .outline_offset(-1.0_f32).outline_color(theme().focus_ring)
            };
            hover: style => $(style.bg(theme().hover));
            on_click: move event => ${action.open_volume(&device, event.modifiers.primary()); action.focus_content_after_mount();};
        }.[
            .div.{flex; items_center; h: SIDEBAR_ITEM_HEIGHT; flex_none; px: SIDEBAR_PADDING; gap: 10.0_f32}.[
                $(icon.view(18.0_f32)),
                .div.{min_w: 0.0_f32; flex_1; overflow_hidden;
                    dyn_style: move style => $(style.opacity(presentation.progress()));
                }.[name.text.{ellipsis; min_w: 0.0_f32; w_full; whitespace_nowrap}],
                if volume_busy [ .spinner.{size: 16.0_f32} ],
            ],
            details,
        ] };
        self.sidebar_volume_menu(gds::ui::tooltip::tooltip(row, hint.join("\n")), volume)
    }

    fn sidebar_heading(
        &self,
        label: &'static str,
        expanded: Animated<f32>,
        presentation: SidebarPresentation,
        toggle: impl Fn() + 'static,
    ) -> SideBarSectionHeader {
        el! { .SideBarSectionHeader::default.[
            .div.{flex; flex_1; items_center; h_full; min_w: 0.0_f32; gap: 6.0_f32; rounded: 4.0_f32;
                cursor: CursorIcon::Pointer; focusable; overflow_hidden;
                dyn_style: move style => $(style.px(12.5_f32 - 4.5_f32 * presentation.progress()));
                access: move node => $(node.role(Role::Button).label(i18n::text(label)).expanded(expanded.get() > 0.0));
                hover: style => $(style.bg(theme().hover));
                on_click: move _ => $(toggle());
            }.[
                $(Icon::ChevronRight.view(12.0)).{dyn_style: move style => $(style.rotate(90.0_f32 * expanded.get()))},
                .SideBarSectionTitle::default.{overflow_hidden;
                    dyn_style: move style => $(style.opacity(presentation.progress()));
                }.[ (move () => $(i18n::text(label))).text.{whitespace_nowrap} ],
            ],
        ] }
    }

    fn sidebar_location(
        &self,
        target: LocationTarget,
        location: Location,
        presentation: SidebarPresentation,
    ) -> Div {
        let label = i18n::text(target.label_id());
        let icon = target.icon();
        let cursor = self.0.sidebar_cursor;
        let open = self.clone();
        let selected = memo({
            let browser = self.clone();
            let location = location.clone();
            move || {
                browser.read(|state| {
                    let navigation = &state.tab().navigation;
                    navigation.location() == &location
                        || location
                            .local_path()
                            .is_some_and(|path| navigation.path() == Some(path))
                })
            }
        });
        let accessible = label.clone();
        let tooltip = label.clone();
        let target = el! { .div.{flex; items_center; flex_none; h: SIDEBAR_ITEM_HEIGHT; px: SIDEBAR_PADDING; gap: 10.0_f32;
            rounded: 6.0_f32; cursor: CursorIcon::Pointer; focusable; overflow_hidden;
            access: move node => $(node.role(Role::Button).label(accessible.clone()).selected(selected.get()));
            dyn_style: move style => ${
                let colors = gds::app::Gds::current().appearance().theme().colors();
                style.bg(if selected.get() {colors.selection} else {Color::TRANSPARENT})
                    .text_color(if selected.get() {colors.selection_foreground} else {colors.sidebar_foreground})
                    .outline(if cursor.with(|row| row.as_ref() == Some(&SidebarRow::Location(target))) {1.0_f32} else {0.0_f32})
                    .outline_offset(-1.0_f32).outline_color(theme().focus_ring)
            };
            hover: style => $(style.bg(theme().hover));
            on_click: move _ => ${open.open(location.clone()); open.focus_content_after_mount();};
        }.[
            $(icon.view(18.0_f32)),
            .div.{flex_1; min_w: 0.0_f32; overflow_hidden;
                dyn_style: move style => $(style.opacity(presentation.progress()));
            }.[label.text.{ellipsis; min_w: 0.0_f32; w_full}],
        ] };
        gds::ui::tooltip::tooltip(target, tooltip)
    }

    fn sidebar_tab(
        &self,
        id: u64,
        location: Location,
        local_path: Option<PathBuf>,
        pinned: bool,
        presentation: SidebarPresentation,
    ) -> Div {
        let title = memo({
            let browser = self.clone();
            let location = location.clone();
            move || browser.location_title(&location)
        });
        let path = Self::location_address(&location, local_path.as_deref());
        let tooltip = path.clone();
        let select = self.clone();
        let action = self.clone();
        let hovered = signal(false);
        let cursor = self.0.sidebar_cursor;
        let selected = memo({
            let browser = self.clone();
            move || browser.read(|state| state.tab().id == id)
        });
        let selection = follow(
            move || if selected.get() { 1.0_f32 } else { 0.0_f32 },
            super::view::WORKSPACE_MOTION,
        );
        let target = el! { .div.{relative; flex; items_center; flex_none; h: 44.0_f32; px: 10.0_f32; gap: 10.0_f32;
            rounded: 6.0_f32; cursor: CursorIcon::Pointer; focusable; overflow_hidden;
            access: move node => $(node.role(Role::Button).label(title.get()).selected(selected.get()));
            dyn_style: move style => ${
                let gds = gds::app::Gds::current();
                let colors = gds.appearance().theme().colors();
                let amount = selection.get();
                let background = if amount == 0.0 && hovered.get() {colors.sidebar_hover()}
                    else {colors.selection.with_alpha(colors.selection.a * amount)};
                style.bg(background).h(gds.appearance().typography().scaled(44.0))
                    .text_color(colors.sidebar_foreground.mix(colors.selection_foreground, amount))
                    .outline(if cursor.with(|row| row.as_ref() == Some(&SidebarRow::Tab(id))) {1.0_f32} else {0.0_f32})
                    .outline_offset(-1.0_f32).outline_color(theme().focus_ring)
            };
            on_hover: move inside => $(hovered.set(inside));
            on_click: move _ => $(if let Some(index) = select.tab_index(id) {
                select.select_tab(index); select.focus_content_after_mount();
            });
        }.[
            $(location.icon().view(18.0_f32)),
            .div.{flex; flex_col; flex_1; min_w: 0.0_f32; overflow_hidden;
                dyn_style: move style => $(style.opacity(presentation.progress()).pr(28.0_f32 * presentation.progress()));
            }.[ $(move || title.get()).text.{ellipsis; min_w: 0.0_f32; w_full; whitespace_nowrap},
                path.text.{ellipsis; min_w: 0.0_f32; w_full; whitespace_nowrap;
                    dyn_style: move style => ${
                        let gds = gds::app::Gds::current();
                        let colors = gds.appearance().theme().colors();
                        style.text_color(colors.muted_foreground.mix(colors.selection_foreground, selection.get()))
                            .text_size(gds.appearance().typography().scaled(12.0))
                    };
                }
            ],
            .div.{absolute; flex; items_center; justify_center; cursor: CursorIcon::Pointer; focusable; rounded: 4.0_f32;
                access: move node => $(node.role(Role::Button).label(i18n::text(if pinned {"unpin-tab"} else {"close-this-tab"})));
                dyn_style: move style => ${
                    if !pinned && !hovered.get() { return style.hidden(); }
                    if presentation.actions_visible() {style.flex().size(32.0_f32).top(6.0_f32).right(4.0_f32)}
                    else if presentation.progress() == 0.0 {style.flex().size(16.0_f32).top(1.0_f32).right(1.0_f32)}
                    else {style.hidden()}
                };
                hover: style => $(style.bg(theme().hover));
                on_pointer_down: event => $(if event.button == Some(PointerButton::Primary) {event.stop_propagation();});
                on_click: move event => ${
                    event.stop_propagation();
                    if let Some(index) = action.tab_index(id) {
                        if pinned {action.toggle_pin(index); action.focus_content_after_mount();} else {action.close_tab(index);}
                    }
                };
            }.[
                $((if pinned {Icon::Pin} else {Icon::Close}).view(12.0_f32)),
            ],
        ] };
        let target = gds::ui::tooltip::tooltip(target, tooltip);
        let target = match local_path {
            Some(path) => self.file_drag_target(target, path),
            None => target,
        };
        let target = self.draggable_tab(target, id, pinned);
        self.tab_menu(target, id)
    }
}
