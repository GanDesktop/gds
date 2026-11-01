use crate::entities::session::connection;
use crate::shared::i18n::tr;

#[derive(Debug, thiserror::Error)]
pub(in crate::features::agent_auth) enum Failure {
    #[error("authentication elicitation has no request")]
    MissingRequest,
    #[error("invalid authentication payload: {0}")]
    InvalidPayload(String),
    #[error("authentication input ended before start")]
    InputEnded,
    #[error("authentication input closed")]
    InputClosed,
    #[error("authentication stream ended without a check")]
    StreamEnded,
    #[error("could not start authentication: {0}")]
    StartRpc(connection::Failure),
    #[error("authentication stream failed: {0}")]
    StreamRpc(connection::Failure),
    #[error("{0}")]
    External(String),
}

impl Failure {
    pub(in crate::features::agent_auth) fn presentation(&self) -> String {
        match self {
            Self::MissingRequest => tr!("auth-empty-request").into(),
            Self::InvalidPayload(message) => message.clone(),
            Self::InputEnded => tr!("auth-ended").into(),
            Self::InputClosed => tr!("auth-connection-closed").into(),
            Self::StreamEnded => tr!("auth-disconnected").into(),
            Self::StartRpc(error) => rpc_error(tr!("auth-start-failed"), error),
            Self::StreamRpc(error) => rpc_error(tr!("auth-failed"), error),
            Self::External(message) => message.clone(),
        }
    }
}

fn rpc_error(action: &str, error: &connection::Failure) -> String {
    if error.code() == Some(tonic::Code::Unimplemented) {
        return tr!("auth-upgrade").into();
    }
    if let connection::Failure::Rpc { message, code, .. } = error {
        let message = message.trim();
        if message.is_empty() {
            tr!(
                "auth-rpc-error",
                action = (action).to_string(),
                code = format!("{code:?}")
            )
        } else {
            format!("{action}：{message}")
        }
    } else {
        format!("{action}：{}", error.presentation())
    }
}
