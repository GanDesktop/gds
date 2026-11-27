use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gds::app::Gds;
use gds::i18n::Language;
use gds::service::task_manager::{Scope, Supervisor};
use zgui::app::Fonts;
use zgui::geom::{CssPx, DevicePx, Point, Size};
use zgui::platform::{AppHandler, PlatformError, SurfaceEvent};
use zgui::prelude::GetUntracked;
use zgui::vocab::{
    KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, PointerAction, PointerButton,
    PointerEvent, Role, Timestamp,
};
use zgui_platform_headless::Harness;

use super::{
    clear_view_probes, click_node_center, footer_terminal_node, noop_renderer, send_key_chord,
};
use crate::entities::location::{Location, places::Places, volumes};
use crate::entities::preferences;
use crate::pages::workspace::{
    navigation::Navigator,
    panes::MenuHost,
    projection::Rows,
    shell::{Browser, keyboard::Keyboard},
    state::{
        LaunchOptions,
        menus::Surface,
        model::{Prompt, Row},
    },
};
use crate::shared::i18n as files_i18n;

#[test]
fn footer_terminal_icon_button_pointer_clicks_open_on_first_click_and_close_on_second_click() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    clear_view_probes();

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = home;

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        assert!(
            !browser.0.terminal.get_untracked(),
            "terminal dock starts closed"
        );

        let term_btn = footer_terminal_node().expect("recorded footer terminal IconButton NodeRef");

        // 1st click opens terminal dock; 2nd click closes it; 3rd opens; 4th closes!
        for (click_idx, expected_open) in [true, false, true, false].into_iter().enumerate() {
            click_node_center(&mut harness, term_btn);
            assert_eq!(
                browser.0.terminal.get_untracked(),
                expected_open,
                "footer terminal IconButton click #{} expected terminal == {expected_open}",
                click_idx + 1
            );
        }

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::Chinese).expect("Chinese catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: start.clone(),
                    home: start.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.view()
        })
        .expect("footer terminal button click toggle harness");
}

