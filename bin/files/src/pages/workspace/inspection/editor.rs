//! Unified tabbed properties editor dialog combining Tags, Permissions, and Comment.

use std::{
    cell::RefCell,
    collections::VecDeque,
    path::{Path, PathBuf},
    rc::Rc,
};

use gds::i18n;
use gds::ui::{IconName, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::view::{current_host, install_stylesheet};
use zgui::vocab::SharedString;
use zgui::{css, view};
use zgui_ui::{
    TabsActivation, TabsContentProps, TabsListProps, TabsListVariant, TabsProps, TabsTriggerProps,
};
use zgui_ui_primitives::Binding;

use crate::entities::file::directory::Entry;

use super::super::ui::{Browser, WeakBrowser};
use super::basic::BasicEditor;
#[cfg(target_os = "linux")]
use super::comment::CommentEditor;
use super::{PermissionEditor, tags::TagEditor};

const EDITOR_SHEET: &str = "gds-files-item-editor";

const EDITOR_CSS: &str = css!(
    r#"
    .gds-files-item-editor__tabs {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 100%;
        min-width: 0;
    }

    .gds-files-item-editor__tabs-header {
        display: flex;
        flex-direction: row;
        align-items: center;
        width: 100%;
    }

    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"] {
        display: inline-flex;
        flex-direction: row;
        align-items: center;
        height: auto;
        gap: 4px;
        padding: 0;
        border-radius: 0;
        background-color: transparent;
    }

    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger {
        display: inline-flex;
        flex: 0 0 auto;
        align-items: center;
        justify-content: center;
        gap: 6px;
        height: 32px;
        padding: 0 12px;
        border: none;
        border-radius: var(--gds-radius-control, 6px);
        background-color: transparent;
        color: var(--gds-muted-foreground);
        font-size: var(--gds-text-sm, 14px);
        font-weight: 500;
        box-shadow: none;
        cursor: pointer;
        user-select: none;
        transition:
            background-color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1)),
            color var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, cubic-bezier(0, 0, 0.2, 1));
    }

    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger:hover {
        background-color: var(--gds-hover);
        color: var(--gds-foreground);
    }

    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger:active {
        background-color: var(--gds-pressed);
    }

    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger[data-state="active"],
    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger[data-state="active"]:hover,
    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger[data-state="active"]:active {
        background-color: var(--gds-selection);
        color: var(--gds-selection-foreground);
        border-color: transparent;
        box-shadow: none;
    }

    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger:focus:not(:focus-visible) {
        outline: none;
        box-shadow: none;
    }

    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger:focus-visible {
        outline: 1px solid var(--gds-primary);
        outline-offset: -1px;
        border-color: transparent;
        box-shadow: none;
    }

    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger::after,
    .gds-files-item-editor__tabs
        .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"]
        .gds-files-item-editor__tabs-trigger.zui-tabs__trigger[data-state="active"]::after {
        display: none;
        opacity: 0;
    }

    .gds-files-item-editor__tab-icon {
        flex: none;
    }

    .gds-files-item-editor__stage {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 216px;
        min-height: 216px;
        max-height: 216px;
        min-width: 0;
        overflow-y: auto;
        overflow-x: hidden;
    }

    .gds-files-item-editor__panel {
        display: flex;
        flex-direction: column;
        flex: 1 1 0%;
        width: 100%;
        min-width: 0;
        min-height: 0;
    }
    "#
);

fn install_editor_stylesheet() {
    if current_host().is_some() {
        install_stylesheet(EDITOR_SHEET, EDITOR_CSS);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::pages::workspace) enum EditTab {
    Basic,
    Tags,
    #[cfg(unix)]
    Permissions,
    #[cfg(target_os = "linux")]
    Comment,
}

impl EditTab {
    const fn id(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Tags => "tags",
            #[cfg(unix)]
            Self::Permissions => "permissions",
            #[cfg(target_os = "linux")]
            Self::Comment => "comment",
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::Basic => "details-basic",
            Self::Tags => "tags-title",
            #[cfg(unix)]
            Self::Permissions => "permissions",
            #[cfg(target_os = "linux")]
            Self::Comment => "details-comment",
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Basic => IconName::Info,
            Self::Tags => IconName::Tag,
            #[cfg(unix)]
            Self::Permissions => IconName::Shield,
            #[cfg(target_os = "linux")]
            Self::Comment => IconName::MessageSquare,
        }
    }
}

