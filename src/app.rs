//! Native application services shared across windows on one UI thread.
use std::{cell::RefCell, rc::Rc, sync::OnceLock};
use zgui::reactive::{Owner, on_cleanup_local, provide_local_context, use_local_context};

mod task_manager;
pub use task_manager::Tasks;
#[cfg(feature = "appearance")]
pub mod appearance;
#[cfg(feature = "appearance")]
pub use appearance::Appearance;
#[cfg(feature = "i18n")]
pub mod i18n;
#[cfg(feature = "i18n")]
pub use i18n::I18n;
#[cfg(feature = "ui")]
pub mod ui;
#[cfg(feature = "ui")]
pub use ui::Ui;

static PROCESS_EXECUTOR: OnceLock<Option<zgui::tokio::Installed>> = OnceLock::new();

struct Services {
    owner: Owner,
    tasks: Tasks,
    #[cfg(feature = "appearance")]
    appearance: Appearance,
    #[cfg(feature = "i18n")]
    i18n: I18n,
    #[cfg(feature = "ui")]
    ui: Ui,
}

thread_local! {
    static CURRENT: RefCell<Option<Gds>> = const { RefCell::new(None) };
}

/// Shared application services context. Clones point to the same UI-thread registry.
#[derive(Clone)]
pub struct Gds(Rc<Services>);

impl Gds {
    /// Install application services inside the active `zgui` reactive root before opening windows.
    ///
    /// Ensures a process-wide `zgui-tokio` background executor is active, enters `scope`'s Tokio
    /// runtime (when present) on the current UI thread, and exposes `Gds` both as a reactive local
    /// context and on the current UI thread.
    pub fn install(scope: crate::service::task_manager::Scope) -> Self {
        #[cfg(feature = "ui")]
        return Self::install_inner(scope, None, false);
        #[cfg(not(feature = "ui"))]
        return Self::install_inner(scope, false);
    }

    /// Install application services for a process-lived [`Supervisor`](crate::service::task_manager::Supervisor).
    ///
    /// Reuses `scope`'s Tokio runtime directly as `zgui`'s process-wide background executor when
    /// initializing for the first time in the process.
    pub fn install_process(scope: crate::service::task_manager::Scope) -> Self {
        #[cfg(feature = "ui")]
        return Self::install_inner(scope, None, true);
        #[cfg(not(feature = "ui"))]
        return Self::install_inner(scope, true);
    }

    /// Install application services together with an optional UI log collector.
    #[cfg(feature = "ui")]
    pub fn install_with_logs(
        scope: crate::service::task_manager::Scope,
        logs: Option<crate::service::logs::Store>,
    ) -> Self {
        Self::install_inner(scope, logs, false)
    }

    /// Install application services and an optional UI log collector for a process-lived
    /// [`Supervisor`](crate::service::task_manager::Supervisor).
    #[cfg(feature = "ui")]
    pub fn install_process_with_logs(
        scope: crate::service::task_manager::Scope,
        logs: Option<crate::service::logs::Store>,
    ) -> Self {
        Self::install_inner(scope, logs, true)
    }