#[test]
fn content_focus_single_dispatch_and_open_menu_or_columns_portal_isolates_workspace_shortcuts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::write(home.join("alpha.txt"), b"a\n").expect("write alpha.txt");
    std::fs::write(home.join("beta.txt"), b"b\n").expect("write beta.txt");
    std::fs::write(home.join("gamma.txt"), b"c\n").expect("write gamma.txt");
    clear_view_probes();

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = home;

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let deadline = Instant::now() + Duration::from_secs(3);
        while browser.read_untracked(|state| state.tab().rows.len() < 3) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for 3 directory rows to load"
            );
            harness.advance(Duration::from_millis(25));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        zgui::reactive::flush();
        harness.settle(8);

        // 1. Single-dispatch verification when `content_focus` holds focus:
        //    Selecting row 0 and pressing `ArrowDown` ONCE must advance to row 1 (never row 2!).
        browser.0.content_focus.focus();
        Keyboard(&browser).select_row(0, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(4);

        send_key_chord(
            &mut harness,
            KeyEvent::named(NamedKey::ArrowDown, PhysicalKey::Code(KeyCode::ArrowDown)),
            Modifiers::NONE,
        );
        let selected_idx = browser.read_untracked(|state| {
            let tab = state.tab();
            let key = tab
                .navigation
                .view()
                .selection
                .as_slice()
                .first()
                .expect("single selected key");
            tab.backing.position(key).expect("selected row index")
        });
        assert_eq!(
            selected_idx, 1,
            "single ArrowDown on content_focus must advance selection by 1 row (to index 1, never 2)"
        );

        // Open a 2nd tab and record a recent location so `Ctrl+W` and `Alt+C` are observable.
        Navigator(&browser).new_tab(None);
        browser.update(|state| {
            state.places.record(Location::from(state.home.as_path()));
        });
        zgui::reactive::flush();
        harness.settle(8);
        assert_eq!(browser.read_untracked(|state| state.tabs.len()), 2);
        assert_eq!(browser.read_untracked(|state| state.places.recent.len()), 1);

        // 2. Open menu (`show_app_menu`) isolation — both when portaled menu holds focus
        //    and when trigger / `root_focus` holds focus!
        MenuHost(&browser).show_app_menu();
        zgui::reactive::flush();
        harness.settle(8);
        assert!(browser.0.menus.is_open());

        for focus_root in [false, true] {
            if focus_root {
                browser.0.root_focus.focus();
                harness.settle(4);
            }
            send_key_chord(&mut harness, KeyEvent::character("b"), Modifiers::CONTROL);
            send_key_chord(&mut harness, KeyEvent::character("w"), Modifiers::CONTROL);
            send_key_chord(&mut harness, KeyEvent::character("c"), Modifiers::ALT);
            assert!(
                browser.0.sidebar.get_untracked(),
                "open menu must block Ctrl+B (focus_root={focus_root})"
            );
            assert_eq!(
                browser.read_untracked(|state| state.tabs.len()),
                2,
                "open menu must block Ctrl+W (focus_root={focus_root})"
            );
            assert_eq!(
                browser.read_untracked(|state| state.places.recent.len()),
                1,
                "open menu must block Alt+C ClearRecent (focus_root={focus_root})"
            );
            assert!(
                browser.0.menus.is_open(),
                "menu stays open across unhandled workspace chords (focus_root={focus_root})"
            );
        }

        send_key_chord(
            &mut harness,
            KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
            Modifiers::NONE,
        );
        assert!(
            !browser.0.menus.is_open(),
            "Escape must close open application menu cleanly"
        );

        // 3. `Surface::Columns` portaled `Dialog` isolation — both when portaled dialog holds focus
        //    and when `content_focus` holds focus!
        Rows(&browser).configure_columns();
        zgui::reactive::flush();
        harness.settle(8);
        assert!(matches!(
            browser.0.menus.surface.get_untracked(),
            Surface::Columns { .. }
        ));

        for focus_content in [false, true] {
            if focus_content {
                browser.0.content_focus.focus();
                harness.settle(4);
            }
            send_key_chord(&mut harness, KeyEvent::character("b"), Modifiers::CONTROL);
            send_key_chord(&mut harness, KeyEvent::character("w"), Modifiers::CONTROL);
            if focus_content {
                // When `DialogContent` itself holds focus, `Alt+C` is `DialogActions`'s own Cancel mnemonic;
                // when `content_focus` holds focus, `menus.is_open()` must block `Alt+C` (`ClearRecent`).
                send_key_chord(&mut harness, KeyEvent::character("c"), Modifiers::ALT);
            }
            send_key_chord(&mut harness, KeyEvent::character("m"), Modifiers::ALT);
            assert!(
                browser.0.sidebar.get_untracked(),
                "Surface::Columns must block Ctrl+B (focus_content={focus_content})"
            );
            assert_eq!(
                browser.read_untracked(|state| state.tabs.len()),
                2,
                "Surface::Columns must block Ctrl+W (focus_content={focus_content})"
            );
            assert_eq!(
                browser.read_untracked(|state| state.places.recent.len()),
                1,
                "Surface::Columns must block Alt+C ClearRecent (focus_content={focus_content})"
            );
            assert!(
                matches!(
                    browser.0.menus.surface.get_untracked(),
                    Surface::Columns { .. }
                ),
                "Surface::Columns must not be overwritten by Alt+M (focus_content={focus_content})"
            );
        }

        send_key_chord(
            &mut harness,
            KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
            Modifiers::NONE,
        );
        assert!(
            !browser.0.menus.is_open(),
            "Escape must close Surface::Columns dialog cleanly"
        );

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::English).expect("English catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: start.clone(),
                    home: start.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.view()
        })
        .expect("single-dispatch and portal isolation harness");
}