pub(super) fn has_editable_tabs(_browser: &Browser) -> bool {
    true
}

struct ItemEditorInner {
    browser: WeakBrowser,
    original_path: PathBuf,
    current_path: RefCell<PathBuf>,
    tabs: Vec<EditTab>,
    active_tab: RwSignal<String, LocalStorage>,
    basic: BasicEditor,
    tags: Option<TagEditor>,
    #[cfg(unix)]
    permissions: Option<PermissionEditor>,
    #[cfg(target_os = "linux")]
    comment: Option<CommentEditor>,
    saving: RwSignal<bool, LocalStorage>,
}

#[derive(Clone)]
pub(super) struct ItemEditor(Rc<ItemEditorInner>);

impl ItemEditor {
    pub(super) fn from_entry(
        browser: &Browser,
        entry: Entry,
        initial_tab: Option<EditTab>,
    ) -> Option<Self> {
        let path = entry.path.clone();
        Self::build(browser, path, Some(entry), initial_tab)
    }

    pub(super) fn from_path(
        browser: &Browser,
        path: PathBuf,
        initial_tab: Option<EditTab>,
    ) -> Option<Self> {
        Self::build(browser, path, None, initial_tab)
    }

    fn build(
        browser: &Browser,
        path: PathBuf,
        entry: Option<Entry>,
        initial_tab: Option<EditTab>,
    ) -> Option<Self> {
        let mut tabs = vec![EditTab::Basic];
        let include_tags = browser.tags_enabled_untracked();
        if include_tags {
            tabs.push(EditTab::Tags);
        }
        #[cfg(unix)]
        tabs.push(EditTab::Permissions);
        #[cfg(target_os = "linux")]
        tabs.push(EditTab::Comment);

        let selected = initial_tab
            .filter(|tab| tabs.contains(tab))
            .unwrap_or(tabs[0]);

        let basic = BasicEditor::new(
            browser,
            path.clone(),
            entry.as_ref(),
            selected == EditTab::Basic,
        );
        let tags =
            include_tags.then(|| TagEditor::new(browser, path.clone(), selected == EditTab::Tags));
        #[cfg(unix)]
        let permissions = Some(match entry {
            Some(entry) => PermissionEditor::new(browser, entry),
            None => PermissionEditor::from_path(browser, path.clone()),
        });
        #[cfg(not(unix))]
        let _ = entry;
        #[cfg(target_os = "linux")]
        let comment = Some(CommentEditor::new(
            browser,
            path.clone(),
            selected == EditTab::Comment,
        ));

        let editor = browser.0.owner.with(|| {
            Self(Rc::new(ItemEditorInner {
                browser: browser.downgrade(),
                original_path: path.clone(),
                current_path: RefCell::new(path),
                tabs,
                active_tab: RwSignal::new_local(selected.id().to_owned()),
                basic,
                tags,
                #[cfg(unix)]
                permissions,
                #[cfg(target_os = "linux")]
                comment,
                saving: RwSignal::new_local(false),
            }))
        });

        editor.focus_tab(selected.id());
        Some(editor)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0.original_path
    }

    fn propagate_renamed_path(&self, new_path: PathBuf) {
        *self.0.current_path.borrow_mut() = new_path.clone();
        if let Some(tags) = self.0.tags.as_ref() {
            tags.set_path(new_path.clone());
        }
        #[cfg(unix)]
        if let Some(perms) = self.0.permissions.as_ref() {
            perms.set_path(new_path.clone());
        }
        #[cfg(target_os = "linux")]
        if let Some(comment) = self.0.comment.as_ref() {
            comment.set_path(new_path);
        }
    }

