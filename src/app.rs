//! Native application services shared by the windows on one UI thread.
#[cfg(feature = "ui")]
use anti_webui::prelude::on_cleanup;
use std::{cell::RefCell, rc::Rc};

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

struct Services {
    owner: anti_webui::prelude::Owner,
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

/// A typed application context. Clones share services; windows own their own UI state.
#[derive(Clone)]
pub struct Gds(Rc<Services>);

impl Gds {
    /// Install inside the application's reactive root, before creating windows.
    #[cfg(feature = "ui")]
    pub fn install(
        scope: crate::service::task_manager::Scope,
        logs: Option<crate::service::logs::Store>,
    ) -> Self {
        let services = Self(Rc::new(Services {
            owner: anti_webui::prelude::Owner::current()
                .expect("install GDS inside the application reactive root"),
            tasks: Tasks::new(scope),
            appearance: Appearance::new(),
            i18n: I18n::new(),
            ui: Ui::new(logs),
        }));
        CURRENT.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "application services must be installed once per UI thread"
            );
            *slot.borrow_mut() = Some(services.clone());
        });
        let tasks = services.tasks().scope();
        on_cleanup(move || {
            tasks.cancel();
            CURRENT.with(|slot| slot.borrow_mut().take());
        });
        services
    }

    pub fn current() -> Self {
        CURRENT.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("install GDS services before constructing UI")
                .clone()
        })
    }

    /// Create process-owned subscriptions under the application root rather than a window.
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