#[test]
fn context_menu_dismissal_modal_scope_cleanup_and_shift_delete_alt_mnemonics() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::write(home.join("victim.txt"), b"bye\n").expect("write victim.txt");
    clear_view_probes();

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = home.clone();

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let deadline = Instant::now() + Duration::from_secs(3);
        while browser.read_untracked(|state| state.tab().rows.is_empty()) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for victim.txt to load"
            );
            harness.advance(Duration::from_millis(25));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        zgui::reactive::flush();
        harness.settle(8);

        // 1. Right-click empty directory space -> click "New Folder" (first item) ->
        //    context menu must dismiss immediately and stay dismissed after creating the folder.
        let menu_origin = Point::new(CssPx(640.0), CssPx(420.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(menu_origin).with_button(PointerButton::Secondary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(menu_origin).with_button(PointerButton::Secondary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(8);
        zgui::reactive::flush();
        assert!(
            browser.0.menus.is_open(),
            "right-clicking empty directory space must open directory context menu"
        );

        // Opening the context menu via right-click must NOT pre-highlight/focus item 0:
        // pressing Enter before any pointer hover or ArrowDown must leave the menu open
        // without triggering item 0 ("New Folder").
        send_key_chord(
            &mut harness,
            KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
            Modifiers::NONE,
        );
        assert!(
            matches!(browser.0.prompt.get_untracked(), Prompt::Closed),
            "context menu must not pre-focus item 0 on open"
        );
        assert!(
            browser.0.menus.is_open(),
            "unfocused Enter on newly opened context menu must keep menu open"
        );

        // Click the first menu item ("New Folder") inside the anchored context menu.
        let first_item_pos = Point::new(CssPx(680.0), CssPx(438.0));
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Pressed,
            event: PointerEvent::mouse(first_item_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.deliver_to_first(SurfaceEvent::Pointer {
            action: PointerAction::Released,
            event: PointerEvent::mouse(first_item_pos).with_button(PointerButton::Primary),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(8);
        zgui::reactive::flush();
        assert!(
            matches!(browser.0.prompt.get_untracked(), Prompt::Name { .. }),
            "clicking New Folder must open NameForm prompt"
        );
        assert!(
            !browser.0.menus.is_open(),
            "clicking New Folder in context menu must immediately dismiss the context menu"
        );

        // Submit the new folder with Enter and wait for creation.
        send_key_chord(
            &mut harness,
            KeyEvent::named(NamedKey::Enter, PhysicalKey::Code(KeyCode::Enter)),
            Modifiers::NONE,
        );
        let folder_deadline = Instant::now() + Duration::from_secs(3);
        while browser.read_untracked(|state| state.tab().rows.len() < 2) {
            assert!(
                Instant::now() < folder_deadline,
                "timed out waiting for created folder to appear"
            );
            harness.advance(Duration::from_millis(25));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        zgui::reactive::flush();
        harness.settle(8);
        assert!(
            !browser.0.menus.is_open(),
            "context menu must remain closed after new folder creation completes"
        );

        // 2. Opening and closing a `DialogScope::enter()` modal (`Surface::Columns`) must NOT
        //    leak `modal_scopes` in `KeytipRegistry` (which would block subsequent shortcuts
        //    and suppress modal `Alt` keytips).
        Rows(&browser).configure_columns();
        zgui::reactive::flush();
        harness.settle(8);
        assert!(gds::ui::keytips::has_active_modal_scope());
        send_key_chord(
            &mut harness,
            KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
            Modifiers::NONE,
        );
        assert!(
            !gds::ui::keytips::has_active_modal_scope(),
            "closing Surface::Columns must clean up its modal scope from KeytipRegistry"
        );

        let collect_labels = |harness: &Harness<Box<dyn AppHandler>>| -> Vec<String> {
            let Some(surface) = harness.platform().offscreens().first().cloned() else {
                return Vec::new();
            };
            let mut nodes = HashMap::new();
            let mut root = None;
            for update in surface.a11y_log() {
                if let Some(tree) = update.tree {
                    root = Some(tree.root);
                }
                for (id, node) in update.nodes {
                    nodes.insert(id, node);
                }
            }
            let Some(root_id) = root else {
                return Vec::new();
            };
            let mut stack = vec![root_id];
            let mut seen = HashSet::new();
            let mut labels = Vec::new();
            while let Some(id) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let Some(node) = nodes.get(&id) else {
                    continue;
                };
                if node.role() == Role::Label
                    && let Some(lbl) = node.label()
                {
                    labels.push(lbl.to_owned());
                }
                for &child in node.children().iter().rev() {
                    stack.push(child);
                }
            }
            labels
        };

        // Holding `Alt` in the workspace after closing `Surface::Columns` must still reveal
        // workspace keytip badges (e.g., `"M"` on the application menu button).
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
            modifiers: Modifiers::ALT,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(6);
        zgui::reactive::flush();
        harness.settle(4);
        let workspace_labels = collect_labels(&harness);
        assert!(
            workspace_labels.iter().any(|lbl| lbl == "M"),
            "workspace Alt keytip 'M' must appear after closing Surface::Columns, got {workspace_labels:?}"
        );
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(6);
        zgui::reactive::flush();

        // 3. Select `victim.txt` and press `Shift+Delete` -> `Alt+C` cancels, then `Shift+Delete`
        //    again -> `Alt+A` confirms permanent deletion.
        let victim_idx = browser.read_untracked(|state| {
            state
                .tab()
                .rows
                .iter()
                .position(|row| row.label() == "victim.txt")
                .expect("victim.txt row")
        });
        browser.0.content_focus.focus();
        Keyboard(&browser).select_row(victim_idx, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(6);

        send_key_chord(
            &mut harness,
            KeyEvent::named(NamedKey::Delete, PhysicalKey::Code(KeyCode::Delete)),
            Modifiers::SHIFT,
        );
        assert!(
            matches!(browser.0.prompt.get_untracked(), Prompt::Confirm { .. }),
            "Shift+Delete must open permanent delete confirmation prompt"
        );
        assert!(
            gds::ui::keytips::has_active_modal_scope(),
            "Prompt::Confirm must activate modal DialogScope"
        );

        // Holding `Alt` inside `Prompt::Confirm` must mount the modal `"A"` and `"C"` keytip
        // badges while suppressing workspace `"M"`.
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Pressed,
            event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
            modifiers: Modifiers::ALT,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(6);
        zgui::reactive::flush();
        harness.settle(4);
        let modal_labels = collect_labels(&harness);
        assert!(
            modal_labels.iter().any(|lbl| lbl == "A")
                && modal_labels.iter().any(|lbl| lbl == "C")
                && !modal_labels.iter().any(|lbl| lbl == "M"),
            "holding Alt inside Shift+Delete confirmation must show modal 'A'/'C' keytips and suppress workspace 'M', got {modal_labels:?}"
        );
        harness.deliver_to_first(SurfaceEvent::Key {
            state: KeyState::Released,
            event: KeyEvent::named(NamedKey::Alt, PhysicalKey::Code(KeyCode::AltLeft)),
            modifiers: Modifiers::NONE,
            timestamp: Timestamp::ORIGIN,
        });
        harness.settle(6);
        zgui::reactive::flush();

        // Cancel via `Alt+C`.
        send_key_chord(&mut harness, KeyEvent::character("c"), Modifiers::ALT);
        assert!(
            matches!(browser.0.prompt.get_untracked(), Prompt::Closed),
            "Alt+C must cancel the Shift+Delete confirmation dialog"
        );
        assert!(
            !gds::ui::keytips::has_active_modal_scope(),
            "cancelling Prompt::Confirm must release its modal DialogScope"
        );
        assert!(home.join("victim.txt").exists());

        // Re-open with `Shift+Delete` and confirm via `Alt+A`.
        browser.0.content_focus.focus();
        Keyboard(&browser).select_row(victim_idx, Modifiers::NONE);
        zgui::reactive::flush();
        harness.settle(6);
        send_key_chord(
            &mut harness,
            KeyEvent::named(NamedKey::Delete, PhysicalKey::Code(KeyCode::Delete)),
            Modifiers::SHIFT,
        );
        assert!(matches!(
            browser.0.prompt.get_untracked(),
            Prompt::Confirm { .. }
        ));

        send_key_chord(&mut harness, KeyEvent::character("a"), Modifiers::ALT);
        assert!(
            matches!(browser.0.prompt.get_untracked(), Prompt::Closed),
            "Alt+A must confirm the Shift+Delete permanent deletion dialog"
        );
        let delete_deadline = Instant::now() + Duration::from_secs(3);
        while home.join("victim.txt").exists() {
            assert!(
                Instant::now() < delete_deadline,
                "timed out waiting for Alt+A permanent delete to remove victim.txt"
            );
            harness.advance(Duration::from_millis(25));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::English).expect("English catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: start.clone(),
                    home: start.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.view()
        })
        .expect("context menu, modal scope cleanup, and Shift+Delete Alt mnemonics harness");
}

#[test]
fn typeahead_filename_matching_and_auto_focus_search_switch() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir(home.join("My Documents")).expect("create My Documents");
    std::fs::create_dir(home.join("My Pictures")).expect("create My Pictures");
    for name in [
        "Cargo.toml",
        "config.toml",
        "const.rs",
        "Lemur.txt",
        "Leopard.txt",
        "Lion.txt",
        "Llama.txt",
        "notes.txt",
        "zeta.txt",
    ] {
        std::fs::write(home.join(name), b"x\n").expect("write fixture");
    }
    for i in 1..=25 {
        std::fs::write(home.join(format!("item-{i:02}.txt")), b"x\n").expect("write filler");
    }
    clear_view_probes();

    let supervisor = Supervisor::new(false).expect("tokio supervisor");
    let bg_scope = supervisor.scope();
    let browser_slot: Rc<RefCell<Option<Browser>>> = Rc::new(RefCell::new(None));
    let slot_for_view = Rc::clone(&browser_slot);
    let start = home;

    let driver = move |handler: Box<dyn AppHandler>| -> Result<(), PlatformError> {
        let mut harness = Harness::new(handler);
        harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
            DevicePx(1280.0),
            DevicePx(800.0),
        )));
        harness.settle(16);

        let browser = browser_slot.borrow().clone().expect("browser constructed");
        let deadline = Instant::now() + Duration::from_secs(3);
        while browser.read_untracked(|state| state.tab().rows.len() < 36) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for fixture directory to load"
            );
            harness.advance(Duration::from_millis(25));
            harness.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        zgui::reactive::flush();
        harness.settle(8);

        let selected_label = |browser: &Browser| -> Option<String> {
            browser.read_untracked(|state| {
                let tab = state.tab();
                let key = tab.navigation.view().selection.as_slice().first()?;
                let idx = tab.backing.position(key)?;
                tab.rows.get(idx).map(Row::label)
            })
        };

        let send_key_at =
            |harness: &mut Harness<Box<dyn AppHandler>>, event: KeyEvent, at_ms: u64| {
                let stamp = Timestamp::from_origin(Duration::from_millis(at_ms));
                harness.deliver_to_first(SurfaceEvent::Key {
                    state: KeyState::Pressed,
                    event: event.clone(),
                    modifiers: Modifiers::NONE,
                    timestamp: stamp,
                });
                harness.deliver_to_first(SurfaceEvent::Key {
                    state: KeyState::Released,
                    event,
                    modifiers: Modifiers::NONE,
                    timestamp: stamp,
                });
                harness.settle(4);
                zgui::reactive::flush();
            };

        assert!(!browser.0.preferences.snapshot().files.auto_focus_search);
        browser.0.content_focus.focus();
        zgui::reactive::flush();
        harness.settle(4);

        // 1. Continuous multi-key prefix matching ("c", "o", "n", "s").
        send_key_at(&mut harness, KeyEvent::character("c"), 100);
        assert_eq!(selected_label(&browser).as_deref(), Some("Cargo.toml"));
        assert!(browser.0.filter.get_untracked().is_empty());
        assert!(browser.0.list_focus_visible.get_untracked());

        send_key_at(&mut harness, KeyEvent::character("o"), 250);
        assert_eq!(selected_label(&browser).as_deref(), Some("config.toml"));

        send_key_at(&mut harness, KeyEvent::character("n"), 400);
        assert_eq!(selected_label(&browser).as_deref(), Some("config.toml"));

        send_key_at(&mut harness, KeyEvent::character("s"), 550);
        assert_eq!(selected_label(&browser).as_deref(), Some("const.rs"));

        // 2. Repeated same character cycles ("l", "l") and distinct key switches to prefix ("a" -> "lla").
        send_key_at(&mut harness, KeyEvent::character("l"), 2000);
        assert_eq!(selected_label(&browser).as_deref(), Some("Lemur.txt"));
        send_key_at(&mut harness, KeyEvent::character("l"), 2150);
        assert_eq!(selected_label(&browser).as_deref(), Some("Leopard.txt"));
        send_key_at(&mut harness, KeyEvent::character("a"), 2300);
        assert_eq!(selected_label(&browser).as_deref(), Some("Llama.txt"));

        // 3. Space mid-query matches filenames with spaces ("my p" -> "My Pictures").
        send_key_at(&mut harness, KeyEvent::character("m"), 4000);
        assert_eq!(selected_label(&browser).as_deref(), Some("My Documents"));
        send_key_at(&mut harness, KeyEvent::character("y"), 4150);
        assert_eq!(selected_label(&browser).as_deref(), Some("My Documents"));
        send_key_at(
            &mut harness,
            KeyEvent::named(NamedKey::Space, PhysicalKey::Code(KeyCode::Space)),
            4300,
        );
        assert_eq!(selected_label(&browser).as_deref(), Some("My Documents"));
        send_key_at(&mut harness, KeyEvent::character("p"), 4450);
        assert_eq!(selected_label(&browser).as_deref(), Some("My Pictures"));

        // 4. Backspace corrects a mistyped character mid-query.
        send_key_at(&mut harness, KeyEvent::character("c"), 6000);
        send_key_at(&mut harness, KeyEvent::character("o"), 6150);
        send_key_at(&mut harness, KeyEvent::character("x"), 6300);
        assert_eq!(selected_label(&browser).as_deref(), Some("config.toml"));
        send_key_at(
            &mut harness,
            KeyEvent::named(NamedKey::Backspace, PhysicalKey::Code(KeyCode::Backspace)),
            6450,
        );
        send_key_at(&mut harness, KeyEvent::character("n"), 6600);
        send_key_at(&mut harness, KeyEvent::character("s"), 6750);
        assert_eq!(selected_label(&browser).as_deref(), Some("const.rs"));

        // 5. Off-screen match scrolls into view.
        send_key_at(&mut harness, KeyEvent::character("z"), 8000);
        assert_eq!(selected_label(&browser).as_deref(), Some("zeta.txt"));
        assert!(
            browser.0.list.scroll_offset().y.0 > 0.0,
            "matching an off-screen row must scroll the list viewport"
        );

        // 6. Escape resets active type-ahead immediately (no wait for timeout).
        send_key_at(&mut harness, KeyEvent::character("c"), 10_000);
        assert_eq!(selected_label(&browser).as_deref(), Some("Cargo.toml"));
        send_key_at(
            &mut harness,
            KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
            10_100,
        );
        send_key_at(&mut harness, KeyEvent::character("n"), 10_200);
        assert_eq!(selected_label(&browser).as_deref(), Some("notes.txt"));

        // 7. Open menu blocks type-ahead.
        MenuHost(&browser).show_app_menu();
        zgui::reactive::flush();
        harness.settle(6);
        send_key_at(&mut harness, KeyEvent::character("c"), 12_000);
        assert_eq!(selected_label(&browser).as_deref(), Some("notes.txt"));
        send_key_at(
            &mut harness,
            KeyEvent::named(NamedKey::Escape, PhysicalKey::Code(KeyCode::Escape)),
            12_100,
        );

        // 8. Grid mode also supports type-ahead.
        Navigator(&browser).toggle_view();
        zgui::reactive::flush();
        harness.settle(8);
        browser.0.content_focus.focus();
        zgui::reactive::flush();
        harness.settle(4);
        send_key_at(&mut harness, KeyEvent::character("l"), 14_000);
        send_key_at(&mut harness, KeyEvent::character("i"), 14_150);
        assert_eq!(selected_label(&browser).as_deref(), Some("Lion.txt"));

        // 9. Enabling `auto_focus_search` switches typing to focus the filter field in order.
        browser.edit_preferences(|prefs| prefs.files.auto_focus_search = true);
        zgui::reactive::flush();
        harness.settle(6);
        browser.0.content_focus.focus();
        zgui::reactive::flush();
        harness.settle(4);
        send_key_at(&mut harness, KeyEvent::character("n"), 16_000);
        send_key_at(&mut harness, KeyEvent::character("o"), 16_150);
        assert_eq!(browser.0.filter.get_untracked(), "no");
        assert_eq!(browser.read_untracked(|state| state.tab().rows.len()), 1);

        harness.shut_down();
        Ok(())
    };

    zgui::app()
        .with_fonts(Fonts::shipped_only())
        .with_renderer(Box::new(noop_renderer))
        .with_size(1280.0, 800.0)
        .run_on(driver, move || {
            let scope: Scope = bg_scope.clone();
            let app = Gds::install(scope.clone());
            app.appearance().motion().set_enabled(false);
            let _ = app.appearance().install_window_sheet();
            app.i18n()
                .install_catalog(files_i18n::prepare(Language::English).expect("English catalog"));
            let _prefs = preferences::Service::install(preferences::Preferences::default(), &scope)
                .expect("preferences service");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(Places::default()),
                    start: start.clone(),
                    home: start.clone(),
                    picker: None,
                },
                volumes::Service::new(scope),
                Rc::new(|_| {}),
            );
            *slot_for_view.borrow_mut() = Some(browser.clone());
            browser.view()
        })
        .expect("typeahead and auto-focus search harness");
}