    pub(super) fn is_saving(&self) -> bool {
        if self.0.saving.get() || self.0.basic.is_saving() {
            return true;
        }
        if self.0.tags.as_ref().is_some_and(TagEditor::is_saving) {
            return true;
        }
        #[cfg(unix)]
        if self
            .0
            .permissions
            .as_ref()
            .is_some_and(PermissionEditor::is_saving)
        {
            return true;
        }
        #[cfg(target_os = "linux")]
        if self
            .0
            .comment
            .as_ref()
            .is_some_and(CommentEditor::is_saving)
        {
            return true;
        }
        false
    }

    pub(super) fn is_saving_untracked(&self) -> bool {
        if self.0.saving.get_untracked() || self.0.basic.is_saving_untracked() {
            return true;
        }
        if self
            .0
            .tags
            .as_ref()
            .is_some_and(TagEditor::is_saving_untracked)
        {
            return true;
        }
        #[cfg(unix)]
        if self
            .0
            .permissions
            .as_ref()
            .is_some_and(PermissionEditor::is_saving_untracked)
        {
            return true;
        }
        #[cfg(target_os = "linux")]
        if self
            .0
            .comment
            .as_ref()
            .is_some_and(CommentEditor::is_saving_untracked)
        {
            return true;
        }
        false
    }

    pub(super) fn can_save(&self) -> bool {
        if self.is_saving() {
            return false;
        }
        let active = self.0.active_tab.get();
        let active_loading = match active.as_str() {
            "basic" => self.0.basic.is_loading(),
            "tags" => self.0.tags.as_ref().is_some_and(TagEditor::is_loading),
            #[cfg(unix)]
            "permissions" => self
                .0
                .permissions
                .as_ref()
                .is_some_and(PermissionEditor::is_loading),
            #[cfg(target_os = "linux")]
            "comment" => self
                .0
                .comment
                .as_ref()
                .is_some_and(CommentEditor::is_loading),
            _ => false,
        };
        if active_loading {
            return false;
        }

        self.0.basic.available()
            || self.0.tags.as_ref().is_some_and(TagEditor::available)
            || {
                #[cfg(unix)]
                {
                    self.0
                        .permissions
                        .as_ref()
                        .is_some_and(PermissionEditor::can_save)
                }
                #[cfg(not(unix))]
                {
                    false
                }
            }
            || {
                #[cfg(target_os = "linux")]
                {
                    self.0
                        .comment
                        .as_ref()
                        .is_some_and(CommentEditor::available)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    false
                }
            }
    }

    fn focus_tab(&self, id: &str) {
        match id {
            "basic" => self.0.basic.focus(),
            "tags" => {
                if let Some(tags) = self.0.tags.as_ref() {
                    tags.focus();
                }
            }
            #[cfg(target_os = "linux")]
            "comment" => {
                if let Some(comment) = self.0.comment.as_ref() {
                    comment.focus();
                }
            }
            _ => {}
        }
    }

    pub(super) fn save(&self, close: impl FnOnce() + 'static) {
        if self.is_saving_untracked() {
            return;
        }
        if self.0.basic.validate_draft().is_err() {
            self.0.active_tab.set(EditTab::Basic.id().to_owned());
            self.0.basic.focus();
            return;
        }
        if let Some(tags) = self.0.tags.as_ref()
            && tags.available_untracked()
            && tags.validate_and_commit_draft().is_err()
        {
            self.0.active_tab.set(EditTab::Tags.id().to_owned());
            tags.focus();
            return;
        }

        let mut pending = VecDeque::new();
        if self.0.basic.has_changes_untracked() {
            pending.push_back(EditTab::Basic);
        }
        if self
            .0
            .tags
            .as_ref()
            .is_some_and(TagEditor::has_changes_untracked)
        {
            pending.push_back(EditTab::Tags);
        }
        #[cfg(target_os = "linux")]
        if self
            .0
            .comment
            .as_ref()
            .is_some_and(CommentEditor::has_changes_untracked)
        {
            pending.push_back(EditTab::Comment);
        }
        #[cfg(unix)]
        if self
            .0
            .permissions
            .as_ref()
            .is_some_and(PermissionEditor::has_changes_untracked)
        {
            pending.push_back(EditTab::Permissions);
        }

        if pending.is_empty() {
            close();
            return;
        }

        self.0.saving.set(true);
        let close = Rc::new(RefCell::new(Some(Box::new(close) as Box<dyn FnOnce()>)));
        self.save_next(pending, None, false, false, close);
    }

