//! Rocket fairing and admin routes for Ignite, rendered with Propel views.
//!
//! Attach an [`Ignite`] builder to a Rocket instance with its resources. The
//! fairing manages the connection pool and mounts the admin routes at ignite:
//!
//! ```ignore
//! rocket::build().attach(Ignite::new().resource::<Users>().fairing())
//! ```

mod assets;
pub mod cli;
mod csrf;
mod embedded;
mod erased;
mod ignite;
mod profile;
mod proxy;
mod routes;
mod session;
mod sessions;
mod throttle;
pub mod users;
mod views;

use std::error::Error;

use diesel::migration::MigrationSource;
use diesel::mysql::{Mysql, MysqlConnection};
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

pub use csrf::{CsrfForm, CsrfToken};
pub use embedded::EmbeddedViews;
pub use ignite::Ignite;
pub use ignite_core;
pub use session::AdminUser;

/// Loads the `.env` file in the working directory into the process environment, without
/// overriding variables already set. A missing file is fine. A file that exists but cannot
/// be parsed stops the app with a clear message, as Laravel does for a malformed `.env`.
///
/// # Panics
///
/// Panics when the `.env` file exists and cannot be read or parsed.
pub fn load_env() {
    match dotenvy::dotenv() {
        Ok(_) => {}
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("Ignite could not read .env: {error}"),
    }
}

/// The panel's migrations, embedded in the crate.
const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Runs the panel's pending migrations and returns the names of the ones it applied, such
/// as `20261112000000_login_throttles_window_index`. Running them again applies nothing.
/// `ignite migrate` calls this. An app that prefers to migrate itself can call it too.
pub fn migrate(conn: &mut MysqlConnection) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
    let names: Vec<(String, String)> = migration_names()?;
    let applied = conn.run_pending_migrations(MIGRATIONS)?;
    Ok(applied
        .iter()
        .map(|version| {
            names
                .iter()
                .find(|(v, _)| v == &version.to_string())
                .map_or_else(|| version.to_string(), |(_, name)| name.clone())
        })
        .collect())
}

/// Runs the panel's pending migrations. Use [`migrate`] to learn what was applied.
pub fn run_migrations(conn: &mut MysqlConnection) -> Result<(), Box<dyn Error + Send + Sync>> {
    migrate(conn).map(|_| ())
}

/// The names of the panel's migrations that have not run yet, in order. It reads the
/// migration table and changes nothing, so it is safe to call before the database is
/// migrated, and `ignite user create` uses it to refuse to run on an old schema.
pub fn pending_migrations(
    conn: &mut MysqlConnection,
) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
    let applied: Vec<String> = if table_exists(conn, "__diesel_schema_migrations")? {
        sql_query("SELECT version FROM __diesel_schema_migrations")
            .load::<VersionRow>(conn)?
            .into_iter()
            .map(|row| row.version)
            .collect()
    } else {
        Vec::new()
    };
    Ok(migration_names()?
        .into_iter()
        .filter(|(version, _)| !applied.contains(version))
        .map(|(_, name)| name)
        .collect())
}

/// Every embedded migration as `(version, name)`, such as `("20261108000000", "20261108000000_create_users")`.
fn migration_names() -> Result<Vec<(String, String)>, Box<dyn Error + Send + Sync>> {
    let migrations = <EmbeddedMigrations as MigrationSource<Mysql>>::migrations(&MIGRATIONS)?;
    Ok(migrations
        .iter()
        .map(|migration| {
            let name = migration.name();
            (name.version().to_string(), name.to_string())
        })
        .collect())
}

#[derive(QueryableByName)]
struct VersionRow {
    #[diesel(sql_type = Text)]
    version: String,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

/// Whether a table of the connected database exists. Reads `information_schema` only.
fn table_exists(
    conn: &mut MysqlConnection,
    table: &str,
) -> Result<bool, Box<dyn Error + Send + Sync>> {
    let rows: Vec<CountRow> = sql_query(
        "SELECT COUNT(*) AS count FROM information_schema.tables \
         WHERE table_schema = DATABASE() AND table_name = ?",
    )
    .bind::<Text, _>(table)
    .load(conn)?;
    Ok(rows.first().is_some_and(|row| row.count > 0))
}
