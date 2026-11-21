use crate::app::Gds;
use crate::service::task_manager::{Scope, Supervisor};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use zgui::reactive::{Mounted, TestWaker, flush, install, set_frame_waker};

#[test]
fn inspector_disabled_unless_requested_and_quit_cancels_scope() {
    let supervisor = Supervisor::new(false).expect("start I/O supervisor");
    install().expect("install reactive runtime");
    let root = Mounted::new();
    root.with(|| {
        let gds = Gds::install(supervisor.scope());
        assert!(gds.tasks().inspector().is_none());
        assert!(!gds.tasks().is_shutting_down());
        gds.tasks().request_quit();
        assert!(gds.tasks().is_shutting_down());
        assert!(gds.tasks().scope().register().is_err());
    });
    root.unmount();
    supervisor.shutdown();
}

#[test]
fn owner_scope_cancels_subtree_when_child_owner_unmounts() {
    install().expect("install reactive runtime");
    let root = Mounted::new();
    root.with(|| {
        let gds = Gds::install(Scope::default());
        let child = Mounted::new();
        let child_scope = child.with(|| gds.tasks().owner_scope("child-view"));
        assert!(!child_scope.token().is_cancelled());
        assert!(!gds.tasks().is_shutting_down());
        child.unmount();
        assert!(child_scope.token().is_cancelled());
        assert!(!gds.tasks().is_shutting_down());
    });
    root.unmount();
}

#[test]
fn spawn_ui_delivers_supervisor_work_onto_ui_thread() {
    let supervisor = Supervisor::new(true).expect("start inspected I/O supervisor");
    install().expect("install reactive runtime");
    set_frame_waker(Arc::new(TestWaker::default()));
    let root = Mounted::new();
    let seen = Rc::new(RefCell::new(None));
    let completed = Arc::new(AtomicBool::new(false));
    root.with(|| {
        let gds = Gds::install(supervisor.scope());
        assert!(gds.tasks().inspector().is_some());
        let seen = Rc::clone(&seen);
        let completed = Arc::clone(&completed);
        drop(
            gds.tasks()
                .spawn_ui("compute-answer", async { 21 * 2 }, move |result| {
                    *seen.borrow_mut() = Some(result.expect("task completed"));
                    completed.store(true, std::sync::atomic::Ordering::Release);
                }),
        );
    });
    for _ in 0..100 {
        flush();
        if completed.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*seen.borrow(), Some(42));
    root.unmount();
    supervisor.shutdown();
}
