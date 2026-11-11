//! The `users` table the panel signs in with: its schema, argon2 password
//! hashes, profile pictures, and the lookups the login and the CLI need.

use chrono::{NaiveDateTime, Utc};
use orbit_orm::{Database, Error, Executor, Model, Operator, OrbitModel, Transaction, Value};

use crate::sessions;

/// A row of the `users` table. See `migrations/20261108000000_create_users`.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "users")]
pub struct User {
    /// The primary key.
    pub id: i64,
    /// The display name.
    #[orbit(fillable)]
    pub name: String,
    /// The login email. Unique.
    #[orbit(fillable)]
    pub email: String,
    /// The argon2 hash in PHC string form. Never shown or logged.
    #[orbit(fillable)]
    pub password_hash: String,
    /// When the row was created.
    pub created_at: NaiveDateTime,
    /// The colour theme the panel shows this user: "light" or "dark".
    #[orbit(fillable)]
    pub theme: String,
    /// The type of the profile picture, such as "image/png". `None` shows the initials.
    #[orbit(fillable)]
    pub avatar_mime: Option<String>,
    /// The values of the profile fields the app adds, as a JSON object. `None` when unset.
    #[orbit(fillable)]
    pub profile: Option<String>,
    /// When the user was disabled. `None` when the user is enabled. A disabled user cannot sign in.
    #[orbit(fillable)]
    pub disabled_at: Option<NaiveDateTime>,
}

/// A row of the `user_avatars` table, one for each user who has a profile picture. See
/// `migrations/20261111000000_profile`.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "user_avatars", primary_key = "user_id")]
#[allow(dead_code)]
struct UserAvatar {
    #[orbit(fillable)]
    user_id: i64,
    #[orbit(fillable)]
    data: Vec<u8>,
}

/// The themes a user can choose.
pub const THEMES: [&str; 2] = ["light", "dark"];

/// Saves the theme `theme` for the user with `id`. The caller checks it is in [`THEMES`].
pub async fn set_theme<'a>(db: impl Executor<'a>, id: i64, theme: &str) -> Result<u64, Error> {
    User::query()
        .where_eq("id", id)
        .update(db, &[("theme", theme.into())])
        .await
}

/// Saves the name, the email and the app's profile fields (as JSON) of the user with `id`.
pub async fn update_profile<'a>(
    db: impl Executor<'a>,
    id: i64,
    name: &str,
    email: &str,
    profile: &str,
) -> Result<u64, Error> {
    User::query()
        .where_eq("id", id)
        .update(
            db,
            &[
                ("name", name.into()),
                ("email", email.into()),
                ("profile", profile.into()),
            ],
        )
        .await
}

/// Replaces the password hash of the user with `id`.
pub async fn update_password<'a>(
    db: impl Executor<'a>,
    id: i64,
    password: &str,
) -> Result<u64, Error> {
    let hash = ignite_core::hash_password(password).map_err(hash_error)?;
    User::query()
        .where_eq("id", id)
        .update(db, &[("password_hash", hash.as_str().into())])
        .await
}

/// Sets a new password for the user with `id` and ends every session of the user, so
/// anyone signed in with the old password must sign in again. Used by `ignite user password`.
pub async fn reset_password(db: &Database, id: i64, password: &str) -> Result<(), Error> {
    db.transaction(async |tx| {
        update_password(&mut *tx, id, password).await?;
        sessions::delete_for_user(&mut *tx, id).await?;
        Ok::<(), Error>(())
    })
    .await
}

/// Disables the user with `id`: they cannot sign in, and their sessions end. The caller
/// checks the user is enabled and is not the last enabled super-admin.
pub async fn disable(db: &Database, id: i64) -> Result<(), Error> {
    db.transaction(async |tx| disable_in(tx, id).await).await
}

