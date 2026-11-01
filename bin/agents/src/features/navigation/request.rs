use crate::entities::session::connection::AgentSummary;
use bas::model::SessionSnapshot;
use gpui_kit::App;
use std::{path::PathBuf, rc::Rc};

pub(crate) type OpenSession = Rc<dyn Fn(SessionSnapshot, &mut App) -> bool>;

#[derive(Clone)]
pub(crate) struct HistoryRequest {
    pub server: bas::service::tls::ConnectionOptions,
    pub agents: Vec<AgentSummary>,
    pub agent: Option<String>,
    pub cwd: Option<PathBuf>,
    pub workspaces: Vec<PathBuf>,
    pub open_session: OpenSession,
}
