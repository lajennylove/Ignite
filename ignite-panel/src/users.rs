//! The `users` table the panel signs in with: its schema, argon2 password
//! hashes, profile pictures, and the lookups the login and the CLI need.
//!
//! [`create_user`] and [`delete_user`] are the only ways Ignite creates and deletes a user,
//! from the panel, the CLI or an app's resource. Each runs the app's lifecycle callbacks,
//! registered with [`Ignite::on_user_created`](crate::Ignite::on_user_created),
//! [`Ignite::on_user_deleting`](crate::Ignite::on_user_deleting) and
//! [`Ignite::on_user_deleted`](crate::Ignite::on_user_deleted), in the transaction that
//! writes the user, so the app's own rows are written or removed with it.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, PoisonError, RwLock};

use chrono::{NaiveDateTime, Utc};
use orbit_orm::{raw, Database, Error, Executor, Model, Operator, OrbitModel, Transaction, Value};

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

/// The future a user lifecycle callback returns. It is boxed, so callbacks of different
/// types are kept in one list, and it borrows the transaction it writes in:
///
/// ```ignore
/// Ignite::new().on_user_created(|user_id, tx| {
///     Box::pin(async move {
///         raw::execute(&mut *tx, "INSERT INTO profiles (user_id) VALUES (?)", &[user_id.into()])
///             .await?;
///         Ok(())
///     })
/// })
/// ```
pub type UserCallbackFuture<'t> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 't>>;

/// A lifecycle callback as the builder keeps it: given the user's id and the open transaction.
pub(crate) type UserCallback =
    Arc<dyn for<'t> Fn(i64, &'t mut Transaction) -> UserCallbackFuture<'t> + Send + Sync>;

/// The lifecycle callbacks of an app, in the order they were registered.
#[derive(Clone, Default)]
pub(crate) struct UserCallbacks {
    /// Run by [`create_user`], after the user's row is inserted.
    pub(crate) created: Vec<UserCallback>,
    /// Run by [`delete_user`], before the user's row is deleted.
    pub(crate) deleting: Vec<UserCallback>,
    /// Run by [`delete_user`], after the user's row is deleted.
    pub(crate) deleted: Vec<UserCallback>,
}

/// The callbacks that [`create_user`] and [`delete_user`] run. They are process-wide, as
/// Laravel's model events are, because an app's resource and the CLI reach the users table
/// with only a [`Database`]. The builder sets them (see [`install_callbacks`]).
static CALLBACKS: RwLock<UserCallbacks> = RwLock::new(UserCallbacks {
    created: Vec::new(),
    deleting: Vec::new(),
    deleted: Vec::new(),
});

/// Makes `callbacks` the ones that run, replacing any set before. Called by the builder when
/// the panel is built and when the app runs its command line.
pub(crate) fn install_callbacks(callbacks: UserCallbacks) {
    *CALLBACKS.write().unwrap_or_else(PoisonError::into_inner) = callbacks;
}

/// Removes every user lifecycle callback, so [`create_user`] and [`delete_user`] run none
/// until a builder installs its own again (its [`fairing`](crate::Ignite::fairing) or
/// [`run_cli`](crate::Ignite::run_cli)), as Laravel's `Model::flushEventListeners` does.
/// The callbacks are process-wide, so a test suite that builds panels with callbacks calls it
/// before it creates users of its own.
pub fn flush_callbacks() {
    install_callbacks(UserCallbacks::default());
}

/// A copy of the callbacks that run now. No lock is held while they run.
fn callbacks() -> UserCallbacks {
    CALLBACKS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Inserts a user with a hashed password and returns the new row.
///
/// The app's [`on_user_created`](crate::Ignite::on_user_created) callbacks run after the
/// insert, in the same transaction and in the order they were registered. When one returns
/// an error, nothing is kept: the user and every row the callbacks wrote are rolled back, and
/// the error is returned.
pub async fn create_user(
    db: &Database,
    name: &str,
    email: &str,
    password: &str,
) -> Result<User, Error> {
    let password_hash = ignite_core::hash_password(password).map_err(hash_error)?;
    let created = callbacks().created;
    db.transaction(async |tx| {
        let id = User::query()
            .insert(
                &mut *tx,
                &[
                    ("name", name.into()),
                    ("email", email.into()),
                    ("password_hash", password_hash.as_str().into()),
                ],
            )
            .await? as i64;
        for callback in &created {
            callback(id, &mut *tx).await?;
        }
        find_by_id(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound { table: "users" })
    })
    .await
}

/// Deletes the user with `id` and returns the number of rows deleted: 0 when there is no such
/// user, and then no callback runs. Their sessions, roles and profile picture go with them,
/// through the foreign keys.
///
/// In one transaction, the app's [`on_user_deleting`](crate::Ignite::on_user_deleting)
/// callbacks run first, while the user's row still exists, so they can read the user and
/// remove or reassign the app's rows that refer to it; then the row is deleted; then the
/// [`on_user_deleted`](crate::Ignite::on_user_deleted) callbacks run. Each kind runs in the
/// order it was registered. When any callback returns an error, nothing is deleted and the
/// error is returned.
///
/// It does not check roles. `ignite user delete` refuses the last enabled super-admin.
pub async fn delete_user(db: &Database, id: i64) -> Result<u64, Error> {
    db.transaction(async |tx| delete_in(tx, id).await).await
}

/// [`delete_user`] inside a transaction the caller already has, so a command can check and
/// delete in one transaction.
pub(crate) async fn delete_in(tx: &mut Transaction, id: i64) -> Result<u64, Error> {
    // Locking the row means two deletes of one user cannot both run the callbacks.
    let found: Vec<(i64,)> = raw::fetch_all(
        &mut *tx,
        "SELECT id FROM users WHERE id = ? FOR UPDATE",
        &[id.into()],
    )
    .await?;
    if found.is_empty() {
        return Ok(0);
    }
    let callbacks = callbacks();
    for callback in &callbacks.deleting {
        callback(id, &mut *tx).await?;
    }
    let deleted = User::query().where_eq("id", id).delete(&mut *tx).await?;
    for callback in &callbacks.deleted {
        callback(id, &mut *tx).await?;
    }
    Ok(deleted)
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
