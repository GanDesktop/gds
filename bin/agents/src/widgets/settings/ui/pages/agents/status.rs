use crate::entities::agent::Status;
use crate::shared::i18n::tr;

pub(super) fn label(status: &Status) -> &'static str {
    match status {
        Status::Checking => tr!("agent-checking-program"),
        Status::Connecting => tr!("agent-checking"),
        Status::Authenticating => tr!("auth-progress"),
        Status::Installed(_) => tr!("agent-unchecked"),
        Status::Connected(_) => tr!("agent-check-passed"),
        Status::AuthenticationRequired { .. } => tr!("auth-required"),
        Status::Missing(_) => tr!("agent-program-missing"),
        Status::Failed(_) => tr!("agent-check-failed"),
    }
}

pub(super) fn check_label(status: &Status) -> &'static str {
    match status {
        Status::Checking | Status::Connecting => tr!("agent-checking"),
        Status::Authenticating => tr!("agent-check-again"),
        Status::Installed(_) => tr!("agent-check"),
        _ => tr!("agent-check-again"),
    }
}
