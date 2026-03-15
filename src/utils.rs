use chrono::{DateTime, Utc};

/// Check if the time elapsed since the last check is greater than the required interval
pub(crate) fn is_refresh_required(
    last_check: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    interval: i64,
) -> bool {
    match last_check {
        Some(time) => now.signed_duration_since(time).num_seconds() >= interval,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn test_is_refresh_required() {
        let now = Utc::now();
        let interval_seconds = 600; // 10 minutes

        // Scenario 1: The feed has never been checked (last_check is None)
        // It should always return true to ensure the first fetch happens.
        let last_check_none = None;
        assert!(
            is_refresh_required(last_check_none, now, interval_seconds),
            "Should require refresh when last_check is None"
        );

        // Scenario 2: The feed was checked recently (e.g., 5 minutes ago)
        // Since 5 mins < 10 mins, it should return false.
        let last_check_recent = Some(now - Duration::minutes(5));
        assert!(
            !is_refresh_required(last_check_recent, now, interval_seconds),
            "Should NOT require refresh when checked 5 minutes ago (interval is 10m)"
        );

        // Scenario 3: The feed was checked a long time ago (e.g., 15 minutes ago)
        // Since 15 mins > 10 mins, it should return true.
        let last_check_old = Some(now - Duration::minutes(15));
        assert!(
            is_refresh_required(last_check_old, now, interval_seconds),
            "Should require refresh when checked 15 minutes ago"
        );

        // Scenario 4: Exact boundary condition
        // The time elapsed is exactly equal to the interval.
        // Using '>=' ensures we refresh exactly on time.
        let last_check_exact = Some(now - Duration::seconds(interval_seconds));
        assert!(
            is_refresh_required(last_check_exact, now, interval_seconds),
            "Should require refresh when elapsed time is exactly equal to the interval"
        );

        // Scenario 5: Future timestamp (Safety check)
        // If for some reason last_check is in the future, it should not refresh
        // unless the logic handles negative durations.
        let last_check_future = Some(now + Duration::minutes(5));
        assert!(
            !is_refresh_required(last_check_future, now, interval_seconds),
            "Should NOT require refresh if last_check is in the future"
        );

        // Scenario 6: same timestamps
        assert!(is_refresh_required(Some(now), now, 0));
    }
}