    fn install_inner(
        scope: crate::service::task_manager::Scope,
        #[cfg(feature = "ui")] logs: Option<crate::service::logs::Store>,
        process_lifetime: bool,
    ) -> Self {
        let owner = Owner::current().expect("install GDS inside the application reactive root");
        let _ = PROCESS_EXECUTOR.get_or_init(|| {
            let result = if process_lifetime && let Some(handle) = scope.runtime_handle() {
                zgui::tokio::install_handle(handle.clone())
            } else {
                zgui::tokio::install()
            };
            match result {
                Ok(installed) => Some(installed),
                Err(error) => {
                    tracing::debug!(
                        %error,
                        "background executor already active; retaining existing spawner"
                    );
                    None
                }
            }
        });
        if let Some(handle) = scope.runtime_handle() {
            zgui::tokio::enter_here(handle.clone());
        }
        let tasks_scope = scope.clone();
        let tasks = Tasks::new(scope, owner.clone());
        #[cfg(feature = "appearance")]
        let appearance = Appearance::new(owner.clone(), tasks.clone());
        let services = Self(Rc::new(Services {
            owner,
            tasks,
            #[cfg(feature = "appearance")]
            appearance,
            #[cfg(feature = "i18n")]
            i18n: I18n::new(),
            #[cfg(feature = "ui")]
            ui: Ui::new(logs),
        }));
        CURRENT.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "application services must be installed once per UI thread"
            );
            *slot.borrow_mut() = Some(services.clone());
        });
        provide_local_context(services.clone());
        on_cleanup_local(move || {
            tasks_scope.cancel();
            CURRENT.with(|slot| {
                slot.borrow_mut().take();
            });
        });
        services
    }

    /// Look up the installed [`Gds`] context on the current UI thread, if active.
    pub fn try_current() -> Option<Self> {
        if zgui::reactive::is_ui_thread()
            && let Some(services) = use_local_context::<Self>()
        {
            return Some(services);
        }
        CURRENT.with(|slot| slot.borrow().clone())
    }

    /// Return the installed [`Gds`] context for the current UI thread.
    pub fn current() -> Self {
        Self::try_current().expect("install GDS services before accessing Gds::current()")
    }

    /// Run `f` under the application-wide reactive owner rather than a single window owner.
    pub fn with<R>(&self, f: impl FnOnce() -> R) -> R {
        self.0.owner.with(f)
    }

    pub fn tasks(&self) -> &Tasks {
        &self.0.tasks
    }

    #[cfg(feature = "appearance")]
    pub fn appearance(&self) -> &Appearance {
        &self.0.appearance
    }

    #[cfg(feature = "i18n")]
    pub fn i18n(&self) -> &I18n {
        &self.0.i18n
    }

    #[cfg(feature = "ui")]
    pub fn ui(&self) -> &Ui {
        &self.0.ui
    }
}

#[cfg(test)]
mod tests {
    use super::Gds;
    use crate::service::task_manager::Supervisor;
    use std::{sync::Arc, time::Duration};
    use zgui::reactive::{
        Mounted, RwSignal, TestWaker, background, blocking, flush, install, prelude::*,
        set_frame_waker, spawn_local,
    };

    fn pump(waker: &TestWaker, settled: impl Fn() -> bool) {
        for _ in 0..2_000 {
            flush();
            if settled() {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
            let _ = waker.take();
        }
        panic!("reactive background tasks did not complete within budget");
    }

    #[test]
    fn background_and_blocking_survive_ephemeral_supervisor_shutdown() {
        install().expect("install reactive runtime");
        let waker = Arc::new(TestWaker::default());
        set_frame_waker(waker.clone());

        let first_supervisor = Supervisor::new(false).expect("start first I/O supervisor");
        let first_root = Mounted::new();
        let primed = first_root.with(|| {
            let _gds = Gds::install(first_supervisor.scope());
            let primed = RwSignal::new(false);
            let _task = spawn_local(async move {
                assert_eq!(blocking(|| 1_u32).await, 1);
                primed.set(true);
            });
            primed
        });
        pump(&waker, || primed.get_untracked());
        first_root.unmount();
        first_supervisor.shutdown();

        let second_supervisor = Supervisor::new(false).expect("start second I/O supervisor");
        let second_root = Mounted::new();
        let outcome = second_root.with(|| {
            let _gds = Gds::install(second_supervisor.scope());
            let outcome = RwSignal::new(None);
            let _task = spawn_local(async move {
                let blocked = blocking(|| 42).await;
                let async_bg = background(async {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    7
                })
                .await;
                tokio::time::sleep(Duration::from_millis(1)).await;
                outcome.set(Some((blocked, async_bg)));
            });
            outcome
        });
        pump(&waker, || outcome.get_untracked().is_some());
        assert_eq!(outcome.get_untracked(), Some((42, 7)));

        second_root.unmount();
        second_supervisor.shutdown();
    }
}
