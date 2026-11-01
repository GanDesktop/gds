use std::time::{Duration, SystemTime};

use chrono::{Local, TimeZone};

use super::{Format, Presentation, precise};

/// A local wall clock time, so the tests hold in every time zone.
fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> SystemTime {
    Local
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .expect("the fixture wall clock time exists in this time zone")
        .into()
}

fn relative(value: SystemTime, now: SystemTime) -> Presentation {
    Format::Relative.presentation(Some(value), now)
}

#[test]
fn unknown_values_and_the_epoch_are_distinct() {
    let now = at(2026, 3, 10, 12, 0);
    assert_eq!(
        Format::Relative.presentation(None, now),
        Presentation::Missing
    );
    assert_eq!(
        Format::Absolute.presentation(None, now),
        Presentation::Missing
    );
    assert_eq!(
        Format::Relative.presentation(Some(SystemTime::UNIX_EPOCH), now),
        Presentation::Epoch
    );
    assert!(matches!(
        Format::Absolute.presentation(Some(SystemTime::UNIX_EPOCH), now),
        Presentation::Absolute { .. }
    ));
    assert_eq!(
        Format::Absolute.presentation(Some(now), now),
        Presentation::Absolute {
            text: "2026-03-10 12:00".to_owned()
        }
    );
    assert!(precise(now).starts_with("2026-03-10 12:00:00 "));
}

#[test]
fn minute_counts_stop_below_thirty_minutes() {
    let now = at(2026, 3, 10, 12, 30);
    assert_eq!(relative(now, now), Presentation::JustNow);
    assert_eq!(
        relative(now - Duration::from_secs(59), now),
        Presentation::JustNow
    );
    assert_eq!(
        relative(now - Duration::from_secs(60), now),
        Presentation::Minutes(1)
    );
    assert_eq!(
        relative(now - Duration::from_secs(29 * 60 + 59), now),
        Presentation::Minutes(29)
    );
    assert_eq!(
        relative(now - Duration::from_secs(30 * 60), now),
        Presentation::Today {
            time: "12:00".to_owned()
        }
    );
}

#[test]
fn crossing_midnight_prefers_minutes_before_today_and_yesterday() {
    let midnight = at(2026, 3, 10, 0, 20);
    assert_eq!(
        relative(at(2026, 3, 9, 23, 55), midnight),
        Presentation::Minutes(25)
    );

    let now = at(2026, 3, 10, 12, 0);
    assert_eq!(
        relative(at(2026, 3, 10, 9, 15), now),
        Presentation::Today {
            time: "09:15".to_owned()
        }
    );
    assert_eq!(
        relative(at(2026, 3, 9, 9, 15), now),
        Presentation::Yesterday {
            time: "09:15".to_owned()
        }
    );
}

#[test]
fn older_values_use_whole_weeks_months_and_years() {
    let now = at(2026, 3, 10, 12, 0);
    assert_eq!(relative(at(2026, 3, 8, 12, 0), now), Presentation::Days(2));
    assert_eq!(relative(at(2026, 3, 3, 12, 0), now), Presentation::Weeks(1));
    assert_eq!(relative(at(2026, 3, 2, 12, 0), now), Presentation::Weeks(1));
    assert_eq!(
        relative(at(2026, 2, 10, 12, 0), now),
        Presentation::Months(1)
    );
    assert_eq!(
        relative(at(2026, 1, 10, 12, 0), now),
        Presentation::Months(2)
    );
    assert_eq!(
        relative(at(2025, 4, 10, 12, 0), now),
        Presentation::Months(11)
    );
    assert_eq!(
        relative(at(2025, 3, 10, 12, 0), now),
        Presentation::Years(1)
    );
    assert_eq!(
        relative(at(2024, 3, 10, 12, 0), now),
        Presentation::Years(2)
    );
}

#[test]
fn month_end_and_leap_anniversaries_count_as_whole_months() {
    assert_eq!(
        relative(at(2026, 1, 31, 12, 0), at(2026, 2, 28, 12, 0)),
        Presentation::Months(1)
    );
    assert_eq!(
        relative(at(2025, 1, 31, 12, 0), at(2025, 2, 27, 12, 0)),
        Presentation::Weeks(3)
    );
    assert_eq!(
        relative(at(2024, 1, 31, 12, 0), at(2024, 2, 29, 12, 0)),
        Presentation::Months(1)
    );
    assert_eq!(
        relative(at(2024, 1, 29, 12, 0), at(2024, 2, 29, 12, 0)),
        Presentation::Months(1)
    );
}

#[test]
fn future_values_fall_back_to_the_exact_local_time() {
    let now = at(2026, 3, 10, 12, 0);
    assert_eq!(
        relative(at(2026, 3, 10, 13, 30), now),
        Presentation::Absolute {
            text: "2026-03-10 13:30".to_owned()
        }
    );
    assert_eq!(
        relative(now + Duration::from_secs(5), now),
        Presentation::Absolute {
            text: "2026-03-10 12:00".to_owned()
        }
    );
}
