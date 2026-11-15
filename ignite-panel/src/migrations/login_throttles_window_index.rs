//! `20261112000000_login_throttles_window_index`: an index on the throttle window.

use orbit_orm::schema;
use orbit_orm::Migration;

/// Lets the panel delete the rows whose window has ended without scanning the table.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261112000000_login_throttles_window_index",
        vec![schema::table("login_throttles", |t| {
            t.index(["window_start"])
                .name("login_throttles_window_start_index");
        })],
        vec![schema::table("login_throttles", |t| {
            t.drop_index("login_throttles_window_start_index");
        })],
    )
}
