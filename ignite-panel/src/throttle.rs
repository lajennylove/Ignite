//! Limits failed attempts, as Laravel's `throttle` middleware does on the login
//! route. The key is the email and the client's IP together, as Laravel's login
//! throttle keys it, so one address cannot lock out an email for everyone, and one
//! email cannot be guessed without limit from one address. A failed sign-in and a
//! wrong current password on the profile both count against the key. When the count
//! reaches the limit inside the window, the attempt is refused with 429 until the
//! window ends. The counts live in the `login_throttles` table, so they hold across
//! restarts and across processes. Rows whose window has ended are deleted on each
//! failure, so the table holds only live windows.

use chrono::{Duration, NaiveDateTime, Utc};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::QueryResult;
use diesel::sql_query;
use diesel::sql_types::{Datetime, Text};

/// The failed attempts allowed per key (an email and an IP) in one window.
pub(crate) const LIMIT: i64 = 5;
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

/// The key for an email and a client address: the lowercased, trimmed email, a `|`, and
/// the IP address, as Laravel's login throttle builds it. Emails differ only in case, so
/// they count as one. A request whose address is unknown shares the key `unknown`.
pub(crate) fn key(email: &str, ip: Option<std::net::IpAddr>) -> String {
    let ip = match ip {
        Some(ip) => ip.to_string(),
        None => "unknown".to_string(),
    };
    format!("{}|{ip}", email.trim().to_lowercase())
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
/// Then deletes every row whose window has ended, for any key, so the table does not
/// grow with every email that was ever tried.
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
    .execute(conn)?;
    prune(conn, cutoff)?;
    Ok(())
}

/// Deletes the rows whose window started before `cutoff`, which have ended. Uses the
/// index on `window_start`.
fn prune(conn: &mut MysqlConnection, cutoff: NaiveDateTime) -> QueryResult<usize> {
    diesel::delete(login_throttles::table.filter(login_throttles::window_start.lt(cutoff)))
        .execute(conn)
}

/// Clears the count for `key`, after a successful sign-in or password change.
pub(crate) fn clear(conn: &mut MysqlConnection, key: &str) -> QueryResult<usize> {
    diesel::delete(login_throttles::table.filter(login_throttles::throttle_key.eq(key)))
        .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_count_regardless_of_case_and_spacing() {
        let ip = Some("203.0.113.9".parse().unwrap());
        assert_eq!(key(" Ada@Example.TEST ", ip), key("ada@example.test", ip));
    }

    #[test]
    fn the_key_pairs_the_email_with_the_address() {
        let ip: Option<std::net::IpAddr> = Some("203.0.113.9".parse().unwrap());
        assert_eq!(key("ada@example.test", ip), "ada@example.test|203.0.113.9");
        assert_ne!(
            key("ada@example.test", ip),
            key("ada@example.test", Some("203.0.113.10".parse().unwrap()))
        );
        assert_eq!(key("ada@example.test", None), "ada@example.test|unknown");
    }
}
