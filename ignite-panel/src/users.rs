//! The `users` table the panel signs in with: its schema, argon2 password
//! hashes, profile pictures, and the lookups the login and the CLI need.

use chrono::{NaiveDateTime, Utc};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::QueryResult;

use crate::sessions;

diesel::table! {
    /// The panel's `users` table. See `migrations/20261108000000_create_users`.
    users (id) {
        id -> BigInt,
        name -> Text,
        email -> Text,
        password_hash -> Text,
        created_at -> Timestamp,
        theme -> Text,
        avatar_mime -> Nullable<Text>,
        profile -> Nullable<Text>,
        disabled_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    /// The profile pictures, one row for each user who has one. See
    /// `migrations/20261111000000_profile`.
    user_avatars (user_id) {
        user_id -> BigInt,
        data -> Binary,
    }
}

/// A row of the `users` table.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = users)]
pub struct User {
    /// The primary key.
    pub id: i64,
    /// The display name.
    pub name: String,
    /// The login email. Unique.
    pub email: String,
    /// The argon2 hash in PHC string form. Never shown or logged.
    pub password_hash: String,
    /// When the row was created.
    pub created_at: NaiveDateTime,
    /// The colour theme the panel shows this user: "light" or "dark".
    pub theme: String,
    /// The type of the profile picture, such as "image/png". `None` shows the initials.
    pub avatar_mime: Option<String>,
    /// The values of the profile fields the app adds, as a JSON object. `None` when unset.
    pub profile: Option<String>,
    /// When the user was disabled. `None` when the user is enabled. A disabled user cannot sign in.
    pub disabled_at: Option<NaiveDateTime>,
}

/// The themes a user can choose.
pub const THEMES: [&str; 2] = ["light", "dark"];

/// Saves the theme `theme` for the user with `id`. The caller checks it is in [`THEMES`].
pub fn set_theme(conn: &mut MysqlConnection, id: i64, theme: &str) -> QueryResult<usize> {
    diesel::update(users::table.find(id))
        .set(users::theme.eq(theme))
        .execute(conn)
}

/// Saves the name, the email and the app's profile fields (as JSON) of the user with `id`.
pub fn update_profile(
    conn: &mut MysqlConnection,
    id: i64,
    name: &str,
    email: &str,
    profile: &str,
) -> QueryResult<usize> {
    diesel::update(users::table.find(id))
        .set((
            users::name.eq(name),
            users::email.eq(email),
            users::profile.eq(profile),
        ))
        .execute(conn)
}

/// Replaces the password hash of the user with `id`.
pub fn update_password(conn: &mut MysqlConnection, id: i64, password: &str) -> QueryResult<usize> {
    let hash = hash_password(password).map_err(hash_error)?;
    diesel::update(users::table.find(id))
        .set(users::password_hash.eq(hash))
        .execute(conn)
}

/// Sets a new password for the user with `id` and ends every session of the user, so
/// anyone signed in with the old password must sign in again. Used by `ignite user password`.
pub fn reset_password(conn: &mut MysqlConnection, id: i64, password: &str) -> QueryResult<()> {
    conn.transaction(|conn| {
        update_password(conn, id, password)?;
        sessions::delete_for_user(conn, id)?;
        Ok(())
    })
}

/// Disables the user with `id`: they cannot sign in, and their sessions end. The caller
/// checks the user is enabled and is not the last enabled super-admin.
pub fn disable(conn: &mut MysqlConnection, id: i64) -> QueryResult<()> {
    conn.transaction(|conn| {
        diesel::update(users::table.find(id))
            .set(users::disabled_at.eq(Some(Utc::now().naive_utc())))
            .execute(conn)?;
        sessions::delete_for_user(conn, id)?;
        Ok(())
    })
}

/// Enables the user with `id` again. Their old sessions stay ended, so they sign in again.
pub fn enable(conn: &mut MysqlConnection, id: i64) -> QueryResult<usize> {
    diesel::update(users::table.find(id))
        .set(users::disabled_at.eq(None::<NaiveDateTime>))
        .execute(conn)
}

/// Whether another user already has `email`. The user `except_id` is not counted,
/// so a user can keep their own email.
pub fn email_taken(conn: &mut MysqlConnection, email: &str, except_id: i64) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        users::table
            .filter(users::email.eq(email))
            .filter(users::id.ne(except_id)),
    ))
    .get_result(conn)
}

/// Stores the profile picture of the user with `id`, replacing any earlier one.
pub fn set_avatar(conn: &mut MysqlConnection, id: i64, mime: &str, data: &[u8]) -> QueryResult<()> {
    conn.transaction(|conn| {
        diesel::replace_into(user_avatars::table)
            .values((user_avatars::user_id.eq(id), user_avatars::data.eq(data)))
            .execute(conn)?;
        diesel::update(users::table.find(id))
            .set(users::avatar_mime.eq(Some(mime)))
            .execute(conn)?;
        Ok(())
    })
}

/// Removes the profile picture of the user with `id`. The initials show again.
pub fn remove_avatar(conn: &mut MysqlConnection, id: i64) -> QueryResult<()> {
    conn.transaction(|conn| {
        diesel::delete(user_avatars::table.find(id)).execute(conn)?;
        diesel::update(users::table.find(id))
            .set(users::avatar_mime.eq(None::<&str>))
            .execute(conn)?;
        Ok(())
    })
}

/// The profile picture of the user with `id`: its type and its bytes, if there is one.
pub fn avatar(conn: &mut MysqlConnection, id: i64) -> QueryResult<Option<(String, Vec<u8>)>> {
    let mime: Option<String> = users::table
        .find(id)
        .select(users::avatar_mime)
        .first(conn)
        .optional()?
        .flatten();
    let Some(mime) = mime else {
        return Ok(None);
    };
    let data: Option<Vec<u8>> = user_avatars::table
        .find(id)
        .select(user_avatars::data)
        .first(conn)
        .optional()?;
    Ok(data.map(|data| (mime, data)))
}

#[derive(Insertable)]
#[diesel(table_name = users)]
struct NewUser<'a> {
    name: &'a str,
    email: &'a str,
    password_hash: &'a str,
}

pub use ignite_core::{hash_password, verify_password};

/// Inserts a user with a hashed password and returns the new row.
pub fn create_user(
    conn: &mut MysqlConnection,
    name: &str,
    email: &str,
    password: &str,
) -> Result<User, diesel::result::Error> {
    let password_hash = hash_password(password).map_err(hash_error)?;
    diesel::insert_into(users::table)
        .values(NewUser {
            name,
            email,
            password_hash: &password_hash,
        })
        .execute(conn)?;
    find_by_email(conn, email)?.ok_or(diesel::result::Error::NotFound)
}

/// Turns a hashing failure into a database error, so the callers have one error type.
fn hash_error(error: impl std::fmt::Display) -> diesel::result::Error {
    diesel::result::Error::SerializationError(Box::new(std::io::Error::other(error.to_string())))
}

/// Finds the user with `email`, if there is one.
pub fn find_by_email(conn: &mut MysqlConnection, email: &str) -> QueryResult<Option<User>> {
    users::table
        .filter(users::email.eq(email))
        .select(User::as_select())
        .first(conn)
        .optional()
}

/// Finds the user with `id`, if there is one.
pub fn find_by_id(conn: &mut MysqlConnection, id: i64) -> QueryResult<Option<User>> {
    users::table
        .find(id)
        .select(User::as_select())
        .first(conn)
        .optional()
}
