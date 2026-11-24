//! Unified tabbed properties editor dialog combining Tags, Permissions, and Comment.

use std::{
    cell::RefCell,
    collections::VecDeque,
    path::{Path, PathBuf},
    rc::Rc,
};

use gds::i18n;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::view::{current_host, install_stylesheet};
use zgui::{css, view};
use zgui_ui::{
    TabsActivation, TabsContentProps, TabsListProps, TabsListVariant, TabsProps, TabsTriggerProps,
};
use zgui_ui_primitives::Binding;

use crate::entities::file::directory::Entry;

use super::super::ui::{Browser, WeakBrowser};
#[cfg(target_os = "linux")]
use super::comment::CommentEditor;
use super::{PermissionEditor, tags::TagEditor};

const EDITOR_SHEET: &str = "gds-files-item-editor";

const EDITOR_CSS: &str = css!(
    r#"
    .gds-files-item-editor__tabs {
        display: flex;
        flex-direction: column;
        gap: 14px;
        width: 100%;
        min-width: 0;
    }

    .gds-files-item-editor__tabs-header {
        display: flex;
        flex-direction: row;
        align-items: flex-end;
        width: 100%;
        border-bottom: 1px solid var(--gds-border);
    }

    .gds-files-item-editor__tabs-list.zui-tabs__list[data-variant="line"] {
        height: 34px;
        gap: 20px;
        padding: 0;
        border-radius: 0;
        background-color: transparent;
    }

    .gds-files-item-editor__tabs-trigger.zui-tabs__trigger {
        height: 34px;
        padding: 0 2px;
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-muted-foreground);
        cursor: pointer;
    }

    .gds-files-item-editor__tabs-trigger.zui-tabs__trigger:hover {
        color: var(--gds-foreground);
    }

    .gds-files-item-editor__tabs-trigger.zui-tabs__trigger[data-state="active"] {
        color: var(--gds-foreground);
    }

    .gds-files-item-editor__tabs-trigger.zui-tabs__trigger::after {
        bottom: -1px;
        height: 2px;
        background-color: var(--gds-primary);
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
    Tags,
    #[cfg(unix)]
    Permissions,
    #[cfg(target_os = "linux")]
    Comment,
}

impl EditTab {
    const fn id(self) -> &'static str {
        match self {
            Self::Tags => "tags",
            #[cfg(unix)]
            Self::Permissions => "permissions",
            #[cfg(target_os = "linux")]
            Self::Comment => "comment",
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::Tags => "tags-title",
            #[cfg(unix)]
            Self::Permissions => "permissions",
            #[cfg(target_os = "linux")]
            Self::Comment => "details-comment",
        }
    }
}

pub(super) fn has_editable_tabs(browser: &Browser) -> bool {
    browser.tags_enabled_untracked() || cfg!(unix) || cfg!(target_os = "linux")
}

struct ItemEditorInner {
    browser: WeakBrowser,
    path: PathBuf,
    tabs: Vec<EditTab>,
    active_tab: RwSignal<String, LocalStorage>,
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
        let mut tabs = Vec::new();
        let include_tags = browser.tags_enabled_untracked();
        if include_tags {
            tabs.push(EditTab::Tags);
        }
        #[cfg(unix)]
        tabs.push(EditTab::Permissions);
        #[cfg(target_os = "linux")]
        tabs.push(EditTab::Comment);

        if tabs.is_empty() {
            return None;
        }

        let selected = initial_tab
            .filter(|tab| tabs.contains(tab))
            .unwrap_or(tabs[0]);

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
                path,
                tabs,
                active_tab: RwSignal::new_local(selected.id().to_owned()),
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
        &self.0.path
    }

    pub(super) fn is_saving(&self) -> bool {
        if self.0.saving.get() {
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
        if self.0.saving.get_untracked() {
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

        self.0.tags.as_ref().is_some_and(TagEditor::available)
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
        if let Some(tags) = self.0.tags.as_ref()
            && tags.available_untracked()
            && tags.validate_and_commit_draft().is_err()
        {
            self.0.active_tab.set(EditTab::Tags.id().to_owned());
            tags.focus();
            return;
        }

        let mut pending = VecDeque::new();
        if self
            .0
            .tags
            .as_ref()
            .is_some_and(TagEditor::has_changes_untracked)
        {
            pending.push_back(EditTab::Tags);
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
        #[cfg(target_os = "linux")]
        if self
            .0
            .comment
            .as_ref()
            .is_some_and(CommentEditor::has_changes_untracked)
        {
            pending.push_back(EditTab::Comment);
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
            if any_saved && let Some(browser) = self.0.browser.upgrade() {
                browser.refresh_affected(std::slice::from_ref(&self.0.path));
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
                AnyView::new(view! {
                    TabsTrigger(value = id, class = "gds-files-item-editor__tabs-trigger") {
                        {move || i18n::text(label_key)}
                    }
                })
            })
            .collect();

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
                    {tags_panel}
                    {permissions_panel}
                    {comment_panel}
                }
            }
        })
    }
}
