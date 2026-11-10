//! Roles and permissions: which signed-in users may view, create, update and delete
//! each resource. The model follows Filament, and its standard plugin Filament Shield,
//! which is built on Spatie laravel-permission:
//!
//! - Every resource has five abilities, named `<slug>.<action>`: `viewAny` (the list and
//!   the navigation entry), `view`, `create` (the new form and its save), `update` (the
//!   edit form and its save) and `delete`. The panel registers them when the app starts.
//! - A user has the abilities of their roles. A role holds permissions, and a user holds
//!   roles, in the tables `roles`, `permissions`, `role_permission` and `role_user`.
//! - The role `super-admin` passes every check.
//!
//! The `ignite` command manages roles: see [`crate::cli`]. The routes check the abilities
//! on the server, and the navigation and buttons show only what the user may do.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use ignite_core::Pool;

use crate::erased::Erased;
use crate::users;

/// The role that passes every check, as Filament Shield's super admin does.
pub const SUPER_ADMIN: &str = "super-admin";

/// What a user may do with a resource. Each resource has one ability per action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// See the list and the navigation entry.
    ViewAny,
    /// See one record. The panel has no detail page yet, so nothing checks this.
    View,
    /// Open the new form and save it.
    Create,
    /// Open the edit form and save it.
    Update,
    /// Delete a record.
    Delete,
}

/// Every action, in the order the panel registers them.
const ACTIONS: [Action; 5] = [
    Action::ViewAny,
    Action::View,
    Action::Create,
    Action::Update,
    Action::Delete,
];

impl Action {
    fn suffix(self) -> &'static str {
        match self {
            Action::ViewAny => "viewAny",
            Action::View => "view",
            Action::Create => "create",
            Action::Update => "update",
            Action::Delete => "delete",
        }
    }
}

/// The ability name of `action` on the resource at `slug`, such as `users.viewAny`.
pub(crate) fn ability(slug: &str, action: Action) -> String {
    format!("{slug}.{}", action.suffix())
}

/// Every ability of the resources, for the permission rows the panel registers.
pub(crate) fn resource_abilities(resources: &[Arc<dyn Erased>]) -> Vec<String> {
    resources
        .iter()
        .flat_map(|resource| ACTIONS.map(|action| ability(resource.slug(), action)))
        .collect()
}

/// The abilities one user holds, from their roles. Loaded with the session on each request.
#[derive(Debug, Clone, Default)]
pub(crate) struct Abilities {
    super_admin: bool,
    names: HashSet<String>,
}

impl Abilities {
    /// Whether the user holds the ability `name`, or is a super-admin.
    pub(crate) fn allows(&self, name: &str) -> bool {
        self.super_admin || self.names.contains(name)
    }
}

#[derive(QueryableByName)]
struct NameRow {
    #[diesel(sql_type = Text)]
    name: String,
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = BigInt)]
    id: i64,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

/// Reads the roles and permissions of the user with `user_id`. Reads only.
pub(crate) fn load(conn: &mut MysqlConnection, user_id: i64) -> Result<Abilities, DieselError> {
    let roles: Vec<NameRow> = sql_query(
        "SELECT roles.name AS name FROM roles \
         INNER JOIN role_user ON role_user.role_id = roles.id \
         WHERE role_user.user_id = ?",
    )
    .bind::<BigInt, _>(user_id)
    .load(conn)?;
    let permissions: Vec<NameRow> = sql_query(
        "SELECT DISTINCT permissions.name AS name FROM permissions \
         INNER JOIN role_permission ON role_permission.permission_id = permissions.id \
         INNER JOIN role_user ON role_user.role_id = role_permission.role_id \
         WHERE role_user.user_id = ?",
    )
    .bind::<BigInt, _>(user_id)
    .load(conn)?;
    Ok(Abilities {
        super_admin: roles.iter().any(|role| role.name == SUPER_ADMIN),
        names: permissions.into_iter().map(|row| row.name).collect(),
    })
}

/// Makes sure each ability has a row in `permissions`. Running it again changes nothing.
/// The panel calls it when the app starts, so a grant can name any of its abilities.
pub(crate) fn register(conn: &mut MysqlConnection, names: &[String]) -> Result<(), DieselError> {
    conn.transaction(|conn| {
        for name in names {
            sql_query(
                "INSERT INTO permissions (name) VALUES (?) ON DUPLICATE KEY UPDATE name = name",
            )
            .bind::<Text, _>(name)
            .execute(conn)?;
        }
        Ok(())
    })
}

/// Registers the abilities on a blocking thread when the app starts. A failure is logged
/// and the app still starts, so the panel can be migrated after the first start. Until the
/// rows exist, no grant can name the abilities, and nobody is allowed them.
pub(crate) async fn register_at_start(pool: Pool, names: Vec<String>) {
    let outcome = rocket::tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        register(&mut conn, &names).map_err(|e| e.to_string())
    })
    .await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => rocket::error!("ignite: could not register permissions: {error}"),
        Err(error) => rocket::error!("ignite: could not register permissions: {error}"),
    }
}

/// A problem with a role command. Its message is the one the command prints.
#[derive(Debug)]
pub enum AuthzError {
    /// A role, permission or user does not exist. The text names it.
    NotFound(String),
    /// A role with this name already exists.
    Exists(String),
    /// The change would leave no super-admin, so the user who would lose the role is kept.
    LastSuperAdmin,
    /// The database failed.
    Database(DieselError),
}

impl fmt::Display for AuthzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthzError::NotFound(what) => write!(f, "{what} does not exist."),
            AuthzError::Exists(name) => write!(f, "A role named {name} already exists."),
            AuthzError::LastSuperAdmin => write!(
                f,
                "This is the last super-admin, so the role cannot be removed. \
                 Give super-admin to another user first."
            ),
            AuthzError::Database(error) => write!(f, "The database failed: {error}"),
        }
    }
}

