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
mod routes;
mod session;
mod sessions;
mod throttle;
pub mod users;
mod views;

use std::error::Error;

use diesel::mysql::MysqlConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

pub use csrf::{CsrfForm, CsrfToken};
pub use embedded::EmbeddedViews;
pub use ignite::Ignite;
pub use ignite_core;
pub use session::AdminUser;

/// The panel's migrations, embedded in the crate.
const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Runs the panel's pending migrations, such as the `users` table. Running them
/// again does nothing. Call it at startup, or let `ignite user create` call it.
pub fn run_migrations(conn: &mut MysqlConnection) -> Result<(), Box<dyn Error + Send + Sync>> {
    conn.run_pending_migrations(MIGRATIONS).map(|_| ())
}
