//! The footer's task, selection, and editable clipboard summaries.
use super::{
    menus::Surface,
    model::{Operation, Row},
    ui::Browser,
};
use crate::shared::lib::clipboard::{Clipboard, Mode, Snapshot};
use anti_webui::prelude::*;
use gds::{
    i18n,
    ui::{Icon, IconButton},
};
use gds_uim::el;
use std::{path::PathBuf, rc::Rc, sync::Arc};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Tasks,
    Selection,
    Clipboard,
}

impl Kind {
    fn title(self) -> &'static str {
        match self {
            Self::Tasks => "tasks-title",
            Self::Selection => "selection-title",
            Self::Clipboard => "clipboard-title",
        }
    }
    fn empty(self) -> &'static str {
        match self {
            Self::Tasks => "tasks-empty",
            Self::Selection => "selection-empty",
            Self::Clipboard => "clipboard-empty",
        }
    }
}

struct Summary {
    icon: Icon,
    count: usize,
    label: String,
}

enum Entry {
    Task {
        label: String,
        cancel: CancellationToken,
    },
    Selection {
        tab: u64,
        row: Row,
    },
    Clipboard {
        expected: Arc<Snapshot>,
        path: PathBuf,
    },
}

impl Entry {
    fn title(&self) -> String {
        match self {
            Self::Task { label, .. } => label.clone(),
            Self::Selection { row, .. } => match row {
                Row::File(entry) => entry.path.to_string_lossy().into_owned(),
                Row::Trash(entry) => entry.original_path.to_string_lossy().into_owned(),
                Row::Place(location) => location.display_address(),
                Row::Remote(entry) => entry.address.to_string(),
                Row::Tag(tag) => tag.clone(),
                Row::Volume(volume) => volume.name.clone(),
            },
            Self::Clipboard { path, .. } => path.to_string_lossy().into_owned(),
        }
    }
}

impl Browser {
    pub(super) fn status_controls(&self) -> Div {
        el! { .div.{flex; items_center; gap: 2.0_f32; min_w: 0.0_f32}.[
            $(self.status_control(Kind::Tasks)),
            $(self.status_control(Kind::Selection)),
            $(self.status_control(Kind::Clipboard)),
        ] }
    }