impl std::error::Error for AuthzError {}

impl From<DieselError> for AuthzError {
    fn from(error: DieselError) -> Self {
        AuthzError::Database(error)
    }
}

fn role_id(conn: &mut MysqlConnection, name: &str) -> Result<Option<i64>, DieselError> {
    let rows: Vec<IdRow> = sql_query("SELECT id FROM roles WHERE name = ?")
        .bind::<Text, _>(name)
        .load(conn)?;
    Ok(rows.first().map(|row| row.id))
}

fn permission_id(conn: &mut MysqlConnection, name: &str) -> Result<Option<i64>, DieselError> {
    let rows: Vec<IdRow> = sql_query("SELECT id FROM permissions WHERE name = ?")
        .bind::<Text, _>(name)
        .load(conn)?;
    Ok(rows.first().map(|row| row.id))
}

/// Creates a role. Its name is unique.
pub fn create_role(conn: &mut MysqlConnection, name: &str) -> Result<(), AuthzError> {
    if role_id(conn, name)?.is_some() {
        return Err(AuthzError::Exists(name.to_string()));
    }
    sql_query("INSERT INTO roles (name) VALUES (?)")
        .bind::<Text, _>(name)
        .execute(conn)?;
    Ok(())
}

/// Gives the role `role` the permission `permission`, such as `users.delete`. Both must
/// exist. A permission exists once the app has started with its resource. Granting it
/// again changes nothing.
pub fn grant_permission(
    conn: &mut MysqlConnection,
    role: &str,
    permission: &str,
) -> Result<(), AuthzError> {
    let role_row =
        role_id(conn, role)?.ok_or_else(|| AuthzError::NotFound(format!("The role {role}")))?;
    let permission_row = permission_id(conn, permission)?.ok_or_else(|| {
        AuthzError::NotFound(format!(
            "The permission {permission}. Start the app once so the panel registers \
             its permissions, then try again"
        ))
    })?;
    sql_query("INSERT IGNORE INTO role_permission (role_id, permission_id) VALUES (?, ?)")
        .bind::<BigInt, _>(role_row)
        .bind::<BigInt, _>(permission_row)
        .execute(conn)?;
    Ok(())
}

/// Gives the user with `email` the role `role`. Giving a role twice changes nothing.
pub fn assign_role(conn: &mut MysqlConnection, email: &str, role: &str) -> Result<(), AuthzError> {
    let (user_row, role_row) = user_and_role(conn, email, role)?;
    sql_query("INSERT IGNORE INTO role_user (user_id, role_id) VALUES (?, ?)")
        .bind::<BigInt, _>(user_row)
        .bind::<BigInt, _>(role_row)
        .execute(conn)?;
    Ok(())
}

/// Takes the role `role` from the user with `email`. Returns `false` when the user did not
/// have it. Refuses to remove super-admin from the last user who has it, so nobody is locked out.
pub fn remove_role(
    conn: &mut MysqlConnection,
    email: &str,
    role: &str,
) -> Result<bool, AuthzError> {
    let (user_row, role_row) = user_and_role(conn, email, role)?;
    conn.transaction(|conn| {
        // Locking the super-admin role row serializes two removals, so both cannot pass the count.
        sql_query("SELECT id FROM roles WHERE id = ? FOR UPDATE")
            .bind::<BigInt, _>(role_row)
            .execute(conn)?;
        let holds: Vec<IdRow> =
            sql_query("SELECT role_id AS id FROM role_user WHERE user_id = ? AND role_id = ?")
                .bind::<BigInt, _>(user_row)
                .bind::<BigInt, _>(role_row)
                .load(conn)?;
        if holds.is_empty() {
            return Ok(false);
        }
        if role == SUPER_ADMIN {
            let holders: Vec<CountRow> =
                sql_query("SELECT COUNT(*) AS count FROM role_user WHERE role_id = ?")
                    .bind::<BigInt, _>(role_row)
                    .load(conn)?;
            if holders.first().is_some_and(|row| row.count <= 1) {
                return Err(AuthzError::LastSuperAdmin);
            }
        }
        sql_query("DELETE FROM role_user WHERE user_id = ? AND role_id = ?")
            .bind::<BigInt, _>(user_row)
            .bind::<BigInt, _>(role_row)
            .execute(conn)?;
        Ok(true)
    })
}

/// The names of the roles of the user with `email`, in name order.
pub fn user_roles(conn: &mut MysqlConnection, email: &str) -> Result<Vec<String>, AuthzError> {
    let user = users::find_by_email(conn, email)?
        .ok_or_else(|| AuthzError::NotFound(format!("The user {email}")))?;
    let rows: Vec<NameRow> = sql_query(
        "SELECT roles.name AS name FROM roles \
         INNER JOIN role_user ON role_user.role_id = roles.id \
         WHERE role_user.user_id = ? ORDER BY roles.name",
    )
    .bind::<BigInt, _>(user.id)
    .load(conn)?;
    Ok(rows.into_iter().map(|row| row.name).collect())
}

/// The ids of the user with `email` and of the role `role`. Both must exist.
fn user_and_role(
    conn: &mut MysqlConnection,
    email: &str,
    role: &str,
) -> Result<(i64, i64), AuthzError> {
    let user = users::find_by_email(conn, email)?
        .ok_or_else(|| AuthzError::NotFound(format!("The user {email}")))?;
    let role_row =
        role_id(conn, role)?.ok_or_else(|| AuthzError::NotFound(format!("The role {role}")))?;
    Ok((user.id, role_row))
}
