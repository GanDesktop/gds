//! Pure classification of timestamps into the labels a [`Format`] shows.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Local, Months, NaiveDate};

use super::Format;

/// What one timestamp reads as at a given moment.
///
/// The counters are unit counts, not wording; [`super::Timestamp`] localizes
/// them, and tests assert on these values rather than on translated text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Presentation {
    Missing,
    Epoch,
    JustNow,
    Minutes(u32),
    Today {
        time: String,
    },
    Yesterday {
        time: String,
    },
    Days(u32),
    Weeks(u32),
    Months(u32),
    Years(u32),
    /// Exact local time; used for the absolute format and for future values.
    Absolute {
        text: String,
    },
}

const ABSOLUTE_FORMAT: &str = "%Y-%m-%d %H:%M";
const CLOCK_FORMAT: &str = "%H:%M";
const PRECISE_FORMAT: &str = "%Y-%m-%d %H:%M:%S %:z";

/// Under this distance the minute count wins even across local midnight.
const MINUTE_GRANULARITY: Duration = Duration::from_secs(30 * 60);
const DAYS_UNTIL_WEEKS: u32 = 7;
const MONTHS_UNTIL_YEARS: u32 = 12;

impl Format {
    /// Classify `value` as it would read at `now`.
    ///
    /// `None` means the timestamp is unknown, while `Some(UNIX_EPOCH)` is a
    /// real value that renders as the epoch.
    pub fn presentation(self, value: Option<SystemTime>, now: SystemTime) -> Presentation {
        let Some(value) = value else {
            return Presentation::Missing;
        };
        match self {
            Self::Absolute => Presentation::Absolute {
                text: absolute(value),
            },
            Self::Relative if value == SystemTime::UNIX_EPOCH => Presentation::Epoch,
            Self::Relative => relative(value, now),
        }
    }
}

/// Exact local time with seconds and UTC offset, for tooltips.
pub fn precise(value: SystemTime) -> String {
    local(value).format(PRECISE_FORMAT).to_string()
}

fn local(value: SystemTime) -> DateTime<Local> {
    DateTime::<Local>::from(value)
}

fn absolute(value: SystemTime) -> String {
    local(value).format(ABSOLUTE_FORMAT).to_string()
}

fn relative(value: SystemTime, now: SystemTime) -> Presentation {
    let Ok(elapsed) = now.duration_since(value) else {
        // There is no wording for a negative distance, so a future value keeps
        // its exact local time instead of clamping to "just now".
        return Presentation::Absolute {
            text: absolute(value),
        };
    };
    if elapsed < MINUTE_GRANULARITY {
        let minutes = u32::try_from(elapsed.as_secs() / 60).unwrap_or(u32::MAX);
        return if minutes == 0 {
            Presentation::JustNow
        } else {
            Presentation::Minutes(minutes)
        };
    }

    let value = local(value);
    let date = value.date_naive();
    let today = local(now).date_naive();
    let time = value.format(CLOCK_FORMAT).to_string();
    if date == today {
        return Presentation::Today { time };
    }
    let days = u32::try_from((today - date).num_days()).unwrap_or(u32::MAX);
    if days == 1 {
        return Presentation::Yesterday { time };
    }
    let months = full_months(date, today);
    if months >= MONTHS_UNTIL_YEARS {
        Presentation::Years(months / MONTHS_UNTIL_YEARS)
    } else if months > 0 {
        Presentation::Months(months)
    } else if days >= DAYS_UNTIL_WEEKS {
        Presentation::Weeks(days / DAYS_UNTIL_WEEKS)
    } else {
        Presentation::Days(days)
    }
}

/// Whole calendar months between two local dates.
///
/// A month counts once its day of month is reached, clamping to the end of a
/// shorter month, so Jan 31 → Feb 28 (or Feb 29 in a leap year) is one month.
fn full_months(from: NaiveDate, to: NaiveDate) -> u32 {
    let months = (to.year() - from.year()) * 12 + (to.month0() as i32 - from.month0() as i32);
    let Ok(months) = u32::try_from(months) else {
        return 0;
    };
    if months == 0 {
        return 0;
    }
    if from
        .checked_add_months(Months::new(months))
        .is_some_and(|date| date <= to)
    {
        months
    } else {
        months - 1
    }
}
