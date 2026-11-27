//! The panel's Users and Roles screens: WordPress's Users screens (the list with its role
//! views, "Add New User" and "Edit User"), built as resources the way a Filament app builds a
//! `UserResource`, and a Roles screen for the roles and their abilities, as Filament Shield's.
//!
//! Each screen is a [`RuntimeResource`] whose rows come from the panel's tables, so an app
//! extends it with the same builder as any runtime resource (its fields, list columns, tabs and
//! row actions) and adds it with [`Ignite::runtime_resource`](crate::Ignite::runtime_resource),
//! or adds both as they are with [`Ignite::user_management`](crate::Ignite::user_management):
//!
//! ```ignore
//! Ignite::new()
//!     .runtime_resource(users_resource().navigation_group("People").column(ColumnDef {
//!         key: "created_at".into(), label: "Registered".into(), sortable: false, searchable: false,
//!     }))
//!     .runtime_resource(roles_resource())
//! ```
//!
//! An app replaces a screen with a resource of its own at the same slug (`users` or `roles`)
//! instead. The screens' abilities are the resources' own: `users.viewAny`, `users.create`,
//! `users.update` (also disable and enable), `users.delete`, and the same for `roles`.
//!
//! A user who is not a super-admin cannot give or take the `super-admin` role, nor disable,
//! delete or change a super-admin, and no one can disable or delete their own account here,
//! as WordPress keeps the current user's Delete link off the list. The last enabled
//! super-admin keeps the role, as on the command line.

use std::collections::HashMap;

use ignite_core::{
    escape_like, ActingUser, ActionAbility, BadgeColor, ColumnDef, Database, Direction, FieldDef,
    FieldErrors, FieldKindDef, FilterCondition, FormInput, ListFilter, ListQuery, Operation, Page,
    Row, RowAction, RuntimeData, RuntimeFuture, RuntimeResource, Tab, ValidationError,
};
use orbit_orm::{raw, Direction as OrbitDirection, Error, Model, Operator, Value as OrbitValue};
use serde_json::{json, Value};

use crate::authz::{self, AuthzError, SUPER_ADMIN};
use crate::profile::MIN_PASSWORD_CHARS;
use crate::roles::role_label;
use crate::{sessions, users};

/// The slug of the Users screen.
pub const USERS_SLUG: &str = "users";
/// The slug of the Roles screen.
pub const ROLES_SLUG: &str = "roles";

/// The `status` of an enabled user on the Users screen.
pub const STATUS_ACTIVE: &str = "Active";
/// The `status` of a disabled user on the Users screen.
pub const STATUS_DISABLED: &str = "Disabled";

fn field(name: &str, label: &str, kind: FieldKindDef, required: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind,
        required,
    }
}

fn column(key: &str, label: &str, sortable: bool, searchable: bool) -> ColumnDef {
    ColumnDef {
        key: key.into(),
        label: label.into(),
        sortable,
        searchable,
    }
}

/// An error of the role functions as a database error, for a data method that cannot refuse.
fn database_error(error: AuthzError) -> Error {
    match error {
        AuthzError::Database(error) => error,
        other => Error::Database(orbit_orm::__sqlx::Error::Protocol(other.to_string())),
    }
}