/// [`disable`] inside a transaction the caller already has, so a change can check and disable
/// in one transaction.
pub(crate) async fn disable_in(tx: &mut Transaction, id: i64) -> Result<(), Error> {
    User::query()
        .where_eq("id", id)
        .update(
            &mut *tx,
            &[("disabled_at", Value::from(Utc::now().naive_utc()))],
        )
        .await?;
    sessions::delete_for_user(&mut *tx, id).await?;
    Ok(())
}

/// Enables the user with `id` again. Their old sessions stay ended, so they sign in again.
pub async fn enable<'a>(db: impl Executor<'a>, id: i64) -> Result<u64, Error> {
    User::query()
        .where_eq("id", id)
        .update(db, &[("disabled_at", Value::Null)])
        .await
}

/// Whether another user already has `email`. The user `except_id` is not counted,
/// so a user can keep their own email.
pub async fn email_taken<'a>(
    db: impl Executor<'a>,
    email: &str,
    except_id: i64,
) -> Result<bool, Error> {
    User::query()
        .where_eq("email", email)
        .where_op("id", Operator::Ne, except_id)
        .exists(db)
        .await
}

/// Stores the profile picture of the user with `id`, replacing any earlier one.
pub async fn set_avatar(db: &Database, id: i64, mime: &str, data: &[u8]) -> Result<(), Error> {
    db.transaction(async |tx| {
        UserAvatar::query()
            .replace(
                &mut *tx,
                &[("user_id", id.into()), ("data", data.to_vec().into())],
            )
            .await?;
        User::query()
            .where_eq("id", id)
            .update(&mut *tx, &[("avatar_mime", mime.into())])
            .await?;
        Ok::<(), Error>(())
    })
    .await
}

/// Removes the profile picture of the user with `id`. The initials show again.
pub async fn remove_avatar(db: &Database, id: i64) -> Result<(), Error> {
    db.transaction(async |tx| {
        UserAvatar::query()
            .where_eq("user_id", id)
            .delete(&mut *tx)
            .await?;
        User::query()
            .where_eq("id", id)
            .update(&mut *tx, &[("avatar_mime", Value::Null)])
            .await?;
        Ok::<(), Error>(())
    })
    .await
}

/// The profile picture of the user with `id`: its type and its bytes, if there is one.
pub async fn avatar(db: &Database, id: i64) -> Result<Option<(String, Vec<u8>)>, Error> {
    let mime: Option<(Option<String>,)> = User::query()
        .where_eq("id", id)
        .select(&["avatar_mime"])
        .first_as(db)
        .await?;
    let Some((Some(mime),)) = mime else {
        return Ok(None);
    };
    let data: Option<(Vec<u8>,)> = UserAvatar::query()
        .where_eq("user_id", id)
        .select(&["data"])
        .first_as(db)
        .await?;
    Ok(data.map(|(data,)| (mime, data)))
}

pub use ignite_core::{hash_password, verify_password};

/// Inserts a user with a hashed password and returns the new row.
pub async fn create_user(
    db: &Database,
    name: &str,
    email: &str,
    password: &str,
) -> Result<User, Error> {
    let password_hash = ignite_core::hash_password(password).map_err(hash_error)?;
    let id = User::query()
        .insert(
            db,
            &[
                ("name", name.into()),
                ("email", email.into()),
                ("password_hash", password_hash.as_str().into()),
            ],
        )
        .await?;
    find_by_id(db, id as i64)
        .await?
        .ok_or(Error::NotFound { table: "users" })
}

/// Turns a hashing failure into a database error, so the callers have one error type.
fn hash_error(error: impl std::fmt::Display) -> Error {
    Error::Database(orbit_orm::__sqlx::Error::Protocol(format!(
        "could not hash the password: {error}"
    )))
}

/// Finds the user with `email`, if there is one.
pub async fn find_by_email<'a>(db: impl Executor<'a>, email: &str) -> Result<Option<User>, Error> {
    User::query().where_eq("email", email).first(db).await
}

/// Finds the user with `id`, if there is one.
pub async fn find_by_id<'a>(db: impl Executor<'a>, id: i64) -> Result<Option<User>, Error> {
    User::query().find(db, id).await
}
