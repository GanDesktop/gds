use bas::{model::Event as SessionEvent, model::SessionSnapshot};
use toasty::{Json, Model};

#[derive(Debug, Model)]
#[table = "cache_sessions"]
#[key(namespace, session)]
pub(super) struct Session {
    pub namespace: String,
    pub session: String,
    #[column(type = text)]
    pub snapshot: Json<SessionSnapshot>,
    pub frontier: i64,
    pub snapshot_bytes: i64,
    #[default(0)]
    pub touched: i64,
    #[default(0)]
    pub bytes: i64,
}

#[derive(Debug, Model)]
#[table = "cache_pages"]
#[key(namespace, session, upper)]
pub(super) struct Page {
    pub namespace: String,
    pub session: String,
    pub upper: i64,
    pub start: i64,
    pub before: Option<i64>,
    #[column(type = text)]
    pub context: Json<Vec<SessionEvent>>,
    pub bytes: i64,
}

#[derive(Debug, Model)]
#[table = "cache_events"]
#[key(namespace, session, cursor)]
pub(super) struct Event {
    pub namespace: String,
    pub session: String,
    pub cursor: i64,
    #[column(type = text)]
    pub payload: Json<SessionEvent>,
    pub bytes: i64,
}
