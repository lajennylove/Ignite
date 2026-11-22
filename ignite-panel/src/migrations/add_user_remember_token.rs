//! `20261117000000_add_user_remember_token`: the token of a "Remember me" sign-in.

use orbit_orm::schema;
use orbit_orm::Migration;

/// The user's remember token, as Laravel's `$table->rememberToken()`: a string of up to 100
/// characters. NULL, the default, until the user first signs in with "Remember me" ticked. A
/// new token, set on sign-out and on a password change, ends every remembered sign-in.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261117000000_add_user_remember_token",
        vec![schema::table("users", |t| {
            t.string("remember_token").length(100).nullable();
        })],
        vec![schema::table("users", |t| {
            t.drop_column("remember_token");
        })],
    )
}
