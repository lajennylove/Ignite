//! `20261113000000_roles_permissions`: roles, permissions and who holds them.

use orbit_orm::schema::{self, Action};
use orbit_orm::Migration;

/// The one row this migration writes: the super admin role.
const SEED_ROLE: &str = "INSERT INTO roles (name) VALUES ('super-admin')";

/// Users who exist before roles are introduced keep full access, so an upgrade does not lock
/// anyone out. Users created later get no roles until `ignite user role add` gives them one.
const GRANT_EXISTING_USERS: &str = "INSERT INTO role_user (user_id, role_id) \
     SELECT users.id, roles.id FROM users CROSS JOIN roles WHERE roles.name = 'super-admin'";

/// Roles and permissions, the Spatie laravel-permission shape that Filament Shield uses. A
/// permission is a named ability such as `users.viewAny`. The panel registers the permissions
/// of each resource when the app starts, and the `ignite` command grants them to roles and
/// roles to users.
pub(super) fn migration() -> Migration {
    Migration::schema(
        "20261113000000_roles_permissions",
        vec![
            schema::create_if_not_exists("roles", |t| {
                t.id();
                t.string("name");
                t.unique(["name"]).name("roles_name_unique");
            }),
            schema::create_if_not_exists("permissions", |t| {
                t.id();
                t.string("name");
                t.unique(["name"]).name("permissions_name_unique");
            }),
            schema::create_if_not_exists("role_permission", |t| {
                t.big_integer("role_id");
                t.big_integer("permission_id");
                t.primary(["role_id", "permission_id"]);
                t.foreign("role_id")
                    .references("roles", "id")
                    .on_delete(Action::Cascade)
                    .name("role_permission_role_fk");
                t.foreign("permission_id")
                    .references("permissions", "id")
                    .on_delete(Action::Cascade)
                    .name("role_permission_permission_fk");
            }),
            schema::create_if_not_exists("role_user", |t| {
                t.big_integer("user_id");
                t.big_integer("role_id");
                t.primary(["user_id", "role_id"]);
                t.foreign("user_id")
                    .references("users", "id")
                    .on_delete(Action::Cascade)
                    .name("role_user_user_fk");
                t.foreign("role_id")
                    .references("roles", "id")
                    .on_delete(Action::Cascade)
                    .name("role_user_role_fk");
            }),
            schema::statement(SEED_ROLE),
            schema::statement(GRANT_EXISTING_USERS),
        ],
        vec![
            schema::drop_if_exists("role_user"),
            schema::drop_if_exists("role_permission"),
            schema::drop_if_exists("permissions"),
            schema::drop_if_exists("roles"),
        ],
    )
}
