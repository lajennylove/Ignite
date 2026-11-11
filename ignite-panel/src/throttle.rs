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
use orbit_orm::{raw, Database, Error, Executor, Model, Operator, OrbitModel};

/// The failed attempts allowed per key (an email and an IP) in one window.
pub(crate) const LIMIT: i64 = 5;
/// The length of a window, in seconds.
pub(crate) const WINDOW_SECONDS: i64 = 60;

/// A row of the `login_throttles` table. See `migrations/20261108020000_create_login_throttles`.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "login_throttles", primary_key = "throttle_key")]
#[allow(dead_code)]
pub(crate) struct LoginThrottle {
    pub throttle_key: String,
    pub hits: i32,
    pub window_start: NaiveDateTime,
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

/// The key for a password change: the same email and address as sign-in, under its own
/// prefix. A successful change clears only this key, so it never clears the sign-in count.
pub(crate) fn password_key(email: &str, ip: Option<std::net::IpAddr>) -> String {
    format!("password|{}", key(email, ip))
}

/// How many seconds until `key` may try again, if it is at its limit.
pub(crate) async fn retry_after(
    db: &Database,
    key: &str,
    limit: i64,
) -> Result<Option<i64>, Error> {
    let now = Utc::now().naive_utc();
    let row: Option<(i32, NaiveDateTime)> = LoginThrottle::query()
        .where_eq("throttle_key", key)
        .select(&["hits", "window_start"])
        .first_as(db)
        .await?;
    let Some((hits, start)) = row else {
        return Ok(None);
    };
    let hits = i64::from(hits);
    let ends = start + Duration::seconds(WINDOW_SECONDS);
    if ends <= now || hits < limit {
        return Ok(None);
    }
    Ok(Some((ends - now).num_seconds().max(1)))
}

/// Counts one failed attempt against `key`. A window that has ended starts again at 1.
/// Then deletes every row whose window has ended, for any key, so the table does not
/// grow with every email that was ever tried.
pub(crate) async fn record_failure(db: &Database, key: &str) -> Result<(), Error> {
    let now = Utc::now().naive_utc();
    let cutoff = now - Duration::seconds(WINDOW_SECONDS);
    raw::execute(
        db,
        "INSERT INTO login_throttles (throttle_key, hits, window_start) VALUES (?, 1, ?) \
         ON DUPLICATE KEY UPDATE \
         hits = IF(window_start < ?, 1, hits + 1), \
         window_start = IF(window_start < ?, ?, window_start)",
        &[
            key.into(),
            now.into(),
            cutoff.into(),
            cutoff.into(),
            now.into(),
        ],
    )
    .await?;
    prune(db, cutoff).await?;
    Ok(())
}

/// Deletes the rows whose window started before `cutoff`, which have ended. Uses the
/// index on `window_start`.
async fn prune<'a>(db: impl Executor<'a>, cutoff: NaiveDateTime) -> Result<u64, Error> {
    LoginThrottle::query()
        .where_op("window_start", Operator::Lt, cutoff)
        .delete(db)
        .await
}

/// Clears the count for `key`, after a successful sign-in or password change.
pub(crate) async fn clear<'a>(db: impl Executor<'a>, key: &str) -> Result<u64, Error> {
    LoginThrottle::query()
        .where_eq("throttle_key", key)
        .delete(db)
        .await
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
    fn the_password_key_is_separate_from_the_sign_in_key() {
        let ip: Option<std::net::IpAddr> = Some("203.0.113.9".parse().unwrap());
        assert_ne!(
            password_key("ada@example.test", ip),
            key("ada@example.test", ip)
        );
        assert_eq!(
            password_key("Ada@example.test", ip),
            "password|ada@example.test|203.0.113.9"
        );
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