    fn save_next(
        &self,
        mut pending: VecDeque<EditTab>,
        first_failed: Option<&'static str>,
        any_saved: bool,
        tags_saved: bool,
        close: Rc<RefCell<Option<Box<dyn FnOnce()>>>>,
    ) {
        let Some(step) = pending.pop_front() else {
            self.0.saving.set(false);
            let current_path = self.0.current_path.borrow().clone();
            let renamed = current_path != self.0.original_path;
            if (any_saved || renamed)
                && let Some(browser) = self.0.browser.upgrade()
            {
                if renamed {
                    browser.reveal_paths(vec![current_path.clone()]);
                    browser.refresh_affected(&[self.0.original_path.clone(), current_path]);
                } else {
                    browser.refresh_affected(std::slice::from_ref(&self.0.original_path));
                }
                super::bump_details_revision(&browser);
                if tags_saved {
                    browser.refresh_tag_tabs();
                }
                if first_failed.is_none() {
                    browser.report_success(i18n::text_untracked("details-properties-saved"));
                }
            }
            if let Some(failed_id) = first_failed {
                self.0.active_tab.set(failed_id.to_owned());
                self.focus_tab(failed_id);
            } else if let Some(callback) = close.borrow_mut().take() {
                callback();
            }
            return;
        };

        let next_editor = self.clone();
        let on_step_done = move |result: Result<(), ()>| {
            let (next_failed, next_saved, next_tags_saved) = match result {
                Ok(()) => (first_failed, true, tags_saved || step == EditTab::Tags),
                Err(()) => (first_failed.or(Some(step.id())), any_saved, tags_saved),
            };
            next_editor.save_next(pending, next_failed, next_saved, next_tags_saved, close);
        };

        match step {
            EditTab::Basic => {
                let rename_editor = self.clone();
                self.0.basic.save_result(move |result| match result {
                    Ok(Some(new_path)) => {
                        rename_editor.propagate_renamed_path(new_path);
                        on_step_done(Ok(()));
                    }
                    Ok(None) => on_step_done(Ok(())),
                    Err(()) => on_step_done(Err(())),
                });
            }
            EditTab::Tags => {
                if let Some(tags) = self.0.tags.as_ref() {
                    tags.save_result(on_step_done);
                } else {
                    on_step_done(Ok(()));
                }
            }
            #[cfg(unix)]
            EditTab::Permissions => {
                if let Some(perms) = self.0.permissions.as_ref() {
                    perms.save_result(on_step_done);
                } else {
                    on_step_done(Ok(()));
                }
            }
            #[cfg(target_os = "linux")]
            EditTab::Comment => {
                if let Some(comment) = self.0.comment.as_ref() {
                    comment.save_result(on_step_done);
                } else {
                    on_step_done(Ok(()));
                }
            }
        }
    }