/// The names in a relationship field's value: the JSON list of the validated row, or none.
fn names_of(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(text)) => serde_json::from_str::<Vec<String>>(text).unwrap_or_default(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn text_of<'a>(row: &'a Row, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The Users screen, as WordPress's Users screens: the list (name, email, roles and status,
/// with a search, a view per role, and Disable, Enable and Delete on each row), the form to add
/// a user (name, email, password and roles) and the form to edit one (a blank password keeps
/// the stored one). Its rows come from [`UsersData`]. Listed in the navigation as "Users", with
/// its "Add new" link, under the `users` icon.
pub fn users_resource() -> RuntimeResource {
    RuntimeResource::new(USERS_SLUG, "Users", "User", UsersData)
        .field(field("name", "Name", FieldKindDef::Text, true))
        .field(field("email", "Email", FieldKindDef::Email, true))
        .field(field("password", "Password", FieldKindDef::Password, true))
        .field(field("roles", "Roles", FieldKindDef::Relationship, false))
        .column(column("name", "Name", true, true))
        .column(column("email", "Email", true, true))
        .column(column("roles", "Roles", false, false))
        .column(column("status", "Status", false, false))
        .navigation_icon(crate::theme::icons::USERS)
        .badge_color("status", |status| match status {
            STATUS_ACTIVE => BadgeColor::Success,
            _ => BadgeColor::Gray,
        })
        // Delete is a row action, so a refusal (the last super-admin, the user's own account)
        // shows as a message instead of an error page.
        .hide_default_delete_when(ListFilter::new())
        .row_action(
            RowAction::make("disable")
                .requires_confirmation()
                .modal_description("Disable this user? They are signed out and cannot sign in.")
                .visible_when(ListFilter::new().where_eq("status", STATUS_ACTIVE))
                .action(|db, user, id, record| {
                    Box::pin(async move {
                        if let Err(message) = may_change(db, user, id, "disable").await? {
                            return Ok(Err(message));
                        }
                        Ok(
                            match authz::disable_user(db, text_of(record, "email")).await {
                                Ok(_) => Ok(()),
                                Err(AuthzError::Database(error)) => return Err(error),
                                Err(refused) => Err(refused.to_string()),
                            },
                        )
                    })
                })
                .success_notification_title("User disabled."),
        )
        .row_action(
            RowAction::make("enable")
                .visible_when(ListFilter::new().where_eq("status", STATUS_DISABLED))
                .action(|db, user, id, record| {
                    Box::pin(async move {
                        if let Err(message) = may_change(db, user, id, "enable").await? {
                            return Ok(Err(message));
                        }
                        authz::enable_user(db, text_of(record, "email"))
                            .await
                            .map_err(database_error)?;
                        Ok(Ok(()))
                    })
                })
                .success_notification_title("User enabled."),
        )
        .row_action(
            RowAction::make("delete")
                .color(BadgeColor::Danger)
                .authorize(ActionAbility::Delete)
                .requires_confirmation()
                .modal_description("Delete this user? This cannot be undone.")
                .action(|db, user, id, record| {
                    Box::pin(async move {
                        if let Err(message) = may_change(db, user, id, "delete").await? {
                            return Ok(Err(message));
                        }
                        Ok(
                            match authz::delete_user(db, text_of(record, "email")).await {
                                Ok(()) => Ok(()),
                                Err(AuthzError::Database(error)) => return Err(error),
                                Err(refused) => Err(refused.to_string()),
                            },
                        )
                    })
                })
                .success_notification_title("User deleted."),
        )
}

/// Whether `user` may disable, enable or delete the user `id`: not their own account, and a
/// super-admin only by a super-admin. `Ok(Err(message))` refuses.
async fn may_change(
    db: &Database,
    user: &ActingUser,
    id: i64,
    verb: &str,
) -> Result<Result<(), String>, Error> {
    if id == user.id() {
        return Ok(Err(format!("You cannot {verb} your own account.")));
    }
    if !user.is_super_admin() && is_super_admin(db, id).await? {
        return Ok(Err(format!("Only a super-admin can {verb} a super-admin.")));
    }
    Ok(Ok(()))
}

async fn is_super_admin(db: &Database, id: i64) -> Result<bool, Error> {
    Ok(authz::roles_of(db, id)
        .await
        .map_err(database_error)?
        .iter()
        .any(|role| role == SUPER_ADMIN))
}

/// The rows of the Users screen: the panel's `users` table, with each user's roles. Reuse it
/// in an app's own users resource.
///
/// A row has `id`, `name`, `email`, `roles` (the role labels joined with `, ` in the list, and
/// the role names as a JSON list for the edit form) and `status` ([`STATUS_ACTIVE`] or
/// [`STATUS_DISABLED`]). The list's filter reads the conditions `role` = a role name and
/// `status` = a status; the search matches the name and the email.
#[derive(Debug, Clone, Copy, Default)]
pub struct UsersData;

impl UsersData {
    async fn roles_by_user(db: &Database, ids: &[i64]) -> Result<HashMap<i64, Vec<String>>, Error> {
        let mut roles: HashMap<i64, Vec<String>> = HashMap::new();
        if ids.is_empty() {
            return Ok(roles);
        }
        let marks = vec!["?"; ids.len()].join(", ");
        let binds: Vec<OrbitValue> = ids.iter().map(|id| (*id).into()).collect();
        let rows: Vec<(i64, String)> = raw::fetch_all(
            db,
            &format!(
                "SELECT role_user.user_id AS user_id, roles.name AS name FROM role_user \
                 INNER JOIN roles ON roles.id = role_user.role_id \
                 WHERE role_user.user_id IN ({marks}) ORDER BY roles.name"
            ),
            &binds,
        )
        .await?;
        for (user_id, name) in rows {
            roles.entry(user_id).or_default().push(name);
        }
        Ok(roles)
    }

    fn row(user: &users::User, roles: Value) -> Row {
        let status = if user.disabled_at.is_some() {
            STATUS_DISABLED
        } else {
            STATUS_ACTIVE
        };
        json!({
            "id": user.id,
            "name": user.name,
            "email": user.email,
            "roles": roles,
            "status": status,
        })
        .as_object()
        .cloned()
        .unwrap_or_default()
    }

    async fn record(db: &Database, id: i64) -> Result<Option<Row>, Error> {
        let Some(user) = users::find_by_id(db, id).await? else {
            return Ok(None);
        };
        let roles = authz::roles_of(db, id).await.map_err(database_error)?;
        Ok(Some(Self::row(&user, json!(roles))))
    }

    /// The ids of the users with the role `role`.
    async fn ids_with_role(db: &Database, role: &str) -> Result<Vec<i64>, Error> {
        let rows: Vec<(i64,)> = raw::fetch_all(
            db,
            "SELECT role_user.user_id AS user_id FROM role_user \
             INNER JOIN roles ON roles.id = role_user.role_id WHERE roles.name = ?",
            &[role.into()],
        )
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// The roles `user` may give: every role, but `super-admin` only for a super-admin.
    async fn assignable(db: &Database, user: &ActingUser) -> Result<Vec<String>, Error> {
        Ok(authz::roles(db)
            .await
            .map_err(database_error)?
            .into_iter()
            .filter(|role| role != SUPER_ADMIN || user.is_super_admin())
            .collect())
    }

    /// The roles to store for a user from the form's `chosen`: a super-admin role the acting
    /// user may not take away stays.
    async fn roles_to_store(
        db: &Database,
        user: &ActingUser,
        target: Option<i64>,
        chosen: Vec<String>,
    ) -> Result<Vec<String>, Error> {
        let mut roles: Vec<String> = chosen
            .into_iter()
            .filter(|role| role != SUPER_ADMIN || user.is_super_admin())
            .collect();
        if let Some(id) = target {
            if !user.is_super_admin() && is_super_admin(db, id).await? {
                roles.push(SUPER_ADMIN.to_string());
            }
        }
        Ok(roles)
    }
}

impl RuntimeData for UsersData {
    fn list<'a>(&'a self, db: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move {
            let mut role_ids: Option<Vec<i64>> = None;
            let mut status: Option<bool> = None;
            for condition in query.filter.conditions() {
                if let FilterCondition::Eq(key, Value::String(value)) = condition {
                    match key.as_str() {
                        "role" => role_ids = Some(Self::ids_with_role(db, value).await?),
                        "status" => status = Some(value == STATUS_DISABLED),
                        _ => {}
                    }
                }
            }
            let search = query
                .search
                .as_ref()
                .map(|text| format!("%{}%", escape_like(text)));
            let base = || {
                let mut q = users::User::query();
                if let Some(ids) = &role_ids {
                    q = q.where_in("id", ids.iter().copied());
                }
                match status {
                    Some(true) => q = q.where_not_null("disabled_at"),
                    Some(false) => q = q.where_null("disabled_at"),
                    None => {}
                }
                if let Some(pattern) = &search {
                    q = q.where_group(|g| {
                        g.where_op("name", Operator::Like, pattern.as_str())
                            .or_where_op("email", Operator::Like, pattern.as_str())
                    });
                }
                q
            };
            let total = base().count(db).await?;
            if query.per_page == 0 {
                return Ok(Page {
                    rows: Vec::new(),
                    total,
                });
            }
            let direction = match query.sort {
                Some((_, Direction::Desc)) => OrbitDirection::Desc,
                _ => OrbitDirection::Asc,
            };
            let order = match query.sort.as_ref().map(|(key, _)| key.as_str()) {
                Some("name") => "name",
                Some("email") => "email",
                _ => "id",
            };
            let found = base()
                .order_by(order, direction)
                .order_by("id", OrbitDirection::Asc)
                .limit(query.per_page as u64)
                .offset(query.offset() as u64)
                .get(db)
                .await?;
            let ids: Vec<i64> = found.iter().map(|user| user.id).collect();
            let roles = Self::roles_by_user(db, &ids).await?;
            let rows = found
                .iter()
                .map(|user| {
                    let labels: Vec<String> = roles
                        .get(&user.id)
                        .map(|names| names.iter().map(|name| role_label(name)).collect())
                        .unwrap_or_default();
                    Self::row(user, json!(labels.join(", ")))
                })
                .collect();
            Ok(Page { rows, total })
        })
    }

    fn find<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(Self::record(db, id))
    }

    fn create<'a>(&'a self, db: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            let user = users::create_user_with_hash(
                db,
                text_of(&input, "name"),
                text_of(&input, "email"),
                text_of(&input, "password"),
            )
            .await?;
            authz::sync_user_roles(db, user.id, &names_of(input.get("roles")))
                .await
                .map_err(database_error)?;
            Self::record(db, user.id)
                .await?
                .ok_or(Error::NotFound { table: "users" })
        })
    }

    fn update<'a>(&'a self, db: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            let mut values: Vec<(&str, OrbitValue)> = Vec::new();
            for key in ["name", "email"] {
                if let Some(Value::String(text)) = input.get(key) {
                    values.push((key, text.as_str().into()));
                }
            }
            if !values.is_empty() {
                users::User::query()
                    .where_eq("id", id)
                    .update(db, &values)
                    .await?;
            }
            if let Some(Value::String(hash)) = input.get("password") {
                users::set_password_hash(db, id, hash).await?;
            }
            if input.contains_key("roles") {
                authz::sync_user_roles(db, id, &names_of(input.get("roles")))
                    .await
                    .map_err(database_error)?;
            }
            Self::record(db, id)
                .await?
                .ok_or(Error::NotFound { table: "users" })
        })
    }

    fn delete<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async move {
            let Some(user) = users::find_by_id(db, id).await? else {
                return Ok(0);
            };
            authz::delete_user(db, &user.email)
                .await
                .map_err(database_error)?;
            Ok(1)
        })
    }

    fn options<'a>(
        &'a self,
        db: &'a Database,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        Box::pin(async move {
            if field != "roles" {
                return Ok(Vec::new());
            }
            let roles = authz::roles(db).await.map_err(database_error)?;
            Ok(roles
                .into_iter()
                .map(|role| (role.clone(), role_label(&role)))
                .collect())
        })
    }

    fn options_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        Box::pin(async move {
            if field != "roles" {
                return Ok(Vec::new());
            }
            Ok(Self::assignable(db, user)
                .await?
                .into_iter()
                .map(|role| (role.clone(), role_label(&role)))
                .collect())
        })
    }

    fn find_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        id: i64,
    ) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async move {
            let Some(mut row) = Self::record(db, id).await? else {
                return Ok(None);
            };
            // The form shows only the roles the user may give; a super-admin role they may
            // not change is kept on save.
            if !user.is_super_admin() {
                let roles: Vec<String> = names_of(row.get("roles"))
                    .into_iter()
                    .filter(|role| role != SUPER_ADMIN)
                    .collect();
                row.insert("roles".into(), json!(roles));
            }
            Ok(Some(row))
        })
    }

    fn try_create_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        mut input: Row,
    ) -> RuntimeFuture<'a, Result<Row, ValidationError>> {
        Box::pin(async move {
            if users::email_taken(db, text_of(&input, "email"), 0).await? {
                return Ok(Err(ValidationError::with_messages([(
                    "email",
                    "Another user already has this email.",
                )])));
            }
            let roles = Self::roles_to_store(db, user, None, names_of(input.get("roles"))).await?;
            input.insert("roles".into(), json!(roles));
            self.create(db, input).await.map(Ok)
        })
    }

    fn try_update_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        id: i64,
        mut input: Row,
    ) -> RuntimeFuture<'a, Result<Row, ValidationError>> {
        Box::pin(async move {
            if !user.is_super_admin() && id != user.id() && is_super_admin(db, id).await? {
                return Ok(Err(ValidationError::with_messages([(
                    "name",
                    "Only a super-admin can change a super-admin.",
                )])));
            }
            if let Some(Value::String(email)) = input.get("email") {
                if users::email_taken(db, email, id).await? {
                    return Ok(Err(ValidationError::with_messages([(
                        "email",
                        "Another user already has this email.",
                    )])));
                }
            }
            if input.contains_key("roles") {
                let roles =
                    Self::roles_to_store(db, user, Some(id), names_of(input.get("roles"))).await?;
                // The last enabled super-admin keeps the role, as on the command line.
                if !roles.iter().any(|role| role == SUPER_ADMIN)
                    && is_super_admin(db, id).await?
                    && last_enabled_super_admin(db, id).await?
                {
                    return Ok(Err(ValidationError::with_messages([(
                        "roles",
                        AuthzError::LastSuperAdmin.to_string(),
                    )])));
                }
                input.insert("roles".into(), json!(roles));
            }
            let changed_password = input.contains_key("password");
            let row = self.update(db, id, input).await?;
            // A new password signs the user out elsewhere, unless it is the user's own.
            if changed_password && id != user.id() {
                sessions::delete_for_user(db, id).await?;
            }
            Ok(Ok(row))
        })
    }

    fn tabs_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
    ) -> RuntimeFuture<'a, Option<Vec<Tab>>> {
        let _ = user;
        Box::pin(async move {
            // WordPress's views of the Users list: All, then one per role.
            let mut tabs = vec![Tab::make("all").label("All")];
            for role in authz::roles(db).await.map_err(database_error)? {
                if role == "all" {
                    continue;
                }
                tabs.push(
                    Tab::make(role.clone())
                        .label(role_label(&role))
                        .where_eq("role", role),
                );
            }
            Ok(Some(tabs))
        })
    }

    fn validate_form(&self, form: &FormInput, operation: Operation) -> FieldErrors {
        let mut errors = FieldErrors::new();
        let password = form.get("password").unwrap_or_default();
        let checked = operation == Operation::Create || !password.is_empty();
        if checked && !password.is_empty() && password.chars().count() < MIN_PASSWORD_CHARS {
            errors.insert(
                "password".to_string(),
                format!("The password must be at least {MIN_PASSWORD_CHARS} characters."),
            );
        }
        errors
    }
}

