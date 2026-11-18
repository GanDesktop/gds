//! Native tab reordering and file payloads retain the existing copy/move policy.
use super::{
    geometry::Grid,
    model::{Row, RowKey},
    ui::Browser,
};
use crate::{
    entities::file::{
        directory::Entry,
        ui::{FileArtwork, FileIcon},
    },
    features::file_operations::{paste, transfer},
};
use anti_webui::prelude::*;
use gds::{
    i18n,
    ui::{
        Icon,
        drag::{DragCard, DragCardContent, DragCardFooter, DragCardHeader},
    },
};
use gds_uim::el;
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

const TAB_SPRING_DELAY: Duration = Duration::from_millis(600);
const PREVIEW_ITEMS: usize = 3;

#[derive(Clone)]
struct TabDrag {
    window: usize,
    id: u64,
    pinned: bool,
}
#[derive(Clone)]
pub(super) struct FileDrag {
    window: usize,
    paths: Rc<[PathBuf]>,
    preview: Rc<[Entry]>,
    grid: Option<Grid>,
}

impl FileDrag {
    pub(super) fn is_source(path: &Path) -> bool {
        dragged::<Self>().is_some_and(|drag| drag.paths.iter().any(|source| source == path))
    }

    fn preview(&self) -> Div {
        let count = self.paths.len();
        let Some(first) = self.preview.first() else {
            return div();
        };
        if count == 1 {
            if let Some(grid) = self.grid {
                let icon = FileArtwork::default()
                    .icon(FileIcon::for_entry(first))
                    .color(FileIcon::category_color(FileIcon::is_directory(first)))
                    .size(grid.icon_size);
                return el! { .div.{flex; flex_col; items_center; w: grid->width; h: grid->tile_height;
                    p: 8.0_f32; gap: 8.0_f32; rounded: 8.0_f32;
                    dyn_style: style => $(style.bg(theme().overlay).text_color(theme().text));
                }.[
                    .div.{flex; flex_none; items_center; justify_center; h: grid->artwork_height}.[icon],
                    $(first.display_name.clone()).text.{w_full; text_align: TextAlign::Center; text_size: grid->name_size;
                        line_height: $(LineHeight::Px(grid.line_height)); line_clamp: 2;
                    },
                ] };
            }
            return el! { .div.{flex; items_center; h: 40.0_f32; max_w: 320.0_f32; px: 12.0_f32; rounded: 6.0_f32;
                dyn_style: style => $(style.bg(theme().overlay).text_color(theme().text));
            }.[ $(Self::item(first)) ] };
        }
        let rows = self.preview.iter().map(Self::item).collect::<Vec<_>>();
        let remaining = count.saturating_sub(PREVIEW_ITEMS);
        el! { .div.{pl: 12.0_f32; pt: 12.0_f32}.[
            .DragCard::default.[
                .DragCardHeader::default.[
                    $(Icon::Files.view(16.0)).{dyn_style: style => $(style.text_color(theme().accent))},
                    $(i18n::count("item-count", count)).text.{ellipsis},
                ],
                .DragCardContent::default.[..rows],
                if $(remaining > 0) [
                    .DragCardFooter::default.[ $(i18n::count("operation-more-items", remaining)).text.{ellipsis} ],
                ],
            ],
        ] }
    }

