//! Window-owned workspace context and UI-independent reactive state operations.

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::{HashMap, HashSet},
    ffi::OsString,
    future::Future,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
};

use gds::{
    app::Gds,
    appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH},
    service::task_manager::{Owner as TaskOwner, Scope},
    ui::sidebar::{SidebarPresentation, SidebarState, use_sidebar_presentation},
};
use tokio_util::sync::CancellationToken;
use zgui::prelude::*;
use zgui::reactive::RenderEffect;

pub(in crate::pages::workspace) use super::ui_flags::FilterFocusMode;
pub(crate) use super::ui_flags::LaunchOptions;
use super::{
    dock::Dock,
    menus::Menus,
    model::{
        AddressPresentation, BackgroundTask, BackgroundTasks, ConfirmOptions, FILES_TASK_KEY,
        Operation, Pages, Pane, Prompt, State,
    },
    name_form::{NameForm, NamePurpose, PromptDecisionSlot},
    typeahead::TypeAhead,
    ui_flags::{AccordionSelection, OpenPickerWindow, SidebarRegion, SidebarRow},
};
use crate::entities::{
    file::columns::Column,
    location::{Location, places::Places, volumes},
    preferences,
};

pub(in crate::pages::workspace) const DEFAULT_WINDOW_WIDTH: f32 = gds::ui::window::INITIAL_WIDTH;
pub(in crate::pages::workspace) const DEFAULT_WINDOW_HEIGHT: f32 = gds::ui::window::INITIAL_HEIGHT;

pub(in crate::pages::workspace) fn resolve_initial_widths(
    reported_width: Option<f32>,
) -> (f32, f32) {
    let window_width = reported_width
        .filter(|width| width.is_finite() && *width > 0.0)
        .unwrap_or(DEFAULT_WINDOW_WIDTH);
    let sidebar_width = if gds::ui::window::WidthTier::from_width(window_width)
        == gds::ui::window::WidthTier::Micro
    {
        COLLAPSED_SIDEBAR_WIDTH
    } else {
        SIDEBAR_WIDTH
    };
    (window_width, (window_width - sidebar_width).max(1.0))
}

pub(in crate::pages::workspace) fn resolve_initial_height(reported_height: Option<f32>) -> f32 {
    reported_height
        .filter(|height| height.is_finite() && *height > 0.0)
        .unwrap_or(DEFAULT_WINDOW_HEIGHT)
}

pub(in crate::pages::workspace) fn should_notify_for_window_focus(
    has_window: bool,
    focused: bool,
) -> bool {
    has_window && !focused
}

pub(in crate::pages::workspace) fn sample_workspace_sidebar(
    sidebar: RwSignal<bool, LocalStorage>,
    window_width: RwSignal<f32, LocalStorage>,
) -> (
    Signal<SidebarPresentation, LocalStorage>,
    Signal<f32, LocalStorage>,
) {
    let sidebar_state = Signal::derive_local(move || {
        SidebarState::from_collapsed(
            !sidebar.get()
                || gds::ui::window::WidthTier::from_width(window_width.get())
                    == gds::ui::window::WidthTier::Micro,
        )
    });
    let presentation = use_sidebar_presentation(sidebar_state);
    let width = Signal::derive_local(move || presentation.get().width());
    (presentation, width)
}

pub(in crate::pages::workspace) fn observe_window_title(
    prefs: ReadSignal<preferences::Preferences, LocalStorage>,
    ready: impl Fn() -> bool + 'static,
    mut apply_title: impl FnMut(&str) + 'static,
) {
    let title_fx = RenderEffect::new(move |prev: Option<Option<String>>| {
        let _ = prefs.with(|p| p.language);
        let _ = Gds::current().i18n().language();
        if !ready() {
            return None;
        }
        let title = gds::i18n::text("app-title");
        if prev.flatten().as_deref() != Some(title.as_str()) {
            apply_title(&title);
        }
        Some(title)
    });
    on_cleanup_local(move || drop(title_fx));
}

