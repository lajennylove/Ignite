//! The panel's migrations, written once with Orbit's schema builder, as Laravel writes them
//! with `Schema::create` and `Blueprint`. Each migration renders the SQL of the connected
//! database when it runs, so the same set serves SQLite, MySQL and MariaDB, and PostgreSQL.
//!
//! One module per migration, named after it. Orbit records only the 14-digit version at the
//! start of a migration's name (`20261108000000` for `20261108000000_create_users`) in
//! `__diesel_schema_migrations`, and matches applied migrations by it. The version of a
//! released migration must never change, or an install would run it again; keep the whole
//! name unchanged too. An install that ran the earlier SQL files therefore has the same
//! history, and nothing runs twice. The names of indexes, uniques and foreign keys are
//! those of the earlier SQL too, so a rollback finds them on an existing install.
//!
//! A new migration gets its own module and goes at the end of [`all`].

use orbit_orm::Migration;

mod account_lifecycle;
mod add_user_admin_theme;
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
        add_user_admin_theme::migration(),
    ]
}
