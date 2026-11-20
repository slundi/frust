use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use twox_hash::XxHash3_64;

/// Default directory for the databases, following the XDG base directory
/// spec: `$XDG_STATE_HOME/frust`, else `$HOME/.local/state/frust`, else
/// `/var/lib/frust` (service accounts without a home). Relative values are
/// ignored, as the spec requires.
pub(crate) fn default_data_dir(xdg_state_home: Option<&str>, home: Option<&str>) -> PathBuf {
    let absolute = |value: Option<&str>| {
        value
            .filter(|v| Path::new(v).is_absolute())
            .map(PathBuf::from)
    };
    if let Some(state_home) = absolute(xdg_state_home) {
        return state_home.join("frust");
    }
    if let Some(home) = absolute(home) {
        return home.join(".local/state/frust");
    }
    PathBuf::from("/var/lib/frust")
}

/// Double-quote a YAML scalar. Escapes `\`, `"`, and control and line
/// separator characters: a raw newline in a feed title would end the string
/// and let the feed inject keys into the generated YAML.
pub(crate) fn yaml_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\u{2028}' | '\u{2029}' => quoted.push_str(&format!("\\u{:04x}", character as u32)),
            other if other.is_control() => quoted.push_str(&format!("\\u{:04x}", other as u32)),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

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
    fn test_default_data_dir_prefers_xdg_state_home() {
        assert_eq!(
            default_data_dir(Some("/state"), Some("/home/me")),
            PathBuf::from("/state/frust")
        );
    }

    #[test]
    fn test_default_data_dir_falls_back_to_home() {
        assert_eq!(
            default_data_dir(None, Some("/home/me")),
            PathBuf::from("/home/me/.local/state/frust")
        );
    }

    #[test]
    fn test_default_data_dir_ignores_relative_values() {
        assert_eq!(
            default_data_dir(Some("state"), Some("/home/me")),
            PathBuf::from("/home/me/.local/state/frust")
        );
        assert_eq!(
            default_data_dir(Some(""), Some("home")),
            PathBuf::from("/var/lib/frust")
        );
    }

    #[test]
    fn test_default_data_dir_without_env_uses_var_lib() {
        assert_eq!(
            default_data_dir(None, None),
            PathBuf::from("/var/lib/frust")
        );
    }

    // ---- yaml_quote (B15) ----

    fn yaml_round_trip(value: &str) -> String {
        use saphyr::{LoadableYamlNode, Yaml};
        let doc = format!("key: {}\nnext: 1\n", yaml_quote(value));
        let docs = Yaml::load_from_str(&doc).unwrap_or_else(|e| panic!("{e}: {doc}"));
        let map = docs[0].as_mapping().unwrap();
        assert_eq!(map.len(), 2, "no key injected: {doc}");
        docs[0]["key"].as_str().unwrap().to_string()
    }

    #[test]
    fn test_yaml_quote_round_trips_special_characters() {
        for value in [
            "plain",
            "C:\\foo\\",
            "Say \"hi\"",
            "a: b # not a comment",
            "line\ninjected: true",
            "cr\rtab\tend",
            "bell\u{7}nul\u{0}",
            "sep\u{2028}para\u{2029}",
            "Été à 5 € 🎸",
            "",
        ] {
            assert_eq!(yaml_round_trip(value), value);
        }
    }

    #[test]
    fn test_yaml_quote_keeps_output_on_one_line() {
        assert_eq!(yaml_quote("a\nb"), "\"a\\nb\"");
        assert!(!yaml_quote("x\r\n\u{2028}y").contains(['\n', '\r', '\u{2028}']));
    }

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
