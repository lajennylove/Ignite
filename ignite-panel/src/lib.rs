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
mod brand;
pub mod cli;
mod csrf;
mod embedded;
mod erased;
mod ignite;
pub mod mail;
mod migrations;
mod navigation;
pub mod not_found;
mod password_reset;
mod profile;
mod proxy;
mod roles;
mod routes;
pub mod screens;
mod session;
mod sessions;
pub mod theme;
mod throttle;
pub mod users;
mod views;

use std::collections::HashSet;

use orbit_orm::{raw, Database, Driver, Error as OrbitError, Migrator};

pub use csrf::{CsrfForm, CsrfToken};
pub use embedded::EmbeddedViews;
pub use ignite::Ignite;
pub use ignite_core;
pub use mail::{EmailitMailer, MailError, Mailer, Message, NotConfigured, RecordingMailer};
pub use navigation::{NavigationGroup, NavigationItem};
pub use not_found::NotFound;
pub use roles::{role_label, Role};
pub use session::AdminUser;
pub use theme::{
    active_admin_theme, register_admin_theme, registered_admin_themes, resolve_admin_theme,
    set_admin_theme, with_admin_theme, AdminTheme,
};

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

/// Creates the permission rows of the runtime resource at `slug`: the five abilities of its
/// actions (`<slug>.viewAny`, `.view`, `.create`, `.update` and `.delete`, or the ones the
/// resource names with `RuntimeResource::ability`) and its access ability, if any, so a role
/// can be granted them at once. Call it
/// with [`ignite_core::register_runtime`], in the same request, as a static resource gets its
/// rows when the app starts. Running it again changes nothing. If the app does not call it,
/// the panel creates the rows on the next signed-in request.
///
/// Refuses, and creates nothing, when no runtime resource is registered at `slug`, so a
/// mistaken call cannot add permissions for a resource that does not exist.
pub async fn register_runtime_permissions(
    db: &Database,
    slug: &str,
) -> Result<(), authz::AuthzError> {
    let Some(resource) = ignite_core::runtime_resource(slug) else {
        return Err(authz::AuthzError::NotFound(format!(
            "The runtime resource {slug}"
        )));
    };
    authz::register(db, &resource.ability_names()).await?;
    Ok(())
}

/// Registers `abilities` as permissions while the app runs, as
/// [`Ignite::abilities`](crate::Ignite::abilities) does when it starts: the Roles screen lists
/// them from the next request on and a role can be granted them, without a restart. Such as
/// the capabilities of a post type created on Mission Control's screens (`edit_books`,
/// `delete_books`). Registering an ability again changes nothing.
pub async fn register_abilities<I, S>(db: &Database, abilities: I) -> Result<(), authz::AuthzError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let names: Vec<String> = abilities.into_iter().map(Into::into).collect();
    authz::register(db, &names).await?;
    Ok(())
}

/// The panel's migrations, written with Orbit's schema builder in `src/migrations/`, one
/// module each. They render the SQL of the connected database when they run.
fn migrator() -> Result<Migrator, OrbitError> {
    Migrator::new(migrations::all())
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

/// Reverts the most recently applied batch, running the down statements of each migration in
/// reverse order, and returns the names it reverted, last first. A batch is the set of
/// migrations that one run applied, and one rollback reverses the whole run, as Laravel's
/// `migrate:rollback` does. The history that Diesel recorded is batch 0, so the rollback that
/// reaches it reverts those migrations too. The batch is the latest one in the database's
/// migrations table, which an app's own migrations can share: when an app records its
/// migrations and the panel's as one batch, the app's migrator rolls the whole run back. When
/// the latest batch holds a migration that is not the panel's, this reverts nothing and returns
/// [`orbit_orm::Error::UnknownMigration`]. Returns nothing when no migration is applied.
pub async fn rollback(db: &Database) -> Result<Vec<String>, OrbitError> {
    let migrator = migrator()?;
    migrator.rollback(db).await
}

/// The names of the migrations that are applied. It reads only: unlike Orbit's
/// `Migrator::status`, it does not create the migrations table.
async fn applied_names(db: &Database, migrator: &Migrator) -> Result<HashSet<String>, OrbitError> {
    if !table_exists(db, MIGRATIONS_TABLE).await? {
        return Ok(HashSet::new());
    }
    let versions: Vec<(String,)> =
        raw::fetch_all(db, "SELECT version FROM __diesel_schema_migrations", &[]).await?;
    Ok(migrator
        .migrations()
        .iter()
        .filter(|migration| versions.iter().any(|(v,)| v == &migration.name[..14]))
        .map(|migration| migration.name.clone())
        .collect())
}

/// Whether a table of the connected database exists. Reads the database's catalog only:
/// `information_schema` on MySQL and PostgreSQL, `sqlite_master` on SQLite.
async fn table_exists(db: &Database, table: &str) -> Result<bool, OrbitError> {
    let sql = match db.driver() {
        Driver::MySql => {
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_name = ?"
        }
        #[cfg(feature = "postgres")]
        Driver::Postgres => {
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = ?"
        }
        #[cfg(feature = "sqlite")]
        Driver::Sqlite => "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
    };
    let count: i64 = raw::fetch_scalar(db, sql, &[table.into()]).await?;
    Ok(count > 0)
}
