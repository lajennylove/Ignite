//! `20261111000000_profile`: the profile picture and the app's profile fields.

use orbit_orm::schema::{self, Action};
use orbit_orm::Migration;

/// The picture's type on the user (NULL when the user keeps their initials), and the values
/// of the fields the app adds, as a JSON object. `users.avatar_mime` is the only record of the
/// picture's type. The picture itself is a file in the panel's storage directory, as WordPress
/// keeps uploads on disk: its own table keeps the file's path, relative to the storage
/// directory (such as `avatars/42.png`).
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261111000000_profile",
        vec![
            schema::table("users", |t| {
                t.string("avatar_mime").length(32).nullable();
                t.text("profile").nullable();
            }),
            schema::create_if_not_exists("user_avatars", |t| {
                t.big_integer("user_id").primary();
                t.string("path");
                t.foreign("user_id")
                    .references("users", "id")
                    .on_delete(Action::Cascade)
                    .name("user_avatars_user_fk");
            }),
        ],
        vec![
            schema::drop_if_exists("user_avatars"),
            schema::table("users", |t| {
                t.drop_column("avatar_mime");
                t.drop_column("profile");
            }),
        ],
    )
}
