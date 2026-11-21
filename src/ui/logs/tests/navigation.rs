use std::time::{Duration, SystemTime};

use zgui::platform::{AppHandler, SurfaceEvent};
use zgui::prelude::*;
use zgui::vocab::{KeyCode, KeyEvent, KeyState, Modifiers, NamedKey, PhysicalKey, Timestamp};
use zgui_platform_headless::Harness;

use super::super::navigation::{NavigationStep, step_index, step_selection};
use super::{deliver, pump_frames, run_headless_logs, with_viewer_probe};
use crate::service::logs::{Entry, Level, Store};

fn entry(id: u64, message: &str) -> Entry {
    Entry {
        id,
        time: SystemTime::UNIX_EPOCH,
        level: Level::Info,
        target: "test".to_owned(),
        message: message.to_owned(),
    }
}

fn press_nav(harness: &mut Harness<Box<dyn AppHandler>>, key: NamedKey, code: KeyCode, ms: u64) {
    let event = KeyEvent::named(key, PhysicalKey::Code(code));
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Pressed,
        event: event.clone(),
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms)),
    });
    harness.deliver_to_first(SurfaceEvent::Key {
        state: KeyState::Released,
        event,
        modifiers: Modifiers::NONE,
        timestamp: Timestamp::from_origin(Duration::from_millis(ms + 5)),
    });
    pump_frames(harness);
}

#[test]
fn arrow_and_page_navigation_respects_boundaries_and_initial_selection() {
    let entries: Vec<Entry> = (0..10)
        .map(|i| entry(i as u64 + 100, &format!("msg {i}")))
        .collect();
    let visible = vec![1, 3, 5, 7, 9];

    assert_eq!(
        step_selection(&visible, &entries, None, NavigationStep::Next, 3),
        Some((0, 101))
    );
    assert_eq!(
        step_selection(&visible, &entries, None, NavigationStep::Previous, 3),
        Some((4, 109))
    );
    assert_eq!(
        step_selection(&visible, &entries, Some(101), NavigationStep::Next, 3),
        Some((1, 103))
    );
    assert_eq!(
        step_selection(&visible, &entries, Some(101), NavigationStep::PageDown, 3),
        Some((3, 107))
    );
    assert_eq!(
        step_selection(&visible, &entries, Some(109), NavigationStep::PageUp, 3),
        Some((1, 103))
    );
    assert_eq!(
        step_selection(&visible, &entries, Some(101), NavigationStep::Previous, 3),
        Some((0, 101))
    );
}

#[test]
fn navigation_on_empty_visible_list_returns_none() {
    assert_eq!(step_index(None, 0, NavigationStep::Next, 4), None);
    assert_eq!(step_index(None, 0, NavigationStep::Previous, 4), None);
    assert_eq!(
        step_selection(&[], &[], None, NavigationStep::PageDown, 4),
        None
    );
}

#[test]
fn keyboard_arrows_and_paging_move_log_selection_in_live_viewer() {
    let store = Store::new();
    run_headless_logs(false, Some(store.clone()), |harness, _| {
        let ids: Vec<_> = (0..50)
            .map(|i| store.record(Level::Info, "frontend", format!("entry {i}")))
            .collect();
        deliver(harness);

        with_viewer_probe(|probe| {
            probe.root_ref.focus();
            probe.selected.set(None);
        });
        pump_frames(harness);

        press_nav(harness, NamedKey::ArrowDown, KeyCode::ArrowDown, 10);
        with_viewer_probe(|probe| {
            assert_eq!(probe.selected.get_untracked(), Some(ids[0]));
        });

        press_nav(harness, NamedKey::ArrowDown, KeyCode::ArrowDown, 30);
        with_viewer_probe(|probe| {
            assert_eq!(probe.selected.get_untracked(), Some(ids[1]));
        });

        press_nav(harness, NamedKey::PageDown, KeyCode::PageDown, 50);
        with_viewer_probe(|probe| {
            let sel = probe.selected.get_untracked().expect("paged selection");
            assert!(sel > ids[1], "PageDown must jump forward past row 1");
        });

        press_nav(harness, NamedKey::PageUp, KeyCode::PageUp, 70);
        with_viewer_probe(|probe| {
            assert_eq!(probe.selected.get_untracked(), Some(ids[1]));
        });

        press_nav(harness, NamedKey::ArrowUp, KeyCode::ArrowUp, 80);
        with_viewer_probe(|probe| {
            assert_eq!(probe.selected.get_untracked(), Some(ids[0]));
        });

        with_viewer_probe(|probe| {
            probe.query.set("no-such-log-entry".to_owned());
        });
        pump_frames(harness);
        press_nav(harness, NamedKey::ArrowDown, KeyCode::ArrowDown, 100);
        with_viewer_probe(|probe| {
            assert_eq!(probe.selected.get_untracked(), None);
        });
    });
}
