//! State and task ownership shared by the image window's reactive surfaces.
mod canvas;
mod chrome;
mod commands;
mod inspector;
mod workflows;

use gds::{
    app::Gds,
    i18n::text_untracked as text,
    service::{
        image::source::{MemoryOrigin, Origin},
        task_manager::Scope,
    },
    ui::{file_inspection::Cache, keytips::KeytipHostProps, notifications},
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
use zgui::{
    prelude::*,
    reactive::{Owner, RenderEffect},
};
use zgui_ui_primitives::Binding;

use super::{CaptureWorkflow, LaunchMode};
use crate::{
    entities::{
        catalog::CatalogState,
        document::{AnnotationItem, DocumentState, PreparedPayload},
        metadata::MetadataState,
        preferences,
    },
    features::edit_canvas::{ActiveInteraction, EditHistory, ToolFsm},
    shared::{
        clipboard::ClipboardOfferKind,
        geometry::{Point2, Size2, ViewportTransform},
    },
};

#[derive(Default)]
struct State {
    doc: Option<DocumentState>,
    metadata: Option<MetadataState>,
    memory_thumbnail: Option<zgui_image::ImageBytes>,
    catalog: CatalogState,
    tool: ToolFsm,
    history: EditHistory,
    transform: ViewportTransform,
    pan: Option<Point2>,
    frame: Option<PreparedPayload>,
    preview_objects: Option<Arc<[AnnotationItem]>>,
    gesture: u64,
    compositor: Option<canvas::Compositor>,
}

impl State {
    /// Clears document-scoped state while preserving the active tool, tool settings, and catalog.
    fn clear_document(&mut self) {
        self.doc = None;
        self.metadata = None;
        self.memory_thumbnail = None;
        self.tool.selection.clear();
        self.tool.staged_crop = None;
        self.tool.interaction = ActiveInteraction::Idle;
        self.history.clear();
        self.transform = ViewportTransform::default();
        self.pan = None;
        self.frame = None;
        self.preview_objects = None;
        self.gesture += 1;
        self.compositor = None;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ActivePopover {
    #[default]
    None,
    AppMenu,
    ColorSelector,
    CustomSize(inspector::Dimension),
}

#[derive(Clone, Copy, Default)]
pub(super) enum InlineEdit {
    #[default]
    None,
    Rename {
        value: RwSignal<String>,
        node: NodeRef,
    },
    Text {
        value: RwSignal<String>,
        node: NodeRef,
    },
}

pub(super) enum Pending {
    Open(Origin),
    Trash(PathBuf),
    Quit,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum ConfirmKind {
    #[default]
    Discard,
    Trash(PathBuf),
}

pub(super) struct ConfirmPrompt {
    pub(super) kind: ConfirmKind,
    pub(super) action: Pending,
}

struct Workspace {
    owner: Owner,
    scope: Scope,
    cache: Cache,
    workflow: CaptureWorkflow,
    prefs: preferences::Service,
    state: RefCell<State>,
    version: RwSignal<u64>,
    catalog_version: RwSignal<u64>,
    paint: RwSignal<u64>,
    overlay: RwSignal<u64>,
    settings: RwSignal<bool, LocalStorage>,
    about: RwSignal<bool, LocalStorage>,
    confirm_open: RwSignal<bool, LocalStorage>,
    confirm_kind: RwSignal<ConfirmKind, LocalStorage>,
    confirm_generation: RwSignal<u64, LocalStorage>,
    pending: RefCell<Option<ConfirmPrompt>>,
    active_popover: RwSignal<ActivePopover, LocalStorage>,
    inline_edit: RwSignal<InlineEdit, LocalStorage>,
    custom_sizes: RwSignal<inspector::CustomSizes, LocalStorage>,
    fonts: RwSignal<Vec<gds::ui::field::SearchOption>, LocalStorage>,
    viewport: NodeRef,
    size: Cell<Size2>,
    loading: RwSignal<bool, LocalStorage>,
    busy: RwSignal<bool, LocalStorage>,
    epoch: Cell<u64>,
    cancel: RefCell<CancellationToken>,
    rendering: Cell<bool>,
    queued: Cell<bool>,
    clipboard_offer: RefCell<Option<ClipboardOfferKind>>,
    clipboard_had_offer: Cell<bool>,
    clipboard_probing: Cell<bool>,
}

#[derive(Clone, Copy)]
struct Page(StoredValue<Rc<Workspace>, LocalStorage>);

impl Page {
    fn window(self) -> Rc<Workspace> {
        self.0.get_value()
    }

    fn read<T>(self, f: impl FnOnce(&State) -> T) -> T {
        f(&self.window().state.borrow())
    }

    fn change<T>(self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.window().state.borrow_mut())
    }

    fn changed(self) {
        self.window().version.update(|v| *v += 1);
    }

    fn repaint(self) {
        self.window().paint.update(|v| *v += 1);
    }

    fn catalog_changed(self) {
        self.window().catalog_version.update(|v| *v += 1);
    }

    fn error(self, error: impl std::fmt::Display) {
        notifications::error(error.to_string());
    }

    fn notice(self, msg: String) {
        notifications::success(msg);
    }

    fn dirty(self) -> bool {
        self.read(|s| {
            s.doc.as_ref().is_some_and(DocumentState::is_dirty)
                || s.metadata.as_ref().is_some_and(MetadataState::is_dirty)
        })
    }

    fn popover_binding(self, slot: ActivePopover) -> Binding<bool> {
        let active = self.window().active_popover;
        Binding::controlled(
            Signal::derive_local(move || active.get() == slot),
            move |open| {
                if open {
                    active.set(slot);
                } else if active.get_untracked() == slot {
                    active.set(ActivePopover::None);
                }
            },
        )
    }

    fn dismiss_popovers(self) {
        let active = self.window().active_popover;
        if active.get_untracked() != ActivePopover::None {
            active.set(ActivePopover::None);
        }
    }

    fn open_rename(self, initial: String) {
        self.dismiss_popovers();
        let w = self.window();
        let value = w.owner.with(|| RwSignal::new(initial));
        w.inline_edit.set(InlineEdit::Rename {
            value,
            node: NodeRef::new(),
        });
    }

    fn close_rename(self) {
        let w = self.window();
        if matches!(w.inline_edit.get_untracked(), InlineEdit::Rename { .. }) {
            w.inline_edit.set(InlineEdit::None);
        }
    }

    fn is_renaming(self) -> bool {
        matches!(
            self.window().inline_edit.get_untracked(),
            InlineEdit::Rename { .. }
        )
    }

    fn rename_editor(self) -> Option<(RwSignal<String>, NodeRef)> {
        match self.window().inline_edit.get_untracked() {
            InlineEdit::Rename { value, node } => Some((value, node)),
            InlineEdit::None | InlineEdit::Text { .. } => None,
        }
    }

    fn open_text_editor(self, initial: String) {
        self.dismiss_popovers();
        let w = self.window();
        let value = w.owner.with(|| RwSignal::new(initial));
        w.inline_edit.set(InlineEdit::Text {
            value,
            node: NodeRef::new(),
        });
    }

    fn close_text_editor(self) {
        let w = self.window();
        if matches!(w.inline_edit.get_untracked(), InlineEdit::Text { .. }) {
            w.inline_edit.set(InlineEdit::None);
        }
    }

    fn text_editor_value(self) -> Option<String> {
        match self.window().inline_edit.get_untracked() {
            InlineEdit::Text { value, .. } => Some(value.get_untracked()),
            InlineEdit::None | InlineEdit::Rename { .. } => None,
        }
    }

    fn text_editor_node(self) -> Option<NodeRef> {
        match self.window().inline_edit.get_untracked() {
            InlineEdit::Text { node, .. } => Some(node),
            InlineEdit::None | InlineEdit::Rename { .. } => None,
        }
    }

    fn close_inline_edit(self) {
        let w = self.window();
        if !matches!(w.inline_edit.get_untracked(), InlineEdit::None) {
            w.inline_edit.set(InlineEdit::None);
        }
    }

    fn is_clipboard_dismissed(self) -> bool {
        let w = self.window();
        w.clipboard_had_offer.get() && w.clipboard_offer.borrow().is_none()
    }

    fn background<T: Send + 'static>(
        self,
        name: &'static str,
        job: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
        done: impl FnOnce(anyhow::Result<T>) + 'static,
    ) {
        let task = self.window().scope.spawn_blocking_named(name, job);
        self.local(async move {
            done(
                task.await
                    .map_err(anyhow::Error::from)
                    .and_then(std::convert::identity),
            );
        });
    }

    fn local(self, work: impl Future<Output = ()> + 'static) {
        let window = self.window();
        let cancel = window.scope.token();
        window.owner.with(|| {
            zgui::task::spawn_local(async move {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => {}
                    () = work => {}
                }
            });
        });
    }

    fn fit(self) {
        let size = self.window().size.get();
        self.change(|s| {
            if let Some(doc) = &s.doc {
                let (w, h) = doc.output_dimensions();
                s.transform
                    .recenter_if_fitted(size, Size2::new(w as f32, h as f32));
            }
        });
        self.repaint();
    }

    fn request(self, action: Pending) {
        if self.window().busy.get_untracked() {
            return;
        }
        self.flush_text();
        let kind = match &action {
            Pending::Trash(path) => Some(ConfirmKind::Trash(path.clone())),
            Pending::Open(_) | Pending::Quit => self.dirty().then_some(ConfirmKind::Discard),
        };
        if let Some(kind) = kind {
            self.open_confirm(ConfirmPrompt { kind, action });
        } else {
            self.proceed(action);
        }
    }

    fn open_confirm(self, prompt: ConfirmPrompt) {
        self.dismiss_popovers();
        let w = self.window();
        w.confirm_kind.set(prompt.kind.clone());
        *w.pending.borrow_mut() = Some(prompt);
        w.confirm_generation.update(|g| *g += 1);
        w.confirm_open.set(true);
    }

    fn proceed(self, action: Pending) {
        match action {
            Pending::Open(origin) => self.load(origin),
            Pending::Trash(path) => self.trash(path),
            Pending::Quit => Gds::current().tasks().request_quit(),
        }
    }

    fn focus(self) {
        self.window().viewport.focus();
    }
}

pub fn mount(
    mode: LaunchMode,
    workflow: CaptureWorkflow,
    is_capture: bool,
    prefs: preferences::Service,
    notices: Vec<String>,
) -> AnyView {
    let scope = Gds::current().tasks().owner_scope("Images workspace");
    let cache = Cache::with_scope(scope.clone(), Default::default());
    let p = Page(StoredValue::new_local(Rc::new(Workspace {
        owner: Owner::current().expect("window owner"),
        scope: scope.clone(),
        cache,
        workflow,
        prefs: prefs.clone(),
        state: RefCell::default(),
        version: RwSignal::new(0),
        catalog_version: RwSignal::new(0),
        paint: RwSignal::new(0),
        overlay: RwSignal::new(0),
        settings: RwSignal::new_local(false),
        about: RwSignal::new_local(false),
        confirm_open: RwSignal::new_local(false),
        confirm_kind: RwSignal::new_local(ConfirmKind::default()),
        confirm_generation: RwSignal::new_local(0),
        pending: RefCell::default(),
        active_popover: RwSignal::new_local(ActivePopover::default()),
        inline_edit: RwSignal::new_local(InlineEdit::default()),
        custom_sizes: RwSignal::new_local(inspector::CustomSizes::default()),
        fonts: RwSignal::new_local(vec![gds::ui::field::SearchOption::new(
            "sans-serif",
            text("annotation-font-default"),
        )]),
        viewport: NodeRef::new(),
        size: Cell::new(Size2::new(1., 1.)),
        loading: RwSignal::new_local(false),
        busy: RwSignal::new_local(false),
        epoch: Cell::new(0),
        cancel: RefCell::new(scope.token().child_token()),
        rendering: Cell::new(false),
        queued: Cell::new(false),
        clipboard_offer: RefCell::default(),
        clipboard_had_offer: Cell::new(false),
        clipboard_probing: Cell::new(false),
    })));
    p.background(
        "Load annotation font families",
        || Ok(gds::appearance::typography::installed_families()),
        move |result| match result {
            Ok(families) => p.window().fonts.update(|options| {
                options.extend(
                    families
                        .into_iter()
                        .filter(|f| f != "sans-serif")
                        .map(|f| gds::ui::field::SearchOption::new(f.clone(), f)),
                );
            }),
            Err(e) => p.error(e),
        },
    );
    let close = zgui::runtime::on_close_request(move || {
        if p.window().busy.get_untracked() {
            return CloseResponse::Veto;
        }
        p.flush_text();
        if p.dirty() {
            p.request(Pending::Quit);
            CloseResponse::Veto
        } else {
            CloseResponse::Close
        }
    });
    let focus = try_use_window().map(|win| {
        win.on_focus_change(move |focused| {
            if focused {
                if p.read(|s| s.doc.is_none()) && !p.is_clipboard_dismissed() {
                    p.probe_clipboard();
                }
            } else {
                p.close_rename();
                p.dismiss_popovers();
                p.cancel_gesture();
                p.change(|s| s.history.break_coalesce());
            }
        })
    });
    let err = prefs.error();
    let observer = RenderEffect::new(move |prev: Option<Option<String>>| {
        let current = err.get();
        if let Some(message) = &current
            && prev.as_ref().and_then(Option::as_ref) != Some(message)
        {
            p.error(gds::t_untracked!(
                "settings-save-failed",
                error = message.clone()
            ));
        }
        current
    });
    let title_effect = try_use_window().map(|win| {
        let presented = win.presented();
        let version = p.window().version;
        RenderEffect::new(move |prev: Option<Option<String>>| {
            version.track();
            let _ = Gds::current().i18n().language();
            let _ = presented.get();
            if !win.is_open() {
                return None;
            }
            let base = gds::i18n::text(if is_capture {
                "app-capture-title"
            } else {
                "app-title"
            });
            let title = p.read(|s| {
                let Some(doc) = &s.doc else {
                    return base;
                };
                let name = doc.decoded.origin.display_name();
                let dirty =
                    doc.is_dirty() || s.metadata.as_ref().is_some_and(MetadataState::is_dirty);
                if dirty {
                    format!("{name} * — {base}")
                } else {
                    format!("{name} — {base}")
                }
            });
            if prev.flatten().as_deref() != Some(title.as_str()) {
                win.set_title(&title);
            }
            Some(title)
        })
    });
    let node = p.window().viewport;
    let size = node.observe_border_size();
    let resize = RenderEffect::new(move |_| {
        if let Some(sz) = size.get() {
            let scale = node.scale().max(0.01);
            let size = Size2::new(sz.width.0 / scale, sz.height.0 / scale);
            if size.width > 0. && size.height > 0. && p.window().size.replace(size) != size {
                let previous = p.read(|s| s.transform.scale);
                p.fit();
                if p.read(|s| {
                    s.transform.scale != previous
                        && s.doc
                            .as_ref()
                            .is_some_and(|d| d.format() == gds::service::image::source::Format::Svg)
                }) {
                    p.render();
                }
            }
        }
    });
    on_cleanup_local(move || drop((close, focus, resize, observer, title_effect)));
    match mode {
        LaunchMode::OpenFile(path) => p.load(Origin::LocalFile { path, stamp: None }),
        LaunchMode::Stdin(bytes) => p.load(Origin::Memory {
            kind: MemoryOrigin::Stdin,
            bytes,
            format_hint: None,
        }),
        LaunchMode::Clipboard => p.open_clipboard(),
        LaunchMode::EmptySmartState => p.probe_clipboard(),
    }
    if !notices.is_empty() {
        p.local(async move {
            for notice in notices {
                notifications::warning(notice);
            }
        });
    }
    use chrome::WorkspaceViewProps;
    AnyView::new(view! { KeytipHost { WorkspaceView(page = p) } })
}
