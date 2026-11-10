//! The `users` table the panel signs in with: its schema, argon2 password
//! hashes, and the lookups the login and the CLI need.

use chrono::NaiveDateTime;
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::QueryResult;

diesel::table! {
    /// The panel's `users` table. See `migrations/20261108000000_create_users`.
    users (id) {
        id -> BigInt,
        name -> Text,
        email -> Text,
        password_hash -> Text,
        created_at -> Timestamp,
        theme -> Text,
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
}

/// The themes a user can choose.
pub const THEMES: [&str; 2] = ["light", "dark"];

/// Saves the theme `theme` for the user with `id`. The caller checks it is in [`THEMES`].
pub fn set_theme(conn: &mut MysqlConnection, id: i64, theme: &str) -> QueryResult<usize> {
    diesel::update(users::table.find(id))
        .set(users::theme.eq(theme))
        .execute(conn)
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
    let password_hash = hash_password(password).map_err(|e| {
        diesel::result::Error::SerializationError(Box::new(std::io::Error::other(e.to_string())))
    })?;
    diesel::insert_into(users::table)
        .values(NewUser {
            name,
            email,
            password_hash: &password_hash,
        })
        .execute(conn)?;
    find_by_email(conn, email)?.ok_or(diesel::result::Error::NotFound)
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