pub(in crate::pages::workspace) fn local_memo<T: PartialEq + Clone + 'static>(
    compute: impl Fn() -> T + 'static,
) -> Signal<T, LocalStorage> {
    let owner = Owner::current();
    let owner_fx = owner.clone();
    let compute = Rc::new(compute);
    let compute_fx = Rc::clone(&compute);
    let slot: Rc<OnceCell<RwSignal<T, LocalStorage>>> = Rc::new(OnceCell::new());
    let slot_fx = Rc::clone(&slot);
    let fx = RenderEffect::new(move |_| {
        let next = compute_fx();
        if let Some(signal) = slot_fx.get() {
            if signal.with_untracked(|current| *current != next) {
                signal.set(next);
            }
        } else {
            let signal = match &owner_fx {
                Some(parent) => parent.with(|| RwSignal::new_local(next)),
                None => RwSignal::new_local(next),
            };
            let _ = slot_fx.set(signal);
        }
    });
    on_cleanup_local(move || drop(fx));
    let signal = *slot.get_or_init(|| match &owner {
        Some(parent) => parent.with(|| RwSignal::new_local(compute())),
        None => RwSignal::new_local(compute()),
    });
    Signal::from(signal.read_only())
}

#[derive(Clone, Copy)]
pub(in crate::pages::workspace) struct DragSources(RwSignal<HashSet<PathBuf>, LocalStorage>);

impl DragSources {
    pub fn new() -> Self {
        Self(RwSignal::new_local(HashSet::new()))
    }

    pub fn read_only(self) -> ReadSignal<HashSet<PathBuf>, LocalStorage> {
        self.0.read_only()
    }

    pub fn set(self, paths: impl IntoIterator<Item = PathBuf>) {
        let next: HashSet<PathBuf> = paths.into_iter().collect();
        if self.0.with_untracked(|current| *current != next) {
            self.0.set(next);
        }
    }

    pub fn clear(self) {
        if !self.0.with_untracked(HashSet::is_empty) {
            self.0.set(HashSet::new());
        }
    }

    pub fn contains(self, path: &Path) -> bool {
        self.0
            .with(|sources| !sources.is_empty() && sources.contains(path))
    }

    #[cfg(test)]
    pub fn contains_untracked(self, path: &Path) -> bool {
        self.0
            .with_untracked(|sources| !sources.is_empty() && sources.contains(path))
    }

    pub fn is_empty_untracked(self) -> bool {
        self.0.with_untracked(HashSet::is_empty)
    }
}

