//! Limits failed sign-in attempts, as Laravel's `throttle` middleware does on the
//! login route. Each failed attempt counts against its email and its IP. When
//! either count reaches its limit inside the window, sign-in is refused with
//! 429 until the window ends. The counts live in the `login_throttles` table, so
//! they hold across restarts and across processes.

use chrono::{Duration, NaiveDateTime, Utc};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::QueryResult;
use diesel::sql_query;
use diesel::sql_types::{Datetime, Text};

/// The failed attempts allowed per email in one window.
pub(crate) const EMAIL_LIMIT: i64 = 5;
/// The failed attempts allowed per IP address in one window.
pub(crate) const IP_LIMIT: i64 = 20;
/// The length of a window, in seconds.
pub(crate) const WINDOW_SECONDS: i64 = 60;

diesel::table! {
    /// The login throttle counts. See `migrations/20261108020000_create_login_throttles`.
    login_throttles (throttle_key) {
        throttle_key -> Text,
        hits -> Integer,
        window_start -> Timestamp,
    }
}

/// The key for an email. Emails differ only in case, so they count as one.
pub(crate) fn email_key(email: &str) -> String {
    format!("email:{}", email.trim().to_lowercase())
}

/// The key for an IP address, or for requests whose address is unknown.
pub(crate) fn ip_key(ip: Option<std::net::IpAddr>) -> String {
    match ip {
        Some(ip) => format!("ip:{ip}"),
        None => "ip:unknown".to_string(),
    }
}

/// How many seconds until `key` may try again, if it is at its limit.
pub(crate) fn retry_after(
    conn: &mut MysqlConnection,
    key: &str,
    limit: i64,
) -> QueryResult<Option<i64>> {
    let now = Utc::now().naive_utc();
    let row: Option<(i64, NaiveDateTime)> = login_throttles::table
        .filter(login_throttles::throttle_key.eq(key))
        .select((login_throttles::hits, login_throttles::window_start))
        .first::<(i32, NaiveDateTime)>(conn)
        .optional()?
        .map(|(hits, start)| (i64::from(hits), start));
    let Some((hits, start)) = row else {
        return Ok(None);
    };
    let ends = start + Duration::seconds(WINDOW_SECONDS);
    if ends <= now || hits < limit {
        return Ok(None);
    }
    Ok(Some((ends - now).num_seconds().max(1)))
}

/// Counts one failed attempt against `key`. A window that has ended starts again at 1.
pub(crate) fn record_failure(conn: &mut MysqlConnection, key: &str) -> QueryResult<()> {
    let now = Utc::now().naive_utc();
    let cutoff = now - Duration::seconds(WINDOW_SECONDS);
    sql_query(
        "INSERT INTO login_throttles (throttle_key, hits, window_start) VALUES (?, 1, ?) \
         ON DUPLICATE KEY UPDATE \
         hits = IF(window_start < ?, 1, hits + 1), \
         window_start = IF(window_start < ?, ?, window_start)",
    )
    .bind::<Text, _>(key)
    .bind::<Datetime, _>(now)
    .bind::<Datetime, _>(cutoff)
    .bind::<Datetime, _>(cutoff)
    .bind::<Datetime, _>(now)
    .execute(conn)
    .map(|_| ())
}

/// Clears the count for `key`, after a successful sign-in.
pub(crate) fn clear(conn: &mut MysqlConnection, key: &str) -> QueryResult<usize> {
    diesel::delete(login_throttles::table.filter(login_throttles::throttle_key.eq(key)))
        .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_count_regardless_of_case_and_spacing() {
        assert_eq!(
            email_key(" Ada@Example.TEST "),
            email_key("ada@example.test")
        );
    }

    #[test]
    fn unknown_addresses_share_one_key() {
        assert_eq!(ip_key(None), "ip:unknown");
        assert_eq!(
            ip_key(Some("203.0.113.9".parse().unwrap())),
            "ip:203.0.113.9"
        );
    }
}