    pub(super) fn fields(&self, on_enter_submit: impl Fn() + 'static) -> AnyView {
        install_editor_stylesheet();
        let submit = Rc::new(on_enter_submit);

        if self.0.tabs.len() == 1 {
            let single = match self.0.tabs[0] {
                EditTab::Basic => {
                    let s = Rc::clone(&submit);
                    self.0.basic.fields(move || s())
                }
                EditTab::Tags => self
                    .0
                    .tags
                    .as_ref()
                    .map_or_else(|| AnyView::new(()), TagEditor::fields),
                #[cfg(unix)]
                EditTab::Permissions => self
                    .0
                    .permissions
                    .as_ref()
                    .map_or_else(|| AnyView::new(()), PermissionEditor::fields),
                #[cfg(target_os = "linux")]
                EditTab::Comment => self.0.comment.as_ref().map_or_else(
                    || AnyView::new(()),
                    |c| {
                        let s = Rc::clone(&submit);
                        c.fields(move || s())
                    },
                ),
            };
            return AnyView::new(view! {
                column(class = "gds-files-item-editor__stage") {
                    {single}
                }
            });
        }

        let active_tab = self.0.active_tab;
        let focus_editor = self.clone();
        let tab_binding = Binding::controlled(active_tab.read_only(), move |next: String| {
            active_tab.set(next.clone());
            focus_editor.focus_tab(&next);
        });

        let triggers: Vec<AnyView> = self
            .0
            .tabs
            .iter()
            .copied()
            .map(|tab| {
                let id = tab.id();
                let label_key = tab.label_key();
                let icon = tab.icon();
                AnyView::new(view! {
                    TabsTrigger(
                        value = id,
                        class = "gds-files-item-editor__tabs-trigger",
                        a11y:label = move || SharedString::from(i18n::text(label_key)),
                    ) {
                        Icon(
                            name = icon,
                            size = 14.0,
                            class = "gds-files-item-editor__tab-icon",
                            a11y:hidden = true,
                        )
                        text {{move || i18n::text(label_key)}}
                    }
                })
            })
            .collect();

        let basic_panel = {
            let basic = self.0.basic.clone();
            let s = Rc::clone(&submit);
            AnyView::new(view! {
                TabsContent(
                    value = EditTab::Basic.id(),
                    keep_mounted = true,
                    class = "gds-files-item-editor__panel",
                ) {
                    {basic.fields({
                        let s = Rc::clone(&s);
                        move || s()
                    })}
                }
            })
        };

        let tags_panel = self.0.tags.as_ref().map(|tags| {
            let tags = tags.clone();
            AnyView::new(view! {
                TabsContent(
                    value = EditTab::Tags.id(),
                    keep_mounted = true,
                    class = "gds-files-item-editor__panel",
                ) {
                    {tags.fields()}
                }
            })
        });

        #[cfg(unix)]
        let permissions_panel = self.0.permissions.as_ref().map(|perms| {
            let perms = perms.clone();
            AnyView::new(view! {
                TabsContent(
                    value = EditTab::Permissions.id(),
                    keep_mounted = true,
                    class = "gds-files-item-editor__panel",
                ) {
                    {perms.fields()}
                }
            })
        });
        #[cfg(not(unix))]
        let permissions_panel: Option<AnyView> = None;

        #[cfg(target_os = "linux")]
        let comment_panel = self.0.comment.as_ref().map(|comment| {
            let comment = comment.clone();
            let s = Rc::clone(&submit);
            AnyView::new(view! {
                TabsContent(
                    value = EditTab::Comment.id(),
                    keep_mounted = true,
                    class = "gds-files-item-editor__panel",
                ) {
                    {comment.fields({
                        let s = Rc::clone(&s);
                        move || s()
                    })}
                }
            })
        });
        #[cfg(not(target_os = "linux"))]
        let comment_panel: Option<AnyView> = None;

        AnyView::new(view! {
            Tabs(
                value = tab_binding,
                activation = TabsActivation::Manual,
                class = "gds-files-item-editor__tabs",
            ) {
                box(class = "gds-files-item-editor__tabs-header") {
                    TabsList(
                        variant = TabsListVariant::Line,
                        class = "gds-files-item-editor__tabs-list",
                    ) {
                        {triggers}
                    }
                }
                column(class = "gds-files-item-editor__stage") {
                    {basic_panel}
                    {tags_panel}
                    {permissions_panel}
                    {comment_panel}
                }
            }
        })
    }
}
