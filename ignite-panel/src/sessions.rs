//! The `sessions` table: one row per sign-in. A signed cookie holds the row's
//! id, and the guard checks the row on each request, so a session can be ended
//! on the server, from logout or from "sign out everywhere".

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, NaiveDateTime, Utc};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::QueryResult;
use rand_core::{OsRng, RngCore};

diesel::table! {
    /// The sessions table. See `migrations/20261108010000_create_sessions`.
    sessions (id) {
        id -> Text,
        user_id -> BigInt,
        expires_at -> Timestamp,
        created_at -> Timestamp,
    }
}

/// How long a sign-in lasts: eight hours.
const LIFETIME_HOURS: i64 = 8;

/// Starts a session for `user_id` and returns its id. Expired rows are removed on the way.
pub(crate) fn create(conn: &mut MysqlConnection, user_id: i64) -> QueryResult<String> {
    let now = Utc::now().naive_utc();
    diesel::delete(sessions::table.filter(sessions::expires_at.lt(now))).execute(conn)?;

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let id = URL_SAFE_NO_PAD.encode(bytes);
    diesel::insert_into(sessions::table)
        .values((
            sessions::id.eq(&id),
            sessions::user_id.eq(user_id),
            sessions::expires_at.eq(now + Duration::hours(LIFETIME_HOURS)),
            sessions::created_at.eq(now),
        ))
        .execute(conn)?;
    Ok(id)
}

/// The user of an unexpired session, or `None` if there is no such session.
pub(crate) fn user_for(conn: &mut MysqlConnection, id: &str) -> QueryResult<Option<i64>> {
    let now: NaiveDateTime = Utc::now().naive_utc();
    sessions::table
        .filter(sessions::id.eq(id))
        .filter(sessions::expires_at.gt(now))
        .select(sessions::user_id)
        .first(conn)
        .optional()
}

/// Ends one session. Returns the number of rows removed.
pub(crate) fn delete(conn: &mut MysqlConnection, id: &str) -> QueryResult<usize> {
    diesel::delete(sessions::table.filter(sessions::id.eq(id))).execute(conn)
}

/// Ends every session of `user_id`, on every device. Returns the number removed.
pub(crate) fn delete_for_user(conn: &mut MysqlConnection, user_id: i64) -> QueryResult<usize> {
    diesel::delete(sessions::table.filter(sessions::user_id.eq(user_id))).execute(conn)
}

/// Ends every session of `user_id` except `keep`, the one the user is using. Returns the number removed.
pub(crate) fn delete_others_for_user(
    conn: &mut MysqlConnection,
    user_id: i64,
    keep: &str,
) -> QueryResult<usize> {
    diesel::delete(
        sessions::table
            .filter(sessions::user_id.eq(user_id))
            .filter(sessions::id.ne(keep)),
    )
    .execute(conn)
}
