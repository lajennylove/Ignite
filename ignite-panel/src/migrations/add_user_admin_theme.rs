//! `20261116000000_add_user_admin_theme`: each user's admin theme.

use orbit_orm::schema;
use orbit_orm::Migration;

/// The admin theme a user chose, by name, such as `wp`. NULL, the default, renders the site's
/// default theme (`set_admin_theme`). It is a separate column from `theme`, which is the light
/// or dark mode, so the two choices never overwrite each other.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261116000000_add_user_admin_theme",
        vec![schema::table("users", |t| {
            t.string("admin_theme").length(64).nullable();
        })],
        vec![schema::table("users", |t| {
            t.drop_column("admin_theme");
        })],
    )
}
