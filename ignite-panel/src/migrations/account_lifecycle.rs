//! `20261114000000_account_lifecycle`: disabled users, and sessions that go with their user.

use orbit_orm::schema::{self, Action};
use orbit_orm::Migration;

/// Rows left by an earlier delete have no user to belong to, so they go before the key is
/// added.
const DELETE_ORPHAN_SESSIONS: &str =
    "DELETE FROM sessions WHERE user_id NOT IN (SELECT id FROM users)";

/// A disabled user cannot sign in, and their sessions end. NULL means the user is enabled.
/// Deleting a user deletes their sessions with them.
///
/// SQLite cannot add a foreign key to an existing table, so on SQLite the schema builder
/// renders the session key, and its drop, as nothing: the sessions table has no key there.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261114000000_account_lifecycle",
        vec![
            schema::table("users", |t| {
                t.date_time("disabled_at").nullable();
            }),
            schema::statement(DELETE_ORPHAN_SESSIONS),
            schema::table("sessions", |t| {
                t.foreign("user_id")
                    .references("users", "id")
                    .on_delete(Action::Cascade)
                    .name("sessions_user_fk");
            }),
        ],
        vec![
            schema::table("sessions", |t| {
                t.drop_foreign("sessions_user_fk");
            }),
            schema::table("users", |t| {
                t.drop_column("disabled_at");
            }),
        ],
    )
}
