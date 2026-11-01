//! One relative-time policy for conversation history and other interface timestamps.

use bas::model::Timestamp;
use gpui_kit::App;
use std::time::{Duration, SystemTime};

pub(crate) fn recent_time(at: Timestamp, now: Timestamp, cx: &App) -> String {
    let at = SystemTime::UNIX_EPOCH + Duration::from_millis(at.millis());
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(now.millis());
    gds::ui::time::Format::Relative
        .presentation(Some(at), now)
        .text(cx)
        .to_string()
}

pub(crate) fn relative_age(age: Duration, cx: &App) -> String {
    let now = SystemTime::now();
    gds::ui::time::Format::Relative
        .presentation(now.checked_sub(age), now)
        .text(cx)
        .to_string()
}

pub(crate) fn parse_rfc3339(value: &str) -> Option<Timestamp> {
    let at =
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()?;
    let millis = u64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok()?;
    Some(Timestamp::from_millis(millis))
}
