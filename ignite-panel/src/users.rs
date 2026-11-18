//! The `users` table the panel signs in with: its schema, argon2 password
//! hashes, profile pictures (files in the storage directory), and the lookups the login and
//! the CLI need.
//!
//! [`create_user`] and [`delete_user`] are the only ways Ignite creates and deletes a user,
//! from the panel, the CLI or an app's resource. Each runs the app's lifecycle callbacks,
//! registered with [`Ignite::on_user_created`](crate::Ignite::on_user_created),
//! [`Ignite::on_user_deleting`](crate::Ignite::on_user_deleting) and
//! [`Ignite::on_user_deleted`](crate::Ignite::on_user_deleted), in the transaction that
//! writes the user, so the app's own rows are written or removed with it.

use std::future::Future;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, PoisonError, RwLock};

use chrono::{NaiveDateTime, Utc};
use orbit_orm::{raw, Database, Error, Executor, Model, Operator, OrbitModel, Transaction, Value};
use rand_core::{OsRng, RngCore};
use rocket::tokio::fs;

use crate::sessions;

/// A row of the `users` table. See `src/migrations/create_users.rs`.
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
    /// The admin theme the user chose, by name, such as `wp`. `None` renders the site's default
    /// theme. Separate from [`theme`](Self::theme), the light or dark mode.
    #[orbit(fillable)]
    pub admin_theme: Option<String>,
}

/// A row of the `user_avatars` table, one for each user who has a profile picture: the path
/// of its file, relative to the storage directory. Its type is [`User::avatar_mime`]. See
/// `src/migrations/profile.rs`.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "user_avatars", primary_key = "user_id")]
#[allow(dead_code)]
struct UserAvatar {
    #[orbit(fillable)]
    user_id: i64,
    #[orbit(fillable)]
    path: String,
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

/// Saves the admin theme `name` for the user with `id`, or clears it with `None`, so the user
/// gets the site's default theme. The caller checks the name is registered. The light or dark
/// mode ([`set_theme`]) is a separate column and is not changed.
pub async fn set_admin_theme<'a>(
    db: impl Executor<'a>,
    id: i64,
    name: Option<&str>,
) -> Result<u64, Error> {
    User::query()
        .where_eq("id", id)
        .update(db, &[("admin_theme", name.into())])
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

/// Stores the profile picture of the user with `id`, replacing any earlier one. `mime` is one
/// of the types the panel accepts: "image/png", "image/jpeg" or "image/webp".
///
/// The picture is a file in the storage directory (see
/// [`Ignite::storage_dir`](crate::Ignite::storage_dir)), `avatars/<id>.<extension>`; the
/// `user_avatars` row keeps that path, relative to the directory, and [`User::avatar_mime`]
/// keeps the type. The picture is first written to a temporary file in the `avatars` folder.
/// Then the row and [`User::avatar_mime`] are written in one transaction. When the
/// transaction fails, only the temporary file is removed, so the earlier picture is kept.
/// After the commit, the temporary file is renamed into place and an earlier file with another
/// extension is removed. If that rename fails, the error is logged and returned and the
/// temporary file is removed; the row already names the new file, so the panel serves the
/// earlier picture when it had the same extension, and otherwise finds none (404), until the
/// user saves one again. A crash between the write and the rename can leave a `.tmp` file in
/// the folder.
pub async fn set_avatar(db: &Database, id: i64, mime: &str, data: &[u8]) -> Result<(), Error> {
    let extension = crate::profile::picture_extension(mime).ok_or_else(|| {
        io_error(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("a profile picture cannot be of type {mime}"),
        ))
    })?;
    let path = format!("{AVATARS}/{id}.{extension}");
    let storage = storage_dir();
    let folder = storage.join(AVATARS);
    fs::create_dir_all(&folder).await.map_err(io_error)?;
    let temporary = folder.join(format!(".{id}.{:016x}.tmp", OsRng.next_u64()));
    if let Err(error) = fs::write(&temporary, data).await {
        let _ = fs::remove_file(&temporary).await;
        return Err(io_error(error));
    }
    let saved = db
        .transaction(async |tx| {
            let earlier = avatar_path(&mut *tx, id).await?;
            UserAvatar::query()
                .replace(
                    &mut *tx,
                    &[("user_id", id.into()), ("path", path.as_str().into())],
                )
                .await?;
            User::query()
                .where_eq("id", id)
                .update(&mut *tx, &[("avatar_mime", mime.into())])
                .await?;
            Ok::<_, Error>(earlier)
        })
        .await;
    let earlier = match saved {
        Ok(earlier) => earlier,
        Err(error) => {
            let _ = fs::remove_file(&temporary).await;
            return Err(error);
        }
    };
    let file = storage.join(&path);
    if let Err(error) = fs::rename(&temporary, &file).await {
        rocket::warn!(
            "ignite: could not move the picture into place at {}: {error}",
            file.display()
        );
        let _ = fs::remove_file(&temporary).await;
        return Err(io_error(error));
    }
    if let Some(earlier) = earlier.filter(|earlier| *earlier != path) {
        remove_avatar_file(&earlier).await;
    }
    Ok(())
}

