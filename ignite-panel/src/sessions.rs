//! The `sessions` table: one row per sign-in. A signed cookie holds the row's
//! id, and the guard checks the row on each request, so a session can be ended
//! on the server, from logout or from "sign out everywhere".

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, NaiveDateTime, Utc};
use orbit_orm::{Database, Error, Executor, Model, Operator, OrbitModel};
use rand_core::{OsRng, RngCore};

/// A row of the `sessions` table. See `migrations/20261108010000_create_sessions`.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "sessions", primary_key = "id")]
#[allow(dead_code)]
pub(crate) struct Session {
    #[orbit(fillable)]
    pub id: String,
    #[orbit(fillable)]
    pub user_id: i64,
    #[orbit(fillable)]
    pub expires_at: NaiveDateTime,
    #[orbit(fillable)]
    pub created_at: NaiveDateTime,
}

/// How long a sign-in lasts: eight hours.
const LIFETIME_HOURS: i64 = 8;

/// Starts a session for `user_id` and returns its id. Expired rows are removed on the way.
pub(crate) async fn create(db: &Database, user_id: i64) -> Result<String, Error> {
    let now = Utc::now().naive_utc();
    Session::query()
        .where_op("expires_at", Operator::Lt, now)
        .delete(db)
        .await?;

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let id = URL_SAFE_NO_PAD.encode(bytes);
    Session::query()
        .insert(
            db,
            &[
                ("id", id.as_str().into()),
                ("user_id", user_id.into()),
                ("expires_at", (now + Duration::hours(LIFETIME_HOURS)).into()),
                ("created_at", now.into()),
            ],
        )
        .await?;
    Ok(id)
}

/// The user of an unexpired session, or `None` if there is no such session.
pub(crate) async fn user_for<'a>(db: impl Executor<'a>, id: &str) -> Result<Option<i64>, Error> {
    let now: NaiveDateTime = Utc::now().naive_utc();
    let row: Option<(i64,)> = Session::query()
        .where_eq("id", id)
        .where_op("expires_at", Operator::Gt, now)
        .select(&["user_id"])
        .first_as(db)
        .await?;
    Ok(row.map(|(user_id,)| user_id))
}

/// Ends one session. Returns the number of rows removed.
pub(crate) async fn delete<'a>(db: impl Executor<'a>, id: &str) -> Result<u64, Error> {
    Session::query().where_eq("id", id).delete(db).await
}

/// Ends every session of `user_id`, on every device. Returns the number removed.
pub(crate) async fn delete_for_user<'a>(db: impl Executor<'a>, user_id: i64) -> Result<u64, Error> {
    Session::query()
        .where_eq("user_id", user_id)
        .delete(db)
        .await
}

/// Ends every session of `user_id` except `keep`, the one the user is using. Returns the number removed.
pub(crate) async fn delete_others_for_user<'a>(
    db: impl Executor<'a>,
    user_id: i64,
    keep: &str,
) -> Result<u64, Error> {
    Session::query()
        .where_eq("user_id", user_id)
        .where_op("id", Operator::Ne, keep)
        .delete(db)
        .await
}
