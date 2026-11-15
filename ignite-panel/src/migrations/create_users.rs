//! `20261108000000_create_users`: the users who sign in to the panel.

use orbit_orm::schema;
use orbit_orm::Migration;

/// The panel owns this table. `create_if_not_exists` lets an app that already created its own
/// users table run the panel's migrations without a conflict.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261108000000_create_users",
        vec![schema::create_if_not_exists("users", |t| {
            t.id();
            t.string("name");
            t.string("email");
            t.string("password_hash");
            t.date_time("created_at").use_current();
            t.unique(["email"]).name("users_email_unique");
        })],
        vec![schema::drop_if_exists("users")],
    )
}
