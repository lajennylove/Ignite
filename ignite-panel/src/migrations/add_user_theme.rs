//! `20261110000000_add_user_theme`: each user's colour theme.

use orbit_orm::schema;
use orbit_orm::Migration;

/// Every user starts on the light theme, and the panel's toggle changes it.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261110000000_add_user_theme",
        vec![schema::table("users", |t| {
            t.string("theme").length(8).default("light");
        })],
        vec![schema::table("users", |t| {
            t.drop_column("theme");
        })],
    )
}
