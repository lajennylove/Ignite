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

use orbit_orm::{raw, Database, Error, Executor, Model, OrbitModel};

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

/// A row of the `roles` table. Its names are unique.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "roles")]
#[allow(dead_code)]
struct Role {
    id: i64,
    #[orbit(fillable)]
    name: String,
}

/// A row of the `permissions` table. Its names are unique.
#[derive(Debug, Clone, OrbitModel)]
#[orbit(table = "permissions")]
#[allow(dead_code)]
struct Permission {
    id: i64,
    #[orbit(fillable)]
    name: String,
}

/// Whether the user with `user_id` holds the super-admin role. Reads only.
async fn is_super_admin<'a>(db: impl Executor<'a>, user_id: i64) -> Result<bool, Error> {
    let roles: Vec<(String,)> = raw::fetch_all(
        db,
        "SELECT roles.name AS name FROM roles \
         INNER JOIN role_user ON role_user.role_id = roles.id \
         WHERE role_user.user_id = ? AND roles.name = ?",
        &[user_id.into(), SUPER_ADMIN.into()],
    )
    .await?;
    Ok(!roles.is_empty())
}

/// Reads the roles and permissions of the user with `user_id`. Reads only.
pub(crate) async fn load(db: &Database, user_id: i64) -> Result<Abilities, Error> {
    let roles: Vec<(String,)> = raw::fetch_all(
        db,
        "SELECT roles.name AS name FROM roles \
         INNER JOIN role_user ON role_user.role_id = roles.id \
         WHERE role_user.user_id = ?",
        &[user_id.into()],
    )
    .await?;
    let permissions: Vec<(String,)> = raw::fetch_all(
        db,
        "SELECT DISTINCT permissions.name AS name FROM permissions \
         INNER JOIN role_permission ON role_permission.permission_id = permissions.id \
         INNER JOIN role_user ON role_user.role_id = role_permission.role_id \
         WHERE role_user.user_id = ?",
        &[user_id.into()],
    )
    .await?;
    Ok(Abilities {
        super_admin: roles.iter().any(|(name,)| name == SUPER_ADMIN),
        names: permissions.into_iter().map(|(name,)| name).collect(),
    })
}

/// Makes sure each ability has a row in `permissions`. Running it again changes nothing.
/// The panel calls it when the app starts, so a grant can name any of its abilities.
pub(crate) async fn register(db: &Database, names: &[String]) -> Result<(), Error> {
    db.transaction(async |tx| {
        for name in names {
            raw::execute(
                &mut *tx,
                "INSERT INTO permissions (name) VALUES (?) ON DUPLICATE KEY UPDATE name = name",
                &[name.as_str().into()],
            )
            .await?;
        }
        Ok::<(), Error>(())
    })
    .await
}

/// Registers the abilities when the app starts. A failure is logged and the app still starts,
/// so the panel can be migrated after the first start. Until the rows exist, no grant can name
/// the abilities, and nobody is allowed them.
pub(crate) async fn register_at_start(db: &Database, names: Vec<String>) {
    if let Err(error) = register(db, &names).await {
        rocket::error!("ignite: could not register permissions: {error}");
    }
}

/// A problem with a role command. Its message is the one the command prints.
#[derive(Debug)]
pub enum AuthzError {
    /// A role, permission or user does not exist. The text names it.
    NotFound(String),
    /// A role with this name already exists.
    Exists(String),
    /// The change would leave no enabled super-admin, so the user who would lose the role is kept.
    LastSuperAdmin,
    /// Disabling the user would leave no enabled super-admin, so the user is kept enabled.
    CannotDisableLastSuperAdmin,
    /// The database failed.
    Database(Error),
}

impl fmt::Display for AuthzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthzError::NotFound(what) => write!(f, "{what} does not exist."),
            AuthzError::Exists(name) => write!(f, "A role named {name} already exists."),
            AuthzError::LastSuperAdmin => write!(
                f,
                "This is the last super-admin who is enabled, so the role cannot be removed. \
                 Give super-admin to another enabled user first."
            ),
            AuthzError::CannotDisableLastSuperAdmin => write!(
                f,
                "This is the last super-admin who is enabled, so the user cannot be disabled. \
                 Give super-admin to another enabled user first."
            ),
            AuthzError::Database(error) => write!(f, "The database failed: {error}"),
        }
    }
}

impl std::error::Error for AuthzError {}

impl From<Error> for AuthzError {
    fn from(error: Error) -> Self {
        AuthzError::Database(error)
    }
}

async fn role_id<'a>(db: impl Executor<'a>, name: &str) -> Result<Option<i64>, Error> {
    Ok(Role::query()
        .where_eq("name", name)
        .first(db)
        .await?
        .map(|role| role.id))
}

async fn permission_id<'a>(db: impl Executor<'a>, name: &str) -> Result<Option<i64>, Error> {
    Ok(Permission::query()
        .where_eq("name", name)
        .first(db)
        .await?
        .map(|permission| permission.id))
}

/// Creates a role. Its name is unique.
pub async fn create_role(db: &Database, name: &str) -> Result<(), AuthzError> {
    if role_id(db, name).await?.is_some() {
        return Err(AuthzError::Exists(name.to_string()));
    }
    Role::query().insert(db, &[("name", name.into())]).await?;
    Ok(())
}