    fn item(entry: &Entry) -> Div {
        let icon = FileArtwork::default()
            .icon(FileIcon::for_entry(entry))
            .color(FileIcon::category_color(FileIcon::is_directory(entry)));
        el! { .div.{flex; items_center; gap: 8.0_f32; min_w: 0.0_f32; overflow_hidden}.[
            icon,
            $(entry.display_name.clone()).text.{min_w: 0.0_f32; ellipsis; whitespace_nowrap},
        ] }
    }
}
impl Browser {
    pub(super) fn background_drop_target(&self, target: Div) -> Div {
        let internal = self.clone();
        let external = self.clone();
        target
            .drop_target(move |drag: &FileDrag, event| {
                let destination =
                    internal.read(|state| state.tab().navigation.path().map(PathBuf::from));
                if let Some(destination) = destination {
                    internal.transfer_dropped(
                        destination,
                        drag.paths.to_vec(),
                        if event.modifiers.copies_drag() {
                            transfer::Mode::Copy
                        } else {
                            transfer::Mode::Move
                        },
                    );
                }
            })
            .on_drop_files(move |paths, _| {
                let destination =
                    external.read(|state| state.tab().navigation.path().map(PathBuf::from));
                if let Some(destination) = destination {
                    external.transfer_dropped(destination, paths.to_vec(), transfer::Mode::Copy);
                }
            })
    }
    pub(super) fn draggable_tab(&self, target: Div, id: u64, pinned: bool) -> Div {
        let window = Rc::as_ptr(&self.0) as usize;
        let browser = self.clone();
        let close = self.clone();
        let hover = Cell::new(None::<Timeout>);
        let spring = self.clone();
        let owner = Owner::current().expect("tab drag target belongs to a view");
        target
            .draggable(move || TabDrag { window, id, pinned })
            .drop_accepts(move |drag: &TabDrag| {
                drag.window == window && drag.id != id && drag.pinned == pinned
            })
            .drop_target(move |drag: &TabDrag, event| {
                if drag.window != window || drag.id == id || drag.pinned != pinned {
                    return;
                }
                let pair = browser.read(|state| {
                    Some((
                        state.tabs.iter().position(|tab| tab.id == drag.id)?,
                        state.tabs.iter().position(|tab| tab.id == id)?,
                    ))
                });
                if let Some((from, to)) = pair {
                    let after = event.fraction().y >= 0.5;
                    let boundary = to + usize::from(after);
                    let destination = boundary.saturating_sub(usize::from(from < boundary));
                    browser.reorder_tab(from, destination);
                }
            })
            .on_aux_click(move |event| {
                if !pinned
                    && event.button == Some(PointerButton::Middle)
                    && let Some(index) =
                        close.read(|state| state.tabs.iter().position(|tab| tab.id == id))
                {
                    close.close_tab(index);
                }
            })
            .on_drag_hover(move |inside| {
                if let Some(timer) = hover.take() {
                    timer.cancel();
                }
                if !inside || spring.read(|state| state.tab().id == id) {
                    return;
                }
                let Some(drag) = dragged::<FileDrag>().filter(|drag| drag.window == window) else {
                    return;
                };
                let weak = spring.downgrade();
                hover.set(Some(owner.with(|| {
                    set_timeout(TAB_SPRING_DELAY, move || {
                        if window_active()
                            && dragged::<FileDrag>()
                                .is_some_and(|active| Rc::ptr_eq(&active.paths, &drag.paths))
                            && let Some(browser) = weak.upgrade()
                            && let Some(index) = browser.tab_index(id)
                        {
                            browser.select_tab(index);
                        }
                    })
                })));
            })
    }
    pub(super) fn file_drag_target(&self, target: Div, destination: PathBuf) -> Div {
        let internal = self.clone();
        let external = self.clone();
        let internal_destination = destination.clone();
        target
            .drag_over(|style| style.bg(theme().accent_muted))
            .drop_target(move |drag: &FileDrag, event| {
                let mode = if event.modifiers.copies_drag() {
                    transfer::Mode::Copy
                } else {
                    transfer::Mode::Move
                };
                internal.transfer_dropped(internal_destination.clone(), drag.paths.to_vec(), mode);
            })
            .on_drop_files(move |paths, _| {
                external.transfer_dropped(destination.clone(), paths.to_vec(), transfer::Mode::Copy)
            })
    }
    pub(super) fn file_row_drag(&self, target: Div, row: &Row, grid: Option<Grid>) -> Div {
        let Row::File(entry) = row else {
            return target;
        };
        let source = entry.clone();
        let browser = self.clone();
        let window = Rc::as_ptr(&self.0) as usize;
        let target = target
            .draggable(move || {
                let (paths, preview) = browser.read(|state| {
                    let tab = state.tab();
                    let selection = &tab.navigation.view().selection;
                    if !selection.contains(&RowKey::Local(source.path.clone())) {
                        return (vec![source.path.clone()], vec![source.clone()]);
                    }
                    let mut paths = Vec::new();
                    let mut preview = Vec::new();
                    for row in &tab.rows {
                        if let Row::File(entry) = row
                            && selection.contains(&row.key())
                        {
                            paths.push(entry.path.clone());
                            if preview.len() < PREVIEW_ITEMS {
                                preview.push(entry.clone());
                            }
                        }
                    }
                    (paths, preview)
                });
                let paths: Rc<[PathBuf]> = paths.into();
                DragData::new(FileDrag {
                    window,
                    paths: paths.clone(),
                    preview: preview.into(),
                    grid,
                })
                .files(paths.iter())
                .actions(DragActions::Both)
            })
            .drag_preview(FileDrag::preview);
        if FileIcon::is_directory(entry) {
            self.file_drag_target(target, entry.path.clone())
        } else {
            target
        }
    }
    fn transfer_dropped(&self, destination: PathBuf, paths: Vec<PathBuf>, mode: transfer::Mode) {
        if self.pane() != super::model::Pane::Files {
            return;
        }
        let paths = paths
            .into_iter()
            .filter(|source| {
                source.parent() != Some(destination.as_path()) && source.as_path() != destination
            })
            .collect::<Vec<_>>();
        if paths.is_empty()
            || self.0.volumes.removal_in_progress(&destination)
            || paths
                .iter()
                .any(|path| self.0.volumes.removal_in_progress(path))
        {
            return;
        }
        let Some(cancel) = self.begin_operation("paste-working") else {
            return;
        };
        self.inspect_paste(paste::PasteJob::new(destination, mode, paths), None, cancel);
    }
    pub(super) fn mouse_navigation(&self, event: &PointerEvent) {
        match event.button {
            Some(PointerButton::Other(0x113 | 8)) => self.history(false),
            Some(PointerButton::Other(0x114 | 9)) => self.history(true),
            _ => return,
        }
        event.stop_propagation();
    }
}
