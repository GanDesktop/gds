//! Presentation follows ACP categories and known identifiers, never translated labels.

use bas::acp::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOptions,
};
use gpui_kit::assets::IconName;

fn identifier(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn icon(option: &SessionConfigOption) -> Option<IconName> {
    match identifier(&option.id.to_string()).as_str() {
        "fastmode" => return Some(IconName::Zap),
        "approvalpolicy" | "approvalmode" | "permissions" | "permissionmode" => {
            return Some(IconName::Shield);
        }
        "collaborationmode" => return Some(IconName::Workflow),
        "mode" if permission_presets(option) => return Some(IconName::Shield),
        _ => {}
    }
    match option.category.as_ref() {
        Some(SessionConfigOptionCategory::Model) => Some(IconName::Sparkles),
        Some(SessionConfigOptionCategory::Mode) => Some(IconName::Workflow),
        Some(SessionConfigOptionCategory::ThoughtLevel) => Some(IconName::Lightbulb),
        Some(SessionConfigOptionCategory::Other(category))
            if identifier(category) == "collaborationmode" =>
        {
            Some(IconName::Workflow)
        }
        _ => match identifier(&option.id.to_string()).as_str() {
            "model" => Some(IconName::Sparkles),
            "mode" => Some(IconName::Workflow),
            "thinking" | "thoughtlevel" | "reasoningeffort" | "reasoninglevel" => {
                Some(IconName::Lightbulb)
            }
            _ => None,
        },
    }
}

/// Codex exposes approval/sandbox presets as the generic ACP `mode` selector.
/// Match its wire values so another agent's editing/planning modes keep their meaning.
fn permission_presets(option: &SessionConfigOption) -> bool {
    let SessionConfigKind::Select(select) = &option.kind else {
        return false;
    };
    let options: Vec<_> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect(),
        SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().flat_map(|group| &group.options).collect()
        }
        _ => return false,
    };
    options.iter().any(|option| {
        matches!(
            option.value.to_string().as_str(),
            "workspace-write" | "agent-full-access"
        )
    }) && options.iter().all(|option| {
        matches!(
            option.value.to_string().as_str(),
            "read-only" | "workspace-write" | "agent" | "agent-full-access"
        )
    })
}

pub(super) fn toggle_icon(option: &SessionConfigOption) -> Option<IconName> {
    if identifier(&option.id.to_string()) == "fastmode"
        || matches!(
            option.category,
            Some(SessionConfigOptionCategory::ThoughtLevel)
        )
    {
        icon(option)
    } else {
        None
    }
}

pub(super) fn description(option: &SessionConfigOption, hint: &str) -> String {
    match option
        .description
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        Some(description) => format!("{hint}\n{description}"),
        None => hint.to_owned(),
    }
}
