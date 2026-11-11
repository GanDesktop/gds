//! Saved locations and the window's persistence bookkeeping.

use super::*;

pub(super) struct State {
    pub home: PathBuf,
    pub places: Places,
    pub pinned_locations: Vec<Location>,
    pub load_error: Option<String>,
    pub saving: bool,
}

impl State {
    pub(super) fn new(
        home: PathBuf,
        places: Result<Places, crate::entities::location::places::Error>,
    ) -> Self {
        let (places, load_error) = match places {
            Ok(places) => (places, None),
            Err(error) => (Places::default(), Some(error.to_string())),
        };
        Self {
            home,
            pinned_locations: places.pinned.clone(),
            places,
            load_error,
            saving: false,
        }
    }
}

impl Browser {
    pub(super) fn sync_pinned(&mut self) {
        let current: Vec<_> = self
            .tabs
            .items
            .iter()
            .filter(|tab| tab.pinned)
            .map(OpenTab::location)
            .collect();
        // Compare this window's tabs rather than the latest database snapshot,
        // which can contain pins added by another window.
        for path in &self.locations.pinned_locations {
            if !current.contains(path) {
                self.locations.places.set_pinned(path, false);
            }
        }
        for path in &current {
            if !self.locations.pinned_locations.contains(path) {
                self.locations.places.set_pinned(path, true);
            }
        }
        if current != self.locations.pinned_locations {
            self.locations.places.reorder_pinned(current.clone());
        }
        self.locations.pinned_locations = current;
    }

    pub(super) fn save_places(&mut self, cx: &mut Context<Self>) {
        if self.locations.load_error.is_some() || self.locations.saving {
            return;
        }
        let Some(save) = self.locations.places.take_save() else {
            return;
        };
        self.locations.saving = true;
        let work = self
            .window
            .tasks
            .spawn_named("Save window locations", async move { save.run().await });
        cx.spawn(async move |this, cx| {
            let result = work
                .await
                .map_err(crate::entities::location::places::Error::from)
                .and_then(std::convert::identity);
            let _ = this.update(cx, |browser, cx| {
                browser.locations.saving = false;
                match result {
                    Ok(snapshot) => {
                        let views_changed = browser.locations.places.views != snapshot.views
                            || browser.locations.places.defaults != snapshot.defaults;
                        browser.locations.places.accept(snapshot);
                        if views_changed {
                            browser.reproject_columns(cx);
                        }
                        browser.save_places(cx);
                    }
                    Err(error) => {
                        notifications::Report::new(notifications::Level::Error, error.to_string())
                            .replacing::<PlacesNotice>()
                            .show(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
