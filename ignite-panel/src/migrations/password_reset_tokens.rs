//! `20261115000000_password_reset_tokens`: password reset links.

use orbit_orm::schema;
use orbit_orm::Migration;

/// One row for each email with a link out. The token is stored as a hash, as Laravel stores
/// it, so a copy of the table cannot reset a password. A new link replaces the row, and a used
/// or expired link is deleted. The time keeps microseconds.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261115000000_password_reset_tokens",
        vec![schema::create_if_not_exists("password_reset_tokens", |t| {
            t.string("email").primary();
            t.string("token");
            t.date_time("created_at").precision(6);
        })],
        vec![schema::drop_if_exists("password_reset_tokens")],
    )
}
