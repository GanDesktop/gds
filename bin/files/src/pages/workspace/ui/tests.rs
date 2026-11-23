use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gds::{
    app::Gds,
    appearance::tokens::{COLLAPSED_SIDEBAR_WIDTH, SIDEBAR_WIDTH},
    i18n::Language,
    service::task_manager::Scope,
    ui::window::WIDE_WIDTH,
};
use zgui::reactive::{
    Mounted, RenderEffect, RwSignal, Signal, flush, install, on_cleanup_local, prelude::*,
};

use super::{
    DEFAULT_WINDOW_WIDTH, DragSources, observe_window_title, resolve_initial_widths,
    sample_workspace_sidebar,
};
use crate::{entities::preferences::Preferences, shared::i18n};

#[test]
fn initial_window_width_defaults_to_desktop_width_and_collapses_only_in_micro_tier() {
    let (default_w, default_content_w) = resolve_initial_widths(None);
    assert_eq!(default_w, DEFAULT_WINDOW_WIDTH);
    assert!(default_w >= WIDE_WIDTH);
    assert_eq!(default_content_w, DEFAULT_WINDOW_WIDTH - SIDEBAR_WIDTH);

    let (pending_w, pending_content_w) = resolve_initial_widths(Some(0.0));
    assert_eq!(pending_w, DEFAULT_WINDOW_WIDTH);
    assert_eq!(pending_content_w, DEFAULT_WINDOW_WIDTH - SIDEBAR_WIDTH);

    let (compact_w, compact_content_w) = resolve_initial_widths(Some(860.0));
    assert_eq!(compact_w, 860.0);
    assert_eq!(compact_content_w, 860.0 - SIDEBAR_WIDTH);

    let (micro_w, micro_content_w) = resolve_initial_widths(Some(540.0));
    assert_eq!(micro_w, 540.0);
    assert_eq!(micro_content_w, 540.0 - COLLAPSED_SIDEBAR_WIDTH);
}

#[test]
fn workspace_sidebar_signal_synchronizes_manual_toggle_and_micro_breakpoint() {
    let _ = install();
    let root = Mounted::new();
    root.with(|| {
        let app = Gds::install(Scope::default());
        app.appearance().motion().set_enabled(false);

        let sidebar = RwSignal::new_local(true);
        let window_width = RwSignal::new_local(DEFAULT_WINDOW_WIDTH);
        let (presentation, width) = sample_workspace_sidebar(sidebar, window_width);

        assert_eq!(width.get_untracked(), SIDEBAR_WIDTH);
        assert!(!presentation.get_untracked().collapsed());

        // Compact width (600..1200) keeps user-expanded sidebar open.
        window_width.set(960.0);
        flush();
        assert_eq!(width.get_untracked(), SIDEBAR_WIDTH);
        assert!(!presentation.get_untracked().collapsed());

        // Micro width (< 600) locks the 56px collapsed rail without mutating `sidebar`.
        window_width.set(540.0);
        flush();
        assert_eq!(width.get_untracked(), COLLAPSED_SIDEBAR_WIDTH);
        assert!(presentation.get_untracked().collapsed());

        window_width.set(1280.0);
        flush();
        assert_eq!(width.get_untracked(), SIDEBAR_WIDTH);

        sidebar.set(false);
        flush();
        assert_eq!(width.get_untracked(), COLLAPSED_SIDEBAR_WIDTH);
    });
    root.unmount();
}

#[test]
fn live_window_title_effect_tracks_language_changes_without_redundant_writes() {
    let _ = install();
    let root = Mounted::new();
    let titles: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let titles_out = Rc::clone(&titles);

    root.with(|| {
        let app = Gds::install(Scope::default());
        app.i18n()
            .install_catalog(i18n::prepare(Language::English).expect("English catalog"));
        let prefs = RwSignal::new_local(Preferences::default());
        let ready = RwSignal::new_local(false);

        observe_window_title(
            prefs.read_only(),
            move || ready.get(),
            move |title| titles_out.borrow_mut().push(title.to_owned()),
        );
        flush();
        assert!(titles.borrow().is_empty());

        ready.set(true);
        flush();
        let en_title = gds::i18n::text_untracked("app-title");
        assert_eq!(titles.borrow().as_slice(), std::slice::from_ref(&en_title));

        prefs.update(|current| current.files.show_hidden = true);
        flush();
        assert_eq!(titles.borrow().len(), 1);

        app.i18n()
            .install_catalog(i18n::prepare(Language::Chinese).expect("Chinese catalog"));
        prefs.update(|current| current.language = Some(Language::Chinese));
        flush();
        let zh_title = gds::i18n::text_untracked("app-title");
        assert_ne!(en_title, zh_title);
        assert_eq!(titles.borrow().as_slice(), [en_title.clone(), zh_title]);
    });
    root.unmount();
}

#[test]
fn active_drag_sources_reactively_track_dragged_paths_without_spurious_wakes() {
    let _ = install();
    let root = Mounted::new();
    root.with(|| {
        let sources = DragSources::new();
        let target = PathBuf::from("/tmp/demo/alpha.txt");
        let bystander = PathBuf::from("/tmp/demo/beta.txt");
        let target_for_sig = target.clone();
        let ghosted = Signal::derive_local(move || sources.contains(&target_for_sig));
        let wakes = Rc::new(Cell::new(0usize));
        let wakes_fx = Rc::clone(&wakes);
        let fx = RenderEffect::new(move |_| {
            let _ = ghosted.get();
            wakes_fx.set(wakes_fx.get() + 1);
        });
        on_cleanup_local(move || drop(fx));
        flush();

        assert!(!ghosted.get_untracked());
        assert_eq!(wakes.get(), 1);

        sources.set([target.clone()]);
        flush();
        assert!(ghosted.get_untracked());
        assert!(!sources.contains_untracked(&bystander));
        assert_eq!(wakes.get(), 2);

        sources.set([target.clone()]);
        flush();
        assert_eq!(wakes.get(), 2);

        sources.clear();
        flush();
        assert!(!ghosted.get_untracked());
        assert!(sources.is_empty_untracked());
        assert_eq!(wakes.get(), 3);
    });
    root.unmount();
}
