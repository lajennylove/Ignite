//! Rocket fairing and admin routes for Ignite, rendered with Propel views.
//!
//! Attach an [`Ignite`] builder to a Rocket instance with its resources. The
//! fairing manages the connection pool and mounts the admin routes at ignite:
//!
//! ```ignore
//! rocket::build().attach(Ignite::new().resource::<Users>().fairing())
//! ```

mod assets;
pub mod authz;
pub mod cli;
mod csrf;
mod embedded;
mod erased;
mod ignite;
pub mod mail;
mod password_reset;
mod profile;
mod proxy;
mod routes;
mod session;
mod sessions;
mod throttle;
pub mod users;
mod views;

use std::collections::HashSet;

use orbit_orm::__sqlx as sqlx;
use orbit_orm::{Database, Error as OrbitError, Migrator};

pub use csrf::{CsrfForm, CsrfToken};
pub use embedded::EmbeddedViews;
pub use ignite::Ignite;
pub use ignite_core;
pub use mail::{EmailitMailer, MailError, Mailer, Message, NotConfigured, RecordingMailer};
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

/// Creates the five permission rows of the runtime resource at `slug` (`<slug>.viewAny`,
/// `.view`, `.create`, `.update` and `.delete`), so a role can be granted them at once. Call it
/// with [`ignite_core::register_runtime`], in the same request, as a static resource gets its
/// rows when the app starts. Running it again changes nothing. If the app does not call it,
/// the panel creates the rows on the next signed-in request.
pub async fn register_runtime_permissions(db: &Database, slug: &str) -> Result<(), OrbitError> {
    authz::register(db, &authz::slug_abilities(slug)).await
}

/// The panel's migrations, embedded in the crate at build time. Each name is a folder in
/// `migrations/`. A new migration is added here, in order, as well as in that folder: Orbit
/// embeds the files by name, and a missing folder is a build error.
fn migrator() -> Result<Migrator, OrbitError> {
    orbit_orm::migrations!(
        "migrations",
        "20261108000000_create_users",
        "20261108010000_create_sessions",
        "20261108020000_create_login_throttles",
        "20261110000000_add_user_theme",
        "20261111000000_profile",
        "20261112000000_login_throttles_window_index",
        "20261113000000_roles_permissions",
        "20261114000000_account_lifecycle",
        "20261115000000_password_reset_tokens",
    )
}

/// The table that the applied migrations are recorded in. It is Diesel's table: the rows that
/// Diesel wrote are the history, as batch 0, and Orbit adds its `batch` column when it needs it.
const MIGRATIONS_TABLE: &str = "__diesel_schema_migrations";

/// Runs the panel's pending migrations and returns the names of the ones it applied, such
/// as `20261112000000_login_throttles_window_index`. Running them again applies nothing.
/// `ignite migrate` calls this. An app that prefers to migrate itself can call it too.
///
/// The migrations Diesel had applied stay applied: they are batch 0 of the same table, and
/// nothing runs twice.
pub async fn migrate(db: &Database) -> Result<Vec<String>, OrbitError> {
    let migrator = migrator()?;
    migrator.run(db).await
}

/// Runs the panel's pending migrations. Use [`migrate`] to learn what was applied.
pub async fn run_migrations(db: &Database) -> Result<(), OrbitError> {
    migrate(db).await.map(|_| ())
}

/// The names of the panel's migrations that have not run yet, in order. It reads the
/// migration table and changes nothing, so it is safe to call before the database is
/// migrated, and `ignite user create` uses it to refuse to run on an old schema.
pub async fn pending_migrations(db: &Database) -> Result<Vec<String>, OrbitError> {
    Ok(migration_status(db)
        .await?
        .into_iter()
        .filter(|(_, applied)| !applied)
        .map(|(name, _)| name)
        .collect())
}

/// Every migration of the panel, in order, as `(name, applied)`. Reads only. `ignite
/// migrate:status` prints it.
pub async fn migration_status(db: &Database) -> Result<Vec<(String, bool)>, OrbitError> {
    let migrator = migrator()?;
    let applied = applied_names(db, &migrator).await?;
    Ok(migrator
        .migrations()
        .iter()
        .map(|migration| (migration.name.clone(), applied.contains(&migration.name)))
        .collect())
}

/// Reverts the most recently applied batch of the panel's migrations, running each `down.sql`
/// in reverse order, and returns the names it reverted, last first. A batch is the set of
/// migrations that one `migrate` run applied. The history that Diesel recorded is batch 0, so
/// the rollback that reaches it reverts those migrations too. Other migrations in the same
/// database are not touched. Returns nothing when no migration of the panel is applied.
pub async fn rollback(db: &Database) -> Result<Vec<String>, OrbitError> {
    let migrator = migrator()?;
    migrator.rollback(db).await
}

/// The names of the migrations that are applied. It reads only.
async fn applied_names(db: &Database, migrator: &Migrator) -> Result<HashSet<String>, OrbitError> {
    if !table_exists(db, MIGRATIONS_TABLE).await? {
        return Ok(HashSet::new());
    }
    let versions: Vec<String> =
        sqlx::query_scalar("SELECT version FROM __diesel_schema_migrations")
            .fetch_all(db.pool())
            .await?;
    Ok(migrator
        .migrations()
        .iter()
        .filter(|migration| versions.iter().any(|v| v == &migration.name[..14]))
        .map(|migration| migration.name.clone())
        .collect())
}

/// Whether a table of the connected database exists. Reads `information_schema` only.
async fn table_exists(db: &Database, table: &str) -> Result<bool, OrbitError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables \
         WHERE table_schema = DATABASE() AND table_name = ?",
    )
    .bind(table)
    .fetch_one(db.pool())
    .await?;
    Ok(count > 0)
}
