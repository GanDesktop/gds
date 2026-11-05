use super::Failure;
use bas::{authentication::elicitation, service::pb};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentSummary {
    pub id: String,
    pub kind: String,
    pub command: String,
}

impl From<pb::Agent> for AgentSummary {
    fn from(agent: pb::Agent) -> Self {
        Self {
            id: agent.id,
            kind: agent.kind,
            command: agent.command,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PendingElicitation {
    pub request_id: String,
    pub answered: bool,
    pub request: Result<elicitation::Request, Failure>,
}
