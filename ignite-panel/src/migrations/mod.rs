//! The panel's migrations, written once with Orbit's schema builder, as Laravel writes them
//! with `Schema::create` and `Blueprint`. Each migration renders the SQL of the connected
//! database when it runs, so the same set serves SQLite, MySQL and MariaDB, and PostgreSQL.
//!
//! One module per migration, named after it. A migration's name never changes: Orbit records
//! it in `__diesel_schema_migrations`, so an install that ran the earlier SQL files has the
//! same history and nothing runs twice. The names of indexes, uniques and foreign keys are
//! those of the earlier SQL too, so a rollback finds them on an existing install.
//!
//! A new migration gets its own module and goes at the end of [`all`].

use orbit_orm::Migration;

mod account_lifecycle;
mod add_user_theme;
mod create_login_throttles;
mod create_sessions;
mod create_users;
mod login_throttles_window_index;
mod password_reset_tokens;
mod profile;
mod roles_permissions;

/// Every migration of the panel, in order.
pub(crate) fn all() -> Vec<Migration> {
    vec![
        create_users::migration(),
        create_sessions::migration(),
        create_login_throttles::migration(),
        add_user_theme::migration(),
        profile::migration(),
        login_throttles_window_index::migration(),
        roles_permissions::migration(),
        account_lifecycle::migration(),
        password_reset_tokens::migration(),
    ]
}
