/// Locations are opaque source identifiers, never paths on the frontend's filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub path: String,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub location: Location,
    pub hidden: bool,
}

/// One page of directories from the host's data source.
#[derive(Clone, Debug)]
pub struct Page {
    pub location: Location,
    pub parent: Option<String>,
    pub ancestors: Vec<Location>,
    pub entries: Vec<Entry>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub path: Option<String>,
    pub cursor: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Navigation {
    Visit,
    Back,
    Forward,
    Refresh,
    More,
}

#[derive(Default)]
pub(super) struct History {
    pub back: Vec<String>,
    pub forward: Vec<String>,
}
impl History {
    pub fn commit(&mut self, kind: Navigation, previous: Option<&str>, current: &str) {
        let Some(previous) = previous.filter(|path| *path != current) else {
            return;
        };
        match kind {
            Navigation::Visit => {
                self.back.push(previous.into());
                self.forward.clear();
            }
            Navigation::Back => {
                self.back.pop();
                self.forward.push(previous.into());
            }
            Navigation::Forward => {
                self.forward.pop();
                self.back.push(previous.into());
            }
            Navigation::Refresh | Navigation::More => {}
        }
    }
}