pub(in crate::pages::workspace) struct Workspace {
    pub alive: Cell<bool>,
    pub state: RefCell<State>,
    pub revision: RwSignal<u64, LocalStorage>,
    pub selection: RwSignal<u64, LocalStorage>,
    pub column_widths: RwSignal<HashMap<Column, f32>, LocalStorage>,
    pub pages: RwSignal<Pages, LocalStorage>,
    pub window_width: RwSignal<f32, LocalStorage>,
    pub window_height: RwSignal<f32, LocalStorage>,
    pub content_width: RwSignal<f32, LocalStorage>,
    pub footer_filter_open: RwSignal<bool, LocalStorage>,
    pub filter_focus_request: RwSignal<Option<FilterFocusMode>, LocalStorage>,
    pub sidebar_in_accordion: RwSignal<bool, LocalStorage>,
    pub sidebar_accordion: RwSignal<AccordionSelection, LocalStorage>,
    pub address_presentation: RwSignal<AddressPresentation, LocalStorage>,
    pub settings: crate::widgets::settings::View,
    pub sidebar: RwSignal<bool, LocalStorage>,
    pub sidebar_presentation: Signal<SidebarPresentation, LocalStorage>,
    pub sidebar_width: Signal<f32, LocalStorage>,
    pub details: RwSignal<bool, LocalStorage>,
    #[allow(dead_code)]
    pub details_revision: RwSignal<u64, LocalStorage>,
    pub terminal: RwSignal<bool, LocalStorage>,
    pub background_tasks: RwSignal<BackgroundTasks, LocalStorage>,
    pub background_tasks_revision: RwSignal<u64, LocalStorage>,
    pub drag_sources: DragSources,
    #[allow(dead_code)]
    pub native_drag_active: Cell<bool>,
    pub notice: RwSignal<Option<String>, LocalStorage>,
    pub prompt: RwSignal<Prompt, LocalStorage>,
    pub prompt_generation: RwSignal<u64, LocalStorage>,
    pub confirm_options: RwSignal<ConfirmOptions, LocalStorage>,
    pub prompt_decisions: PromptDecisionSlot,
    pub address: RwSignal<String, LocalStorage>,
    pub filter: RwSignal<String, LocalStorage>,
    pub save_name: RwSignal<String, LocalStorage>,
    pub save_input_node: NodeRef,
    pub list: NodeRef,
    pub content_focus: NodeRef,
    pub root_focus: NodeRef,
    pub list_focused: Cell<bool>,
    pub list_focus_visible: RwSignal<bool, LocalStorage>,
    pub settings_node: NodeRef,
    pub address_node: NodeRef,
    pub filter_node: NodeRef,
    pub tabs_node: NodeRef,
    pub quick_access_node: NodeRef,
    pub locations_node: NodeRef,
    pub volumes_node: NodeRef,
    pub sidebar_content_node: NodeRef,
    pub locations_region_node: NodeRef,
    pub sidebar_region: RwSignal<Option<SidebarRegion>, LocalStorage>,
    pub sidebar_cursor: RwSignal<Option<SidebarRow>, LocalStorage>,
    pub sidebar_reveal_quick_access: RwSignal<bool, LocalStorage>,
    pub sidebar_reveal_volumes: RwSignal<bool, LocalStorage>,
    pub tasks: TaskOwner,
    pub owner: Owner,
    pub window: Option<WindowHandle>,
    pub volumes: volumes::Service,
    pub preferences: preferences::Service,
    pub new_window: Rc<dyn Fn(PathBuf)>,
    pub open_picker_window: OpenPickerWindow,
    pub inspections: RwSignal<gds::ui::file_inspection::Cache, LocalStorage>,
    pub external_thumbnailers:
        RefCell<Option<std::sync::Arc<gds::service::file_inspection::ExternalThumbnailers>>>,
    pub external_thumbnailer_probe: RefCell<Option<TaskOwner>>,
    pub external_thumbnailer_gen: Cell<u64>,
    pub terminals: Dock,
    pub connector: crate::features::connect_server::Connector,
    pub menus: Menus,
    pub picker_result: Option<Rc<dyn Fn(Vec<PathBuf>)>>,
    pub pending_focus: RefCell<Option<zgui::view::time::FrameHandle>>,
    pub typeahead: RefCell<TypeAhead>,
}

#[derive(Clone)]
pub(crate) struct Context(pub(in crate::pages::workspace) Rc<Workspace>);

#[derive(Clone)]
pub(in crate::pages::workspace) struct WeakContext(Weak<Workspace>);

impl WeakContext {
    pub(in crate::pages::workspace) fn upgrade(&self) -> Option<Context> {
        self.0
            .upgrade()
            .map(Context)
            .filter(|ctx| ctx.0.alive.get())
    }
}

impl Context {
    pub(in crate::pages::workspace) fn downgrade(&self) -> WeakContext {
        WeakContext(Rc::downgrade(&self.0))
    }

    pub(in crate::pages::workspace) fn focus_content_after_mount(&self) {
        if self.pane_untracked() == Pane::Files
            && matches!(
                self.0.address_presentation.get_untracked(),
                AddressPresentation::Breadcrumbs
            )
            && self.0.content_focus.is_bound()
        {
            self.focus_content();
            return;
        }
        let weak = self.downgrade();
        let handle = self.0.owner.with(|| {
            request_frame(move |_| {
                if let Some(ctx) = weak.upgrade()
                    && ctx.pane_untracked() == Pane::Files
                    && matches!(
                        ctx.0.address_presentation.get_untracked(),
                        AddressPresentation::Breadcrumbs
                    )
                {
                    ctx.focus_content();
                }
            })
        });
        *self.0.pending_focus.borrow_mut() = Some(handle);
    }

