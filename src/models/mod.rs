pub mod _entities;
#[cfg(feature = "bench")]
pub mod bench_events;
pub mod cast;
pub mod sessions;
pub mod tokens;
pub mod users;

use sea_orm::prelude::DateTimeWithTimeZone;

/// A timestamp the way Rails' `as_json` writes it: UTC, milliseconds, `Z`
/// (`2026-09-29T16:15:24.391Z`), whatever offset and precision the column holds.
#[must_use]
pub fn as_json_time(time: &DateTimeWithTimeZone) -> String {
    time.with_timezone(&chrono::Utc)
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_json_time_is_utc_with_milliseconds() {
        let t =
            DateTimeWithTimeZone::parse_from_rfc3339("2026-09-29T11:15:24.391876-05:00").unwrap();
        assert_eq!(as_json_time(&t), "2026-09-29T16:15:24.391Z");
        let t = DateTimeWithTimeZone::parse_from_rfc3339("2025-08-01T12:00:00+00:00").unwrap();
        assert_eq!(as_json_time(&t), "2025-08-01T12:00:00.000Z");
    }
}
pub mod accounts;
pub mod invitations;
pub mod memberships;
