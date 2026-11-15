//! `20261108010000_create_sessions`: server-side sign-in sessions.

use orbit_orm::schema;
use orbit_orm::Migration;

/// The signed cookie holds only the session id; the row says whose session it is and when it
/// ends.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261108010000_create_sessions",
        vec![schema::create_if_not_exists("sessions", |t| {
            t.string("id").length(64).primary();
            t.big_integer("user_id");
            t.date_time("expires_at");
            t.date_time("created_at").use_current();
            t.index(["user_id"]).name("sessions_user_index");
        })],
        vec![schema::drop_if_exists("sessions")],
    )
}
