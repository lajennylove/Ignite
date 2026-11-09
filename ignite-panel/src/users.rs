//! The `users` table the panel signs in with: its schema, argon2 password
//! hashes, and the lookups the login and the CLI need.

use argon2::password_hash::{
    Error as PasswordError, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use chrono::NaiveDateTime;
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::QueryResult;
use rand_core::OsRng;

diesel::table! {
    /// The panel's `users` table. See `migrations/20261108000000_create_users`.
    users (id) {
        id -> BigInt,
        name -> Text,
        email -> Text,
        password_hash -> Text,
        created_at -> Timestamp,
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
}

#[derive(Insertable)]
#[diesel(table_name = users)]
struct NewUser<'a> {
    name: &'a str,
    email: &'a str,
    password_hash: &'a str,
}

/// Hashes `password` with argon2 and a random salt.
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

/// Returns true when `password` matches the stored `hash`. A malformed hash never matches.
pub fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrong_password_does_not_verify() {
        let hash = hash_password("correct horse").unwrap();

        assert!(verify_password(&hash, "correct horse"));
        assert!(!verify_password(&hash, "wrong password"));
    }

    #[test]
    fn a_malformed_hash_never_verifies() {
        assert!(!verify_password("not-a-hash", "anything"));
    }

    #[test]
    fn hashes_use_a_fresh_salt() {
        assert_ne!(
            hash_password("same").unwrap(),
            hash_password("same").unwrap()
        );
    }
}
