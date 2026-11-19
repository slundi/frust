use chrono::{DateTime, Utc};
use twox_hash::XxHash3_64;

/// Storage id of a feed entry, scoped to its feed so two feeds publishing the
/// same GUID (YouTube playlists, planets, "all" + "category" feeds, `1`, `2`…)
/// never share — and overwrite — the same article.
pub(crate) fn article_id(feed_id: u64, entry_id: &str) -> u64 {
    scope_guid_hash(feed_id, XxHash3_64::oneshot(entry_id.as_bytes()))
}

/// Combine a feed id with the XXH3 hash of an entry GUID.
///
/// Built from the GUID *hash* rather than the GUID itself so that legacy ids
/// (plain `xxh3(guid)`, schema v1) can be migrated without the original GUID.
pub(crate) fn scope_guid_hash(feed_id: u64, guid_hash: u64) -> u64 {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&feed_id.to_le_bytes());
    buf[8..].copy_from_slice(&guid_hash.to_le_bytes());
    XxHash3_64::oneshot(&buf)
}

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
    fn test_article_id_is_stable() {
        assert_eq!(article_id(1, "guid"), article_id(1, "guid"));
    }

    #[test]
    fn test_article_id_differs_across_feeds_for_same_guid() {
        assert_ne!(article_id(1, "guid"), article_id(2, "guid"));
    }

    #[test]
    fn test_article_id_differs_across_guids_in_same_feed() {
        assert_ne!(article_id(1, "a"), article_id(1, "b"));
    }

    #[test]
    fn test_article_id_matches_scoped_legacy_hash() {
        // Migration relies on this: a v1 id (xxh3(guid)) scoped with its
        // feed_id must equal the id computed from the GUID directly.
        let legacy = XxHash3_64::oneshot(b"guid");
        assert_eq!(scope_guid_hash(7, legacy), article_id(7, "guid"));
    }

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