    pub(in crate::pages::workspace) fn focus_content(&self) {
        if self.0.content_focus.is_bound() {
            self.0.content_focus.focus();
        } else {
            self.0.root_focus.focus();
        }
    }

    pub(in crate::pages::workspace) fn focus_page_after_mount(&self) {
        if self.pane_untracked() == Pane::Files {
            self.focus_content_after_mount();
        } else {
            let node = self.0.settings_node;
            let handle = self.0.owner.with(|| request_frame(move |_| node.focus()));
            *self.0.pending_focus.borrow_mut() = Some(handle);
        }
    }

    pub(in crate::pages::workspace) fn reset_typeahead(&self) {
        self.0.typeahead.borrow_mut().reset();
    }

    pub(in crate::pages::workspace) fn pane(&self) -> Pane {
        self.0.pages.with(Pages::current)
    }

    pub(in crate::pages::workspace) fn pane_untracked(&self) -> Pane {
        self.0.pages.with_untracked(Pages::current)
    }

    pub(in crate::pages::workspace) fn show_files(&self) {
        self.0.pages.update(|pages| pages.open(Pane::Files));
        self.focus_content_after_mount();
    }

    pub(in crate::pages::workspace) fn back_page(&self) {
        self.0.pages.update(Pages::back);
        self.focus_page_after_mount();
    }

    pub(in crate::pages::workspace) fn track(&self) {
        let _ = self.0.revision.get();
        let _ = self.0.selection.get();
    }

