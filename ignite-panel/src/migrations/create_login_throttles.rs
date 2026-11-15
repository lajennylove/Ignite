//! `20261108020000_create_login_throttles`: failed sign-in attempts.

use orbit_orm::schema;
use orbit_orm::Migration;

/// Failed sign-in attempts, counted per key (an email or an IP) in a window. The table lives
/// in the database, so the count holds across restarts and processes.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261108020000_create_login_throttles",
        vec![schema::create_if_not_exists("login_throttles", |t| {
            t.string("throttle_key").primary();
            t.integer("hits");
            t.date_time("window_start");
        })],
        vec![schema::drop_if_exists("login_throttles")],
    )
}