    fn status_summary(&self, kind: Kind) -> Summary {
        match kind {
            Kind::Tasks => self.read(|state| match &state.operation {
                Operation::Idle => Summary {
                    icon: Icon::Tasks,
                    count: 0,
                    label: String::new(),
                },
                Operation::Running { label, .. } => Summary {
                    icon: Icon::Tasks,
                    count: 1,
                    label: label.clone(),
                },
            }),
            Kind::Selection => self.read(|state| {
                let count = state.tab().navigation.view().selection.len();
                Summary {
                    icon: Icon::Check,
                    count,
                    label: if count == 0 {
                        String::new()
                    } else {
                        i18n::count("selection-count", count)
                    },
                }
            }),
            Kind::Clipboard => {
                let snapshot = Clipboard::current().snapshot();
                let count = snapshot.as_ref().map_or(0, |snapshot| snapshot.paths.len());
                let icon = if snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.mode == Mode::Cut)
                {
                    Icon::Cut
                } else {
                    Icon::Copy
                };
                let label = snapshot.as_ref().map_or_else(String::new, |snapshot| {
                    if let [path] = snapshot.paths.as_slice() {
                        path.file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        i18n::count("item-count", count)
                    }
                });
                Summary { icon, count, label }
            }
        }
    }

    fn status_control(&self, kind: Kind) -> Div {
        let summary = self.clone();
        let open = self.clone();
        let shown = self.clone();
        let content = self.clone();
        let anchor = node_ref();
        let fade = {
            let browser = self.clone();
            follow(
                move || {
                    if matches!(browser.0.menus.surface.get(), Surface::Status(current) if current == kind)
                    {
                        1.0_f32
                    } else {
                        0.0_f32
                    }
                },
                Transition::ms(140).ease_out(),
            )
        };
        let control = el! { .div.{relative; min_w: 0.0_f32}.[
            .div.{
                node_ref: anchor; flex; items_center; min_w: 0.0_f32; h: gds::appearance::tokens::ICON_BUTTON_SIZE;
                rounded: 6.0_f32; cursor: CursorIcon::Pointer; focusable;
                access: move a => $(a.role(Role::Button).label(i18n::text(kind.title())));
                hover: style => $(style.bg(theme().hover));
                sheet: () => $(Sheet::new(|style| style).focus_visible(|style| style.outline(1.0_f32).outline_color(theme().focus_ring)).into());
                on_click: move _ => $(open.0.menus.surface.update(|surface| {
                    *surface = if matches!(surface, Surface::Status(current) if *current == kind) {
                        Surface::Closed
                    } else { Surface::Status(kind) };
                    if kind == Kind::Clipboard { Clipboard::current().refresh(); }
                }));
            }.[
                $(move || {
                    let Summary { icon, count, label } = summary.status_summary(kind);
                    let compact = summary.compact();
                    el! { .div.{flex; items_center; min_w: 0.0_f32}.[
                        .div.{relative; size: gds::appearance::tokens::ICON_BUTTON_SIZE; flex_none; flex; items_center; justify_center}.[
                            $(icon.view(gds::appearance::tokens::ICON_SIZE)),
                            if $(compact && count > 0) [
                                .div.{absolute; top: 0.0_f32; right: 0.0_f32; min_w: 12.0_f32; h: 12.0_f32;
                                    rounded: 999.0_f32; px: 2.0_f32; text_size: 9.0_f32; flex; items_center; justify_center;
                                    dyn_style: style => $(style.bg(theme().accent).text_color(theme().on_accent));
                                }.[ $(count.to_string()).text ],
                            ],
                        ],
                        if $(!compact && !label.is_empty()) [
                            label.text.{max_w: 240.0_f32; min_w: 0.0_f32; overflow_hidden; ellipsis;
                                text_size: 12.0_f32; mr: 8.0_f32},
                        ],
                    ] }
                }).dyn_view,
            ],
            $(show(
                move || matches!(shown.0.menus.surface.get(), Surface::Status(current) if current == kind),
                move || {
                    let close = content.clone();
                    let escape = content.clone();
                    let details = content.clone();
                    let focus = node_ref();
                    let view = el! { .popup.{anchor: anchor; placement: Placement::AboveStart; modal;
                        on_dismiss: move () => $(close.0.menus.close());
                    }.[
                        .div.{flex; flex_col; w: 320.0_f32; max_h: 320.0_f32; p: 4.0_f32; rounded: 8.0_f32;
                            node_ref: focus; focusable; select_none;
                            border: 1.0_f32; overflow_y_scroll;
                            dyn_style: move style => $(style.bg(theme().overlay).text_color(theme().text).border_color(theme().border).opacity(fade.get()));
                            on_pointer_down: event => $(event.stop_propagation());
                            on_click: event => $(event.stop_propagation());
                            on_key_down: move event => ${
                                event.stop_propagation();
                                if event.key == Key::Named(NamedKey::Escape) {
                                    event.prevent_default();
                                    escape.0.menus.close();
                                }
                            };
                        }.[ $(move || details.status_details(kind)).dyn_view ],
                    ] }.into_view();
                    focus.focus();
                    view
                },
            )),
        ] };
        el! { .div.{flex; min_w: 0.0_f32}.[
            $(tooltip_with(control, TooltipOptions::new(), move |_| move || {
                el! { .div.{px: 8.0_f32; py: 4.0_f32; rounded: 4.0_f32; text_size: 12.0_f32;
                    dyn_style: style => $(style.bg(theme().tooltip).text_color(theme().on_tooltip));
                }.[ $(move || i18n::text(kind.title())).text ] }
            })),
        ] }
    }

    fn status_details(&self, kind: Kind) -> Div {
        let rows: Vec<Entry> = match kind {
            Kind::Tasks => {
                let task = self.read(|state| match &state.operation {
                    Operation::Idle => None,
                    Operation::Running { label, cancel, .. } => {
                        Some((label.clone(), cancel.clone()))
                    }
                });
                task.into_iter()
                    .map(|(label, cancel)| Entry::Task { label, cancel })
                    .collect()
            }
            Kind::Selection => {
                let (tab, rows) = self.read(|state| (state.tab().id, state.tab().selected()));
                rows.into_iter()
                    .map(|row| Entry::Selection { tab, row })
                    .collect()
            }
            Kind::Clipboard => Clipboard::current()
                .snapshot()
                .map_or_else(Vec::new, |snapshot| {
                    snapshot
                        .paths
                        .iter()
                        .cloned()
                        .map(|path| Entry::Clipboard {
                            expected: snapshot.clone(),
                            path,
                        })
                        .collect()
                }),
        };
        if rows.is_empty() {
            return el! { .div.{p: 12.0_f32; text_size: 12.0_f32; text_color: $(theme().text_muted)}.[
                $(i18n::text(kind.empty())).text,
            ] };
        }
        let rows = Rc::new(rows);
        let count = rows.len();
        let browser = self.clone();
        let list = virtual_list(
            move || count,
            32.0,
            move |index| browser.status_entry(&rows[index]),
        );
        el! { .div.{h: $((count as f32 * 32.0).min(312.0)); w_full; min_w: 0.0_f32}.[
            list.{h_full; w_full},
        ] }
    }

    fn status_entry(&self, entry: &Entry) -> Div {
        let title = entry.title();
        match entry {
            Entry::Task { label, cancel } => {
                let browser = self.clone();
                let cancel = cancel.clone();
                let cancelled = cancel.is_cancelled();
                let label = if cancelled {
                    format!("{label} — {}", i18n::text("tasks-cancelling"))
                } else {
                    label.clone()
                };
                self.status_row(Icon::Tasks, label, title, el! {
                        "cancel-task".IconButton::new.{icon: Icon::Close; label: $(i18n::text("operation-cancel")); disabled: cancelled;
                            on_click: move _ => ${ cancel.cancel(); browser.changed(); };
                        }
                    })
            }
            Entry::Selection { tab, row } => {
                let tab = *tab;
                let browser = self.clone();
                let key = row.key();
                self.status_row(row.icon(), row.label(), title, el! {
                        "deselect-entry".IconButton::new.{icon: Icon::Close; label: $(i18n::text("selection-remove-item"));
                            on_click: move _ => $(browser.update_selection(|state| {
                                if state.tab().id != tab { return; }
                                let visit = state.tab_mut().navigation.view_mut();
                                visit.selection.remove(&key);
                                if visit.anchor.as_ref() == Some(&key) { visit.anchor = None; }
                                if visit.cursor.as_ref() == Some(&key) { visit.cursor = None; }
                            }));
                        }
                    })
            }
            Entry::Clipboard { expected, path } => {
                let path = path.clone();
                let label = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                let expected = expected.clone();
                let browser = self.clone();
                self.status_row(Icon::File, label, title, el! {
                        "remove-clipboard-entry".IconButton::new.{icon: Icon::Close; label: $(i18n::text("clipboard-remove-item"));
                            on_click: move _ => ${
                                let notice = browser.clone();
                                browser.0.owner.with(|| Clipboard::current().remove(expected.clone(), vec![path.clone()])
                                    .then(move |result| {
                                        if let Err(error) = result { notice.report_notice(error.to_string()); }
                                    }));
                            };
                        }
                    })
            }
        }
    }

    fn status_row(&self, icon: Icon, label: String, title: String, action: impl IntoView) -> Div {
        let row = el! { .div.{flex; items_center; gap: 8.0_f32; min_w: 0.0_f32; px: 8.0_f32; h: 32.0_f32; rounded: 4.0_f32;
            hover: style => $(style.bg(theme().hover));
        }.[
            $(icon.view(16.0)),
            label.text.{flex_1; min_w: 0.0_f32; overflow_hidden; ellipsis; whitespace_nowrap},
            action,
        ] };
        tooltip(row, title)
    }
}