    pub(in crate::pages::workspace) fn changed(&self) {
        self.0
            .revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    pub(in crate::pages::workspace) fn bump_details_revision(&self) {
        self.0
            .details_revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    pub(in crate::pages::workspace) fn read<R>(&self, read: impl FnOnce(&State) -> R) -> R {
        self.track();
        read(&self.0.state.borrow())
    }

    pub(in crate::pages::workspace) fn read_structure<R>(
        &self,
        read: impl FnOnce(&State) -> R,
    ) -> R {
        let _ = self.0.revision.get();
        read(&self.0.state.borrow())
    }

    pub(in crate::pages::workspace) fn read_untracked<R>(
        &self,
        read: impl FnOnce(&State) -> R,
    ) -> R {
        read(&self.0.state.borrow())
    }

    pub(in crate::pages::workspace) fn update_selection<R>(
        &self,
        edit: impl FnOnce(&mut State) -> R,
    ) -> R {
        let result = edit(&mut self.0.state.borrow_mut());
        self.0
            .selection
            .update(|revision| *revision = revision.wrapping_add(1));
        result
    }

    pub(in crate::pages::workspace) fn update<R>(&self, edit: impl FnOnce(&mut State) -> R) -> R {
        let (value, op_sync) = {
            let mut state = self.0.state.borrow_mut();
            let prior = match &state.operation {
                Operation::Idle => None,
                Operation::Running { label, cancel } => {
                    Some((label.clone(), cancel.is_cancelled()))
                }
            };
            let value = edit(&mut state);
            let next = match &state.operation {
                Operation::Idle => None,
                Operation::Running { label, cancel } => Some((label.clone(), cancel.clone())),
            };
            let next_sig = next
                .as_ref()
                .map(|(label, cancel)| (label.as_str(), cancel.is_cancelled()));
            let prior_sig = prior
                .as_ref()
                .map(|(label, cancelled)| (label.as_str(), *cancelled));
            (value, (prior_sig != next_sig).then_some(next))
        };
        if let Some(op_state) = op_sync {
            self.0.background_tasks.update(|tasks| {
                tasks.sync(FILES_TASK_KEY, op_state);
            });
            self.bump_background_tasks_revision();
        }
        self.changed();
        value
    }

    pub(crate) fn report_notice(&self, notice: impl Into<String>) {
        self.0.notice.set(Some(notice.into()));
    }

    pub(in crate::pages::workspace) fn report_success(&self, message: impl Into<String>) {
        gds::ui::feedback::notifications::success(message);
    }

    pub(in crate::pages::workspace) fn report_info(&self, message: impl Into<String>) {
        gds::ui::feedback::notifications::info(message);
    }

    pub(in crate::pages::workspace) fn should_notify_background_untracked(&self) -> bool {
        let has_window = self.0.window.is_some();
        let focused = self
            .0
            .window
            .as_ref()
            .is_some_and(|window| window.focused().get_untracked());
        should_notify_for_window_focus(has_window, focused)
    }

    pub(in crate::pages::workspace) fn notify_if_unfocused(
        &self,
        id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        urgent: bool,
    ) {
        if !self.should_notify_background_untracked() {
            return;
        }
        gds::service::desktop::notify_background(
            &self.scope(),
            gds::service::desktop::DesktopNotification {
                id: id.into(),
                title: title.into(),
                body: body.into(),
                urgency: if urgent {
                    gds::service::desktop::NotificationUrgency::High
                } else {
                    gds::service::desktop::NotificationUrgency::Normal
                },
            },
        );
    }

    pub(in crate::pages::workspace) fn scope(&self) -> Scope {
        (*self.0.tasks).clone()
    }

    pub(in crate::pages::workspace) fn work<T: Send + 'static>(
        &self,
        name: &'static str,
        work: impl Future<Output = Result<T, String>> + Send + 'static,
        finished: impl FnOnce(&Context, Result<T, String>) + 'static,
    ) {
        let ctx = self.downgrade();
        let task = self.0.tasks.spawn_named(name, work);
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(std::convert::identity);
                if let Some(ctx) = ctx.upgrade() {
                    finished(&ctx, result);
                }
            }));
        });
    }

    pub(in crate::pages::workspace) fn blocking<T: Send + 'static>(
        &self,
        name: &'static str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
        finished: impl FnOnce(&Context, Result<T, String>) + 'static,
    ) {
        let ctx = self.downgrade();
        let task = self.0.tasks.spawn_blocking_named(name, work);
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                let result = task
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(std::convert::identity);
                if let Some(ctx) = ctx.upgrade() {
                    finished(&ctx, result);
                }
            }));
        });
    }

    /// Subscribe this window to external place-store updates (such as live
    /// `user-dirs.dirs` reloads) and rebase them onto any pending local edits.
    pub(in crate::pages::workspace) fn bind_places_sync(&self) {
        let Some(mut updates) =
            self.read_untracked(|state| state.places.subscribe_external_updates())
        else {
            return;
        };
        let weak = self.downgrade();
        let cancel = self.0.tasks.token();
        self.0.owner.with(|| {
            drop(spawn_local(async move {
                enum Event {
                    Cancelled,
                    Updated,
                    Closed,
                }
                loop {
                    let event = tokio::select! {
                        () = cancel.cancelled() => Event::Cancelled,
                        result = updates.changed() => match result {
                            Ok(()) => Event::Updated,
                            Err(_) => Event::Closed,
                        },
                    };
                    match event {
                        Event::Cancelled | Event::Closed => break,
                        Event::Updated => {
                            let snapshot = updates.borrow_and_update().clone();
                            let Some(snapshot) = snapshot else {
                                continue;
                            };
                            let Some(ctx) = weak.upgrade() else {
                                break;
                            };
                            ctx.update(|state| {
                                state.places.accept(snapshot);
                            });
                        }
                    }
                }
            }));
        });
    }

    pub(in crate::pages::workspace) fn save_places(&self) {
        let save = {
            let mut state = self.0.state.borrow_mut();
            if state.saving_places {
                return;
            }
            let Some(save) = state.places.take_save() else {
                return;
            };
            state.saving_places = true;
            save
        };
        self.work(
            "Save Files places",
            async move { save.run().await.map_err(|error| error.to_string()) },
            |ctx, result| {
                ctx.update(|state| {
                    state.saving_places = false;
                    if let Ok(snapshot) = result.as_ref() {
                        state.places.accept(snapshot.clone());
                    }
                });
                match result {
                    Ok(_) => ctx.save_places(),
                    Err(error) => ctx.report_notice(error),
                }
            },
        );
    }

    pub(in crate::pages::workspace) fn edit_preferences(
        &self,
        edit: impl FnOnce(&mut preferences::Preferences),
    ) {
        if let Err(error) = self.0.preferences.edit(edit) {
            self.report_notice(error.to_string());
        }
    }

    pub(in crate::pages::workspace) fn open_prompt(&self, prompt: Prompt) {
        self.open_prompt_with_options(prompt, ConfirmOptions::default());
    }

    fn open_prompt_with_options(&self, prompt: Prompt, options: ConfirmOptions) {
        debug_assert!(!matches!(prompt, Prompt::Closed));
        self.0.menus.close();
        self.0.confirm_options.set(options);
        self.0.prompt.set(prompt);
        self.0
            .prompt_generation
            .update(|generation| *generation = generation.wrapping_add(1));
    }

    pub(in crate::pages::workspace) fn confirm_with_options(
        &self,
        title: String,
        message: String,
        primary: String,
        options: ConfirmOptions,
        accept: impl Fn() + 'static,
    ) {
        self.open_prompt_with_options(
            Prompt::Confirm {
                title,
                message,
                primary,
                accept: Rc::new(accept),
            },
            options,
        );
    }

    pub(in crate::pages::workspace) fn prompt_name(
        &self,
        title: String,
        initial: String,
        submit: impl Fn(String) + 'static,
    ) {
        self.name_form(
            NameForm::new(NamePurpose::Generic, title, initial.into()),
            move |name| {
                submit(
                    name.into_string()
                        .expect("generic name comes from a UTF-8 text field"),
                );
            },
        );
    }

    pub(in crate::pages::workspace) fn name_form(
        &self,
        form: NameForm,
        submit: impl Fn(OsString) + 'static,
    ) {
        self.open_prompt(Prompt::Name {
            form,
            submit: Rc::new(submit),
        });
    }

    pub(in crate::pages::workspace) fn conflict_prompt(
        &self,
        mode: crate::features::file_operations::transfer::Mode,
        conflict: crate::features::file_operations::paste::PasteConflict,
        resolve: impl Fn(crate::features::file_operations::paste::ConflictResolution) + 'static,
        cancel: impl Fn() + 'static,
    ) {
        self.open_prompt(Prompt::Conflict {
            mode,
            conflict,
            resolve: Rc::new(resolve),
            cancel: Some(Rc::new(cancel)),
        });
    }

    pub(in crate::pages::workspace) fn cancel_prompt(&self) {
        let prompt = self.0.prompt.get_untracked();
        if matches!(prompt, Prompt::Closed) {
            return;
        }
        self.0.prompt_decisions.clear();
        self.0.prompt.set(Prompt::Closed);
        if let Prompt::Conflict {
            cancel: Some(cancel),
            ..
        } = prompt
        {
            cancel();
        }
    }

    pub(in crate::pages::workspace) fn column_widths_map(
        places: &Places,
        location: &Location,
    ) -> HashMap<Column, f32> {
        places
            .column_widths(location.clone())
            .into_iter()
            .filter_map(|(id, width)| Column::from_id(&id).map(|column| (column, width)))
            .collect()
    }

    pub(in crate::pages::workspace) fn sidebar_presentation(
        &self,
    ) -> Signal<SidebarPresentation, LocalStorage> {
        self.0.sidebar_presentation
    }

    pub(in crate::pages::workspace) fn sidebar_width(&self) -> Signal<f32, LocalStorage> {
        self.0.sidebar_width
    }

    pub(in crate::pages::workspace) fn tags_enabled(&self) -> bool {
        self.0.preferences.state().with(|prefs| prefs.tags.enabled)
    }

    pub(in crate::pages::workspace) fn tags_enabled_untracked(&self) -> bool {
        self.0.preferences.snapshot().tags.enabled
    }

    fn bump_background_tasks_revision(&self) {
        self.0
            .background_tasks_revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    fn mirror_files_operation(&self, task: Option<(String, CancellationToken)>) {
        let mut state = self.0.state.borrow_mut();
        state.operation = match task {
            Some((label, cancel)) => Operation::Running { label, cancel },
            None => Operation::Idle,
        };
    }

    pub(in crate::pages::workspace) fn background_tasks_revision(
        &self,
    ) -> ReadSignal<u64, LocalStorage> {
        self.0.background_tasks_revision.read_only()
    }

    pub(in crate::pages::workspace) fn background_task_list(&self) -> Vec<BackgroundTask> {
        let _ = self.0.background_tasks_revision.get();
        self.0.background_tasks.with(|tasks| tasks.items().to_vec())
    }

    pub(in crate::pages::workspace) fn has_background_tasks_untracked(&self) -> bool {
        self.0
            .background_tasks
            .with_untracked(|tasks| !tasks.is_empty())
    }

    pub(in crate::pages::workspace) fn is_background_task_busy(&self, key: &str) -> bool {
        let _ = self.0.background_tasks_revision.get();
        self.0.background_tasks.with(|tasks| tasks.is_busy(key))
    }

    pub(in crate::pages::workspace) fn is_background_task_busy_untracked(&self, key: &str) -> bool {
        self.0
            .background_tasks
            .with_untracked(|tasks| tasks.is_busy(key))
    }

    pub(in crate::pages::workspace) fn begin_background_task(
        &self,
        key: impl Into<String>,
        label: impl Into<String>,
    ) -> Option<CancellationToken> {
        let key = key.into();
        let label = label.into();
        let cancel = self.0.tasks.token().child_token();
        if self.is_background_task_busy_untracked(&key) {
            return None;
        }
        if key == FILES_TASK_KEY {
            if self.read_untracked(|state| state.operation.busy()) {
                return None;
            }
            self.mirror_files_operation(Some((label.clone(), cancel.clone())));
        }
        let mut started = false;
        let label_for_task = label;
        let cancel_for_task = cancel.clone();
        let key_for_task = key;
        self.0.background_tasks.update(|tasks| {
            started = tasks.begin(key_for_task, label_for_task, cancel_for_task);
        });
        if !started {
            return None;
        }
        self.bump_background_tasks_revision();
        self.changed();
        Some(cancel)
    }

    pub(in crate::pages::workspace) fn set_background_task_detail(
        &self,
        key: &str,
        detail: Option<String>,
    ) {
        let mut updated = false;
        self.0.background_tasks.update(|tasks| {
            updated = tasks.set_detail(key, detail);
        });
        if updated {
            self.bump_background_tasks_revision();
        }
    }

    pub(in crate::pages::workspace) fn cancel_background_task(&self, key: &str) -> bool {
        let cancelled = self
            .0
            .background_tasks
            .with_untracked(|tasks| tasks.cancel(key));
        if key == FILES_TASK_KEY {
            self.read_untracked(|state| state.operation.cancel());
        }
        if cancelled {
            self.bump_background_tasks_revision();
            self.changed();
        }
        cancelled
    }

    pub(in crate::pages::workspace) fn finish_background_task(
        &self,
        key: &str,
    ) -> Option<BackgroundTask> {
        if key == FILES_TASK_KEY {
            self.mirror_files_operation(None);
        }
        let mut removed = None;
        self.0.background_tasks.update(|tasks| {
            removed = tasks.finish(key);
        });
        if removed.is_some() {
            self.bump_background_tasks_revision();
            self.changed();
        }
        removed
    }

    pub(in crate::pages::workspace) fn drag_sources(
        &self,
    ) -> ReadSignal<HashSet<PathBuf>, LocalStorage> {
        self.0.drag_sources.read_only()
    }

    pub(in crate::pages::workspace) fn set_drag_sources(
        &self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) {
        self.0.drag_sources.set(paths);
    }

    pub(in crate::pages::workspace) fn clear_drag_sources(&self) {
        self.0.drag_sources.clear();
    }

    pub(in crate::pages::workspace) fn is_drag_source(&self, path: &Path) -> bool {
        self.0.drag_sources.contains(path)
    }

    pub(in crate::pages::workspace) fn has_drag_sources_untracked(&self) -> bool {
        !self.0.drag_sources.is_empty_untracked()
    }
}