/// Gives the role `role` the permission `permission`, such as `users.delete`. Both must
/// exist. A permission exists once the app has started with its resource. Granting it
/// again changes nothing.
pub async fn grant_permission(
    db: &Database,
    role: &str,
    permission: &str,
) -> Result<(), AuthzError> {
    let role_row = role_id(db, role)
        .await?
        .ok_or_else(|| AuthzError::NotFound(format!("The role {role}")))?;
    let permission_row = permission_id(db, permission).await?.ok_or_else(|| {
        AuthzError::NotFound(format!(
            "The permission {permission}. Start the app once so the panel registers \
             its permissions, then try again"
        ))
    })?;
    raw::execute(
        db,
        "INSERT IGNORE INTO role_permission (role_id, permission_id) VALUES (?, ?)",
        &[role_row.into(), permission_row.into()],
    )
    .await?;
    Ok(())
}

/// Gives the user with `email` the role `role`. Giving a role twice changes nothing.
pub async fn assign_role(db: &Database, email: &str, role: &str) -> Result<(), AuthzError> {
    let (user_row, role_row) = user_and_role(db, email, role).await?;
    raw::execute(
        db,
        "INSERT IGNORE INTO role_user (user_id, role_id) VALUES (?, ?)",
        &[user_row.into(), role_row.into()],
    )
    .await?;
    Ok(())
}

/// Takes the role `role` from the user with `email`. Returns `false` when the user did not
/// have it. Refuses to remove super-admin from the last enabled user who has it, so nobody is locked out.
pub async fn remove_role(db: &Database, email: &str, role: &str) -> Result<bool, AuthzError> {
    let (user_row, role_row) = user_and_role(db, email, role).await?;
    db.transaction(async |tx| {
        // Locking the role row serializes two removals, so both cannot pass the count.
        raw::execute(
            &mut *tx,
            "SELECT id FROM roles WHERE id = ? FOR UPDATE",
            &[role_row.into()],
        )
        .await?;
        let holds: Vec<(i64,)> = raw::fetch_all(
            &mut *tx,
            "SELECT role_id AS id FROM role_user WHERE user_id = ? AND role_id = ?",
            &[user_row.into(), role_row.into()],
        )
        .await?;
        if holds.is_empty() {
            return Ok(false);
        }
        if role == SUPER_ADMIN && enabled_super_admins_besides(&mut *tx, user_row).await? == 0 {
            return Err(AuthzError::LastSuperAdmin);
        }
        raw::execute(
            &mut *tx,
            "DELETE FROM role_user WHERE user_id = ? AND role_id = ?",
            &[user_row.into(), role_row.into()],
        )
        .await?;
        Ok(true)
    })
    .await
}

/// How many enabled users other than `user_id` hold super-admin. A disabled user cannot
/// sign in, so they do not keep the panel open for anyone.
async fn enabled_super_admins_besides<'a>(
    db: impl Executor<'a>,
    user_id: i64,
) -> Result<i64, Error> {
    let rows: Vec<(i64,)> = raw::fetch_all(
        db,
        "SELECT COUNT(*) AS count FROM role_user \
         INNER JOIN roles ON roles.id = role_user.role_id \
         INNER JOIN users ON users.id = role_user.user_id \
         WHERE roles.name = ? AND role_user.user_id <> ? AND users.disabled_at IS NULL",
        &[SUPER_ADMIN.into(), user_id.into()],
    )
    .await?;
    Ok(rows.first().map_or(0, |row| row.0))
}

/// Disables the user with `email`: they cannot sign in, and their sessions end. Returns
/// `false` when the user was already disabled. Refuses to disable the last enabled super-admin.
pub async fn disable_user(db: &Database, email: &str) -> Result<bool, AuthzError> {
    let user = find_user(db, email).await?;
    if user.disabled_at.is_some() {
        return Ok(false);
    }
    db.transaction(async |tx| {
        // The same lock as `remove_role`, so two changes to the super-admins cannot both pass.
        raw::execute(
            &mut *tx,
            "SELECT id FROM roles WHERE name = ? FOR UPDATE",
            &[SUPER_ADMIN.into()],
        )
        .await?;
        if is_super_admin(&mut *tx, user.id).await?
            && enabled_super_admins_besides(&mut *tx, user.id).await? == 0
        {
            return Err(AuthzError::CannotDisableLastSuperAdmin);
        }
        users::disable_in(tx, user.id).await?;
        Ok(true)
    })
    .await
}

/// Enables the user with `email` again. Returns `false` when the user was already enabled.
pub async fn enable_user(db: &Database, email: &str) -> Result<bool, AuthzError> {
    let user = find_user(db, email).await?;
    if user.disabled_at.is_none() {
        return Ok(false);
    }
    users::enable(db, user.id).await?;
    Ok(true)
}

/// The user with `email`. The user must exist.
async fn find_user(db: &Database, email: &str) -> Result<users::User, AuthzError> {
    users::find_by_email(db, email)
        .await?
        .ok_or_else(|| AuthzError::NotFound(format!("The user {email}")))
}

/// The names of the roles of the user with `email`, in name order.
pub async fn user_roles(db: &Database, email: &str) -> Result<Vec<String>, AuthzError> {
    let user = find_user(db, email).await?;
    let rows: Vec<(String,)> = raw::fetch_all(
        db,
        "SELECT roles.name AS name FROM roles \
         INNER JOIN role_user ON role_user.role_id = roles.id \
         WHERE role_user.user_id = ? ORDER BY roles.name",
        &[user.id.into()],
    )
    .await?;
    Ok(rows.into_iter().map(|(name,)| name).collect())
}

/// The ids of the user with `email` and of the role `role`. Both must exist.
async fn user_and_role(db: &Database, email: &str, role: &str) -> Result<(i64, i64), AuthzError> {
    let user = find_user(db, email).await?;
    let role_row = role_id(db, role)
        .await?
        .ok_or_else(|| AuthzError::NotFound(format!("The role {role}")))?;
    Ok((user.id, role_row))
}