/// Whether the user `id` is the only enabled super-admin.
async fn last_enabled_super_admin(db: &Database, id: i64) -> Result<bool, Error> {
    let others: i64 = raw::fetch_scalar(
        db,
        "SELECT COUNT(*) FROM role_user \
         INNER JOIN roles ON roles.id = role_user.role_id \
         INNER JOIN users ON users.id = role_user.user_id \
         WHERE roles.name = ? AND role_user.user_id <> ? AND users.disabled_at IS NULL",
        &[SUPER_ADMIN.into(), id.into()],
    )
    .await?;
    Ok(others == 0)
}

/// The Roles screen: the list of roles (name, label, number of users and of abilities), the
/// form to create a role (its name and abilities) and the form to edit one, whose abilities
/// are every permission the panel knows: each resource's (`<slug>.viewAny`, `.view`,
/// `.create`, `.update`, `.delete`) and the app's own. Delete is a row action, refused for
/// `super-admin`. Its rows come from [`RolesData`].
pub fn roles_resource() -> RuntimeResource {
    RuntimeResource::new(ROLES_SLUG, "Roles", "Role", RolesData)
        .field(field("name", "Name", FieldKindDef::Text, true))
        .field(field(
            "abilities",
            "Abilities",
            FieldKindDef::Relationship,
            false,
        ))
        .column(column("name", "Name", true, true))
        .column(column("label", "Label", false, false))
        .column(column("users", "Users", false, false))
        .column(column("abilities", "Abilities", false, false))
        .navigation_icon(crate::theme::icons::USERS)
        .hide_default_delete_when(ListFilter::new())
        .row_action(
            RowAction::make("delete")
                .color(BadgeColor::Danger)
                .authorize(ActionAbility::Delete)
                .requires_confirmation()
                .modal_description("Delete this role? Its users lose it.")
                .action(|db, _, _, record| {
                    Box::pin(async move {
                        Ok(
                            match authz::delete_role(db, text_of(record, "name")).await {
                                Ok(_) => Ok(()),
                                Err(AuthzError::Database(error)) => return Err(error),
                                Err(refused) => Err(refused.to_string()),
                            },
                        )
                    })
                })
                .success_notification_title("Role deleted."),
        )
}