/// Removes the profile picture of the user with `id`. The initials show again. The row and
/// [`User::avatar_mime`] are cleared in one transaction, then the file is removed.
pub async fn remove_avatar(db: &Database, id: i64) -> Result<(), Error> {
    let path = db
        .transaction(async |tx| {
            let path = avatar_path(&mut *tx, id).await?;
            UserAvatar::query()
                .where_eq("user_id", id)
                .delete(&mut *tx)
                .await?;
            User::query()
                .where_eq("id", id)
                .update(&mut *tx, &[("avatar_mime", Value::Null)])
                .await?;
            Ok::<_, Error>(path)
        })
        .await?;
    if let Some(path) = path {
        remove_avatar_file(&path).await;
    }
    Ok(())
}

/// The profile picture of the user with `id`: its type, from [`User::avatar_mime`], and its
/// bytes, read from its file, if there is one. A user without a type has no picture. A stored
/// path that is absolute or leaves the storage directory is refused, as is a file that is
/// missing: both are `None`.
pub async fn avatar(db: &Database, id: i64) -> Result<Option<(String, Vec<u8>)>, Error> {
    let row: Option<(String, Option<String>)> = raw::fetch_all(
        db,
        "SELECT user_avatars.path, users.avatar_mime FROM user_avatars \
         JOIN users ON users.id = user_avatars.user_id WHERE user_avatars.user_id = ?",
        &[id.into()],
    )
    .await?
    .pop();
    let Some((path, Some(mime))) = row else {
        return Ok(None);
    };
    let Some(file) = storage_file(&path) else {
        rocket::warn!(
            "ignite: not serving the picture at {path}: it is outside the storage directory"
        );
        return Ok(None);
    };
    match fs::read(&file).await {
        Ok(bytes) => Ok(Some((mime, bytes))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

/// The folder of the storage directory that holds the profile pictures.
const AVATARS: &str = "avatars";

/// The stored path of the picture of the user with `id`, if they have one.
async fn avatar_path(tx: &mut Transaction, id: i64) -> Result<Option<String>, Error> {
    let row: Option<(String,)> = UserAvatar::query()
        .where_eq("user_id", id)
        .select(&["path"])
        .first_as(&mut *tx)
        .await?;
    Ok(row.map(|(path,)| path))
}

/// The file at `path` in the storage directory. `None` when `path` is absolute or has a part
/// such as `..`, so a stored path never reaches a file outside the directory.
fn storage_file(path: &str) -> Option<PathBuf> {
    let relative = Path::new(path);
    let inside = !path.is_empty()
        && !path.contains("..")
        && relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
    inside.then(|| storage_dir().join(relative))
}

/// Removes the picture file at the stored `path`. The row is already gone, so a failure is
/// logged and not returned; a file that is already missing is not a failure.
async fn remove_avatar_file(path: &str) {
    let Some(file) = storage_file(path) else {
        rocket::warn!(
            "ignite: not removing the picture at {path}: it is outside the storage directory"
        );
        return;
    };
    match fs::remove_file(&file).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            rocket::warn!(
                "ignite: could not remove the picture {}: {error}",
                file.display()
            )
        }
    }
}

/// Turns a file error into a database error, so the callers have one error type.
fn io_error(error: io::Error) -> Error {
    Error::Database(orbit_orm::__sqlx::Error::Io(error))
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

/// The storage directory when no builder sets one: `storage` in the working directory, as
/// Laravel's `storage/`.
pub(crate) const DEFAULT_STORAGE_DIR: &str = "storage";

/// The directory the panel keeps its files in, set by the builder's
/// [`storage_dir`](crate::Ignite::storage_dir). Process-wide, as [`CALLBACKS`] is, because
/// [`delete_user`] removes the user's picture with only a [`Database`]. `None` is the default.
static STORAGE_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Makes `dir` the storage directory, replacing any set before. Called by the builder when the
/// panel is built and when the app runs its command line.
pub(crate) fn install_storage_dir(dir: PathBuf) {
    *STORAGE_DIR.write().unwrap_or_else(PoisonError::into_inner) = Some(dir);
}

/// Restores the default storage directory, `storage` in the working directory, until a
/// builder sets its own again. The setting is process-wide, so a test suite that builds panels
/// with their own directory calls it, as it calls [`flush_callbacks`].
pub fn flush_storage_dir() {
    *STORAGE_DIR.write().unwrap_or_else(PoisonError::into_inner) = None;
}

/// The storage directory in use now.
fn storage_dir() -> PathBuf {
    STORAGE_DIR
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STORAGE_DIR))
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
/// user, and then no callback runs. Their sessions are deleted with them, and their roles and
/// picture row go through the foreign keys. Their picture file is removed after the
/// transaction commits; a file that is missing or cannot be removed does not fail the delete.
///
/// In one transaction, the app's [`on_user_deleting`](crate::Ignite::on_user_deleting)
/// callbacks run first, while the user's row and sessions still exist, so they can read the
/// user and remove or reassign the app's rows that refer to it; then the sessions and the row
/// are deleted; then the [`on_user_deleted`](crate::Ignite::on_user_deleted) callbacks run.
/// Each kind runs in the order it was registered. When any callback returns an error, nothing
/// is deleted and the error is returned.
///
/// It does not check roles. `ignite user delete` refuses the last enabled super-admin.
pub async fn delete_user(db: &Database, id: i64) -> Result<u64, Error> {
    let (deleted, picture) = db.transaction(async |tx| delete_in(tx, id).await).await?;
    remove_picture_of_deleted(picture).await;
    Ok(deleted)
}

/// [`delete_user`] inside a transaction the caller already has, so a command can check and
/// delete in one transaction. Returns the number of rows deleted and the stored path of the
/// user's picture, read before the delete; the caller passes it to
/// [`remove_picture_of_deleted`] once the transaction has committed.
pub(crate) async fn delete_in(
    tx: &mut Transaction,
    id: i64,
) -> Result<(u64, Option<String>), Error> {
    // Locking the row means two deletes of one user cannot both run the callbacks. SQLite has
    // no row locks, so Orbit leaves the clause out there, as Laravel's SQLite grammar does.
    let found: Option<(i64,)> = User::query()
        .where_eq("id", id)
        .select(&["id"])
        .for_update()
        .first_as(&mut *tx)
        .await?;
    if found.is_none() {
        return Ok((0, None));
    }
    let picture = avatar_path(&mut *tx, id).await?;
    let callbacks = callbacks();
    for callback in &callbacks.deleting {
        callback(id, &mut *tx).await?;
    }
    // A SQLite install has no `sessions_user_fk` (SQLite cannot add a key to an existing
    // table), so the sessions are deleted here, not left to the key. After the deleting
    // callbacks, which see the user as it was, and before the user's row.
    sessions::delete_for_user(&mut *tx, id).await?;
    let deleted = User::query().where_eq("id", id).delete(&mut *tx).await?;
    for callback in &callbacks.deleted {
        callback(id, &mut *tx).await?;
    }
    Ok((deleted, picture))
}

/// Removes the picture file of a user whose delete has committed, given the path
/// [`delete_in`] returned.
pub(crate) async fn remove_picture_of_deleted(picture: Option<String>) {
    if let Some(path) = picture {
        remove_avatar_file(&path).await;
    }
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
