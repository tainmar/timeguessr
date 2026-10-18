//! Deadline and "D-n" reminder scheduling in the organizer's time zone.

use chrono::{DateTime, Duration, LocalResult, TimeZone, Utc};
use chrono_tz::Tz;

/// Reminders go out at this local hour, n days before the deadline's date.
pub const REMINDER_LOCAL_HOUR: u32 = 10;
pub const MAX_REMINDER_DAYS: i32 = 30;

pub fn parse_tz(name: &str) -> Option<Tz> {
    name.parse().ok()
}

pub fn tz_or_utc(name: &str) -> Tz {
    parse_tz(name).unwrap_or(chrono_tz::UTC)
}

/// When the "D-n" reminder should be sent: 10:00 local time on the date n days before
/// the deadline, and never later than 2 hours before the deadline (matters for D-0).
pub fn reminder_send_at(deadline: DateTime<Utc>, tz: Tz, days_before: i32) -> DateTime<Utc> {
    let date = deadline.with_timezone(&tz).date_naive() - Duration::days(days_before as i64);
    let naive = date
        .and_hms_opt(REMINDER_LOCAL_HOUR, 0, 0)
        .expect("valid wall clock time");
    let local = match tz.from_local_datetime(&naive) {
        LocalResult::Single(t) => t,
        LocalResult::Ambiguous(first, _) => first,
        // 10:00 falls in a DST gap (no real zone does this, but be safe).
        LocalResult::None => tz
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .expect("valid time after DST gap"),
    };
    local.with_timezone(&Utc).min(deadline - Duration::hours(2))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedReminder {
    pub days_before: i32,
    pub send_at: DateTime<Utc>,
    /// True when the send time is already past (or too close to `not_before`):
    /// stored for transparency but never sent.
    pub skipped: bool,
}

/// Plan the reminders for a deadline. Anything scheduled before `not_before` is marked
/// skipped so that starting (or extending) a collection never blasts stale reminders.
pub fn plan_reminders(
    deadline: DateTime<Utc>,
    tz: Tz,
    days: &[i32],
    not_before: DateTime<Utc>,
) -> Vec<PlannedReminder> {
    let mut days: Vec<i32> = days
        .iter()
        .copied()
        .filter(|d| (0..=MAX_REMINDER_DAYS).contains(d))
        .collect();
    days.sort_unstable_by(|a, b| b.cmp(a));
    days.dedup();
    days.into_iter()
        .map(|days_before| {
            let send_at = reminder_send_at(deadline, tz, days_before);
            PlannedReminder {
                days_before,
                send_at,
                skipped: send_at <= not_before,
            }
        })
        .collect()
}

/// Human readable deadline, e.g. "Friday 24 October 2026 at 18:00 (CEST)".
pub fn format_deadline(deadline: DateTime<Utc>, tz: Tz) -> String {
    deadline
        .with_timezone(&tz)
        .format("%A %-d %B %Y at %H:%M (%Z)")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn paris() -> Tz {
        parse_tz("Europe/Paris").unwrap()
    }

    #[test]
    fn reminders_are_sent_at_ten_local_time() {
        // Saturday 24 Oct 2026 18:00 Paris (CEST, UTC+2) = 16:00 UTC
        let deadline = Utc.with_ymd_and_hms(2026, 10, 24, 16, 0, 0).unwrap();
        // D-1: Friday 23 Oct 10:00 CEST = 08:00 UTC
        assert_eq!(
            reminder_send_at(deadline, paris(), 1),
            Utc.with_ymd_and_hms(2026, 10, 23, 8, 0, 0).unwrap()
        );
        // D-7 crosses nothing; D-0 is the deadline day at 10:00 local.
        assert_eq!(
            reminder_send_at(deadline, paris(), 0),
            Utc.with_ymd_and_hms(2026, 10, 24, 8, 0, 0).unwrap()
        );
    }

    #[test]
    fn reminders_follow_dst_changes() {
        // Deadline Tuesday 27 Oct 2026 18:00 CET (UTC+1) = 17:00 UTC; DST ended on the 25th.
        let deadline = Utc.with_ymd_and_hms(2026, 10, 27, 17, 0, 0).unwrap();
        // D-3: Saturday 24 Oct 10:00 CEST (UTC+2) = 08:00 UTC
        assert_eq!(
            reminder_send_at(deadline, paris(), 3),
            Utc.with_ymd_and_hms(2026, 10, 24, 8, 0, 0).unwrap()
        );
    }

    #[test]
    fn same_day_reminder_never_after_deadline() {
        // Deadline at 09:00 local: D-0 would be 10:00, after the deadline.
        let deadline = Utc.with_ymd_and_hms(2026, 10, 24, 7, 0, 0).unwrap();
        assert_eq!(
            reminder_send_at(deadline, paris(), 0),
            deadline - Duration::hours(2)
        );
    }

    #[test]
    fn plan_skips_past_and_dedupes() {
        let deadline = Utc.with_ymd_and_hms(2026, 10, 24, 16, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap();
        let plan = plan_reminders(deadline, paris(), &[1, 7, 3, 1, 99, -1], now);
        let days: Vec<_> = plan.iter().map(|p| (p.days_before, p.skipped)).collect();
        assert_eq!(days, vec![(7, true), (3, true), (1, false)]);
    }

    #[test]
    fn formats_deadline_in_zone() {
        let deadline = Utc.with_ymd_and_hms(2026, 10, 24, 16, 0, 0).unwrap();
        assert_eq!(
            format_deadline(deadline, paris()),
            "Saturday 24 October 2026 at 18:00 (CEST)"
        );
    }
}