/// The rows of the Roles screen: the panel's `roles` table, with each role's abilities. A row
/// has `id`, `name`, `label`, `users` (how many users have it), and `abilities` (how many in
/// the list, the names as a JSON list for the edit form).
#[derive(Debug, Clone, Copy, Default)]
pub struct RolesData;

impl RolesData {
    async fn record(db: &Database, id: i64) -> Result<Option<Row>, Error> {
        let found: Vec<(String,)> =
            raw::fetch_all(db, "SELECT name FROM roles WHERE id = ?", &[id.into()]).await?;
        let Some((name,)) = found.into_iter().next() else {
            return Ok(None);
        };
        let abilities = authz::role_permissions(db, &name)
            .await
            .map_err(database_error)?;
        Ok(json!({
            "id": id,
            "name": name,
            "label": role_label(&name),
            "abilities": abilities,
        })
        .as_object()
        .cloned())
    }

    fn refused(error: AuthzError) -> Result<Result<Row, ValidationError>, Error> {
        match error {
            AuthzError::Database(error) => Err(error),
            other => Ok(Err(ValidationError::with_messages([(
                "name",
                other.to_string(),
            )]))),
        }
    }
}

impl RuntimeData for RolesData {
    fn list<'a>(&'a self, db: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move {
            let (filter, binds): (&str, Vec<OrbitValue>) = match &query.search {
                Some(text) => (
                    " WHERE roles.name LIKE ?",
                    vec![format!("%{}%", escape_like(text)).into()],
                ),
                None => ("", Vec::new()),
            };
            let total: i64 =
                raw::fetch_scalar(db, &format!("SELECT COUNT(*) FROM roles{filter}"), &binds)
                    .await?;
            if query.per_page == 0 {
                return Ok(Page {
                    rows: Vec::new(),
                    total,
                });
            }
            let direction = match query.sort {
                Some((_, Direction::Desc)) => "DESC",
                _ => "ASC",
            };
            let rows: Vec<(i64, String, i64, i64)> = raw::fetch_all(
                db,
                &format!(
                    "SELECT roles.id AS id, roles.name AS name, \
                     (SELECT COUNT(*) FROM role_user WHERE role_user.role_id = roles.id) AS users, \
                     (SELECT COUNT(*) FROM role_permission \
                      WHERE role_permission.role_id = roles.id) AS abilities \
                     FROM roles{filter} ORDER BY roles.name {direction} LIMIT {} OFFSET {}",
                    query.per_page.max(0),
                    query.offset().max(0)
                ),
                &binds,
            )
            .await?;
            let rows = rows
                .into_iter()
                .map(|(id, name, users, abilities)| {
                    json!({
                        "id": id,
                        "label": role_label(&name),
                        "name": name,
                        "users": users,
                        "abilities": abilities,
                    })
                    .as_object()
                    .cloned()
                    .unwrap_or_default()
                })
                .collect();
            Ok(Page { rows, total })
        })
    }

    fn find<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(Self::record(db, id))
    }

    fn create<'a>(&'a self, db: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            let name = text_of(&input, "name");
            authz::create_role(db, name).await.map_err(database_error)?;
            authz::sync_role_permissions(db, name, &names_of(input.get("abilities")))
                .await
                .map_err(database_error)?;
            let id: i64 =
                raw::fetch_scalar(db, "SELECT id FROM roles WHERE name = ?", &[name.into()])
                    .await?;
            Self::record(db, id)
                .await?
                .ok_or(Error::NotFound { table: "roles" })
        })
    }

    fn update<'a>(&'a self, db: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move {
            let current = Self::record(db, id)
                .await?
                .ok_or(Error::NotFound { table: "roles" })?;
            let mut name = text_of(&current, "name").to_string();
            if let Some(Value::String(to)) = input.get("name") {
                authz::rename_role(db, &name, to)
                    .await
                    .map_err(database_error)?;
                name = to.clone();
            }
            if input.contains_key("abilities") {
                authz::sync_role_permissions(db, &name, &names_of(input.get("abilities")))
                    .await
                    .map_err(database_error)?;
            }
            Self::record(db, id)
                .await?
                .ok_or(Error::NotFound { table: "roles" })
        })
    }

    fn delete<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async move {
            let Some(row) = Self::record(db, id).await? else {
                return Ok(0);
            };
            let deleted = authz::delete_role(db, text_of(&row, "name"))
                .await
                .map_err(database_error)?;
            Ok(u64::from(deleted))
        })
    }

    fn options<'a>(
        &'a self,
        db: &'a Database,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        Box::pin(async move {
            if field != "abilities" {
                return Ok(Vec::new());
            }
            let names = authz::permissions(db).await.map_err(database_error)?;
            Ok(names.into_iter().map(|name| (name.clone(), name)).collect())
        })
    }

    fn try_create_for<'a>(
        &'a self,
        db: &'a Database,
        _: &'a ActingUser,
        input: Row,
    ) -> RuntimeFuture<'a, Result<Row, ValidationError>> {
        Box::pin(async move {
            let name = text_of(&input, "name");
            if authz::roles(db)
                .await
                .map_err(database_error)?
                .iter()
                .any(|role| role == name)
            {
                return Self::refused(AuthzError::Exists(name.to_string()));
            }
            self.create(db, input).await.map(Ok)
        })
    }

    fn try_update_for<'a>(
        &'a self,
        db: &'a Database,
        _: &'a ActingUser,
        id: i64,
        input: Row,
    ) -> RuntimeFuture<'a, Result<Row, ValidationError>> {
        Box::pin(async move {
            let current = Self::record(db, id)
                .await?
                .ok_or(Error::NotFound { table: "roles" })?;
            let from = text_of(&current, "name");
            if let Some(Value::String(to)) = input.get("name") {
                if to != from {
                    if from == SUPER_ADMIN || to == SUPER_ADMIN {
                        return Self::refused(AuthzError::SuperAdminRole);
                    }
                    if authz::roles(db)
                        .await
                        .map_err(database_error)?
                        .iter()
                        .any(|role| role == to)
                    {
                        return Self::refused(AuthzError::Exists(to.clone()));
                    }
                }
            }
            self.update(db, id, input).await.map(Ok)
        })
    }

    fn validate_form(&self, form: &FormInput, operation: Operation) -> FieldErrors {
        let _ = operation;
        let mut errors = FieldErrors::new();
        let name = form.get("name").unwrap_or_default().trim();
        if !name.is_empty() && !authz::valid_role_name(name) {
            errors.insert(
                "name".to_string(),
                "Use lowercase letters, digits, - and _, such as author.".to_string(),
            );
        }
        errors
    }
}
