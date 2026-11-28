//! Who is acting, and what they may do with each record: the signed-in user the panel passes
//! to a resource's queries, and the policies that decide each action for a user and a record.
//!
//! The model follows Laravel and Filament:
//!
//! - The panel builds an [`ActingUser`] from the session on each request, as Laravel's
//!   `auth()->user()`, and passes it to the resource's queries: the `*_for` methods of
//!   [`RuntimeData`](crate::RuntimeData) and [`IgniteResource`](crate::IgniteResource). A
//!   resource restricts its list and its records to the user there, as a Filament resource
//!   does in `getEloquentQuery()`. A record the user's query does not find is 404.
//! - A [`Policy`] decides each action for the user and the record, as a Laravel policy does:
//!   `view_any`, `view`, `create`, `update` and `delete`. It is registered for a resource's
//!   slug with [`register_policy`], as `Gate::policy` maps a model to its policy. A resource
//!   without a policy is decided by its abilities alone, as before.
//!
//! ```ignore
//! struct OwnBooks;
//!
//! impl Policy for OwnBooks {
//!     fn update<'a>(&'a self, _: &'a Database, user: &'a ActingUser, book: &'a Row)
//!         -> RuntimeFuture<'a, bool> {
//!         let own = book.get("author_id").and_then(|v| v.as_i64()) == Some(user.id());
//!         Box::pin(async move { Ok(own) })
//!     }
//! }
//!
//! register_policy("books", OwnBooks);
//! ```

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, PoisonError, RwLock};

use orbit_orm::Database;

use crate::runtime_resource::{Row, RuntimeFuture};

/// The signed-in user an action is done for: the panel builds it from the session on each
/// request and passes it to the resource's queries and to its [`Policy`]. It holds the user's
/// id, name and email, and the abilities their roles grant (such as `books.update`).
///
/// An app builds one itself only to call a resource or a policy outside the panel, such as in
/// a test, with [`new`](Self::new), [`with_ability`](Self::with_ability) and
/// [`with_super_admin`](Self::with_super_admin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActingUser {
    id: i64,
    name: String,
    email: String,
    super_admin: bool,
    abilities: BTreeSet<String>,
}

impl ActingUser {
    /// The user with `id`, `name` and `email`, with no abilities and no super-admin role.
    pub fn new(id: i64, name: impl Into<String>, email: impl Into<String>) -> Self {
        ActingUser {
            id,
            name: name.into(),
            email: email.into(),
            super_admin: false,
            abilities: BTreeSet::new(),
        }
    }

    /// Adds the ability `name`, such as `books.update`.
    pub fn with_ability(mut self, name: impl Into<String>) -> Self {
        self.abilities.insert(name.into());
        self
    }

    /// Sets whether the user holds the `super-admin` role.
    pub fn with_super_admin(mut self, super_admin: bool) -> Self {
        self.super_admin = super_admin;
        self
    }

    /// The user's id, the `id` of their row in `users`.
    pub fn id(&self) -> i64 {
        self.id
    }

    /// The user's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The user's email.
    pub fn email(&self) -> &str {
        &self.email
    }

    /// Whether the user holds the `super-admin` role, which passes every ability check and
    /// every policy, as Filament Shield's super admin does.
    pub fn is_super_admin(&self) -> bool {
        self.super_admin
    }

    /// Whether the user may do `ability`, such as `books.update`: their roles grant it, or
    /// they are a super-admin.
    pub fn can(&self, ability: &str) -> bool {
        self.super_admin || self.abilities.contains(ability)
    }
}

/// An action on a resource as a whole, as the methods of a Filament resource policy
/// (`viewAny`, `view`, `create`, `update`, `delete`): each needs the resource's ability for it,
/// `<slug>.<action>` unless the resource names another (see
/// [`RuntimeResource::ability`](crate::RuntimeResource::ability)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceAction {
    /// See the list and the navigation entry, as `viewAny`.
    ViewAny,
    /// See one record, as `view`.
    View,
    /// Open the new form and save it, as `create`.
    Create,
    /// Open the edit form and save it, as `update`.
    Update,
    /// Delete a record, as `delete`.
    Delete,
}

impl ResourceAction {
    /// Every action, in the order the panel registers their abilities.
    pub const ALL: [ResourceAction; 5] = [
        ResourceAction::ViewAny,
        ResourceAction::View,
        ResourceAction::Create,
        ResourceAction::Update,
        ResourceAction::Delete,
    ];

    /// The action's name as the policy method and the default ability spell it, such as
    /// `"viewAny"`.
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceAction::ViewAny => "viewAny",
            ResourceAction::View => "view",
            ResourceAction::Create => "create",
            ResourceAction::Update => "update",
            ResourceAction::Delete => "delete",
        }
    }
}

/// What a user may do with the records of one resource, decided for the user and, where there
/// is one, the record, as a Laravel policy decides. Register it for the resource's slug with
/// [`register_policy`]. The record is the row as the resource returns it: a
/// [`RuntimeData`](crate::RuntimeData)'s [`Row`], or a static resource's record serialized to
/// JSON.
///
/// The panel asks the policy after the user's ability (`<slug>.<action>`), as a policy that
/// Filament Shield generates checks the permission first: an action needs both. A super-admin
/// passes without asking, as Shield's super admin passes before any policy. Every method
/// allows by default, so a policy writes only the actions it restricts. An error is a server
/// error (500), not a refusal.
///
/// - [`view_any`](Self::view_any): the list, and the resource in the navigation and on the
///   dashboard.
/// - [`view`](Self::view): one record. The panel has no detail page yet, so nothing asks it,
///   as nothing checks the `view` ability.
/// - [`create`](Self::create): the new form and its save.
/// - [`update`](Self::update): the edit form and its save, and the Edit button of the record.
/// - [`delete`](Self::delete): the delete action, and the Delete button of the record.
///
/// A refusal answers 403 Forbidden. Which records a user sees at all is the resource's query,
/// not the policy: see [`ActingUser`].
pub trait Policy: Send + Sync + 'static {
    /// Whether `user` may see the list of the resource.
    fn view_any<'a>(&'a self, db: &'a Database, user: &'a ActingUser) -> RuntimeFuture<'a, bool> {
        let _ = (db, user);
        Box::pin(async { Ok(true) })
    }

    /// Whether `user` may see `record`.
    fn view<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        record: &'a Row,
    ) -> RuntimeFuture<'a, bool> {
        let _ = (db, user, record);
        Box::pin(async { Ok(true) })
    }

    /// Whether `user` may create a record.
    fn create<'a>(&'a self, db: &'a Database, user: &'a ActingUser) -> RuntimeFuture<'a, bool> {
        let _ = (db, user);
        Box::pin(async { Ok(true) })
    }

    /// Whether `user` may change `record`.
    fn update<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        record: &'a Row,
    ) -> RuntimeFuture<'a, bool> {
        let _ = (db, user, record);
        Box::pin(async { Ok(true) })
    }

    /// Whether `user` may delete `record`.
    fn delete<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        record: &'a Row,
    ) -> RuntimeFuture<'a, bool> {
        let _ = (db, user, record);
        Box::pin(async { Ok(true) })
    }
}

/// The policies, by the slug of their resource.
static POLICIES: RwLock<Option<HashMap<String, Arc<dyn Policy>>>> = RwLock::new(None);

/// Registers `policy` for the resource at `slug`, static or runtime, as Laravel's
/// `Gate::policy` maps a model to its policy. It takes effect from the next request on, and
/// replaces the policy the slug had. The slug need not be registered yet: a policy is kept when
/// its runtime resource is removed and registered again.
pub fn register_policy(slug: impl Into<String>, policy: impl Policy) {
    let mut guard = POLICIES.write().unwrap_or_else(PoisonError::into_inner);
    guard
        .get_or_insert_with(HashMap::new)
        .insert(slug.into(), Arc::new(policy));
}

/// Removes the policy of the resource at `slug`, so its abilities alone decide again. Returns
/// whether it had one.
pub fn unregister_policy(slug: &str) -> bool {
    let mut guard = POLICIES.write().unwrap_or_else(PoisonError::into_inner);
    guard
        .as_mut()
        .is_some_and(|policies| policies.remove(slug).is_some())
}

/// The policy of the resource at `slug`, if one is registered.
pub fn policy(slug: &str) -> Option<Arc<dyn Policy>> {
    let guard = POLICIES.read().unwrap_or_else(PoisonError::into_inner);
    guard
        .as_ref()
        .and_then(|policies| policies.get(slug).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_can_what_their_roles_grant() {
        let user = ActingUser::new(7, "Ada", "ada@example.test").with_ability("books.update");
        assert_eq!(user.id(), 7);
        assert_eq!(user.name(), "Ada");
        assert_eq!(user.email(), "ada@example.test");
        assert!(user.can("books.update"));
        assert!(!user.can("books.delete"));
        assert!(!user.is_super_admin());
    }

    #[test]
    fn a_super_admin_can_everything() {
        let user = ActingUser::new(1, "Root", "root@example.test").with_super_admin(true);
        assert!(user.is_super_admin());
        assert!(user.can("books.delete"));
        assert!(user.can("anything.at.all"));
    }

    // The panel's tests check that a policy's unwritten methods allow, on a database.
    struct Nothing;

    impl Policy for Nothing {}

    #[test]
    fn a_policy_is_registered_replaced_and_removed_by_slug() {
        assert!(policy("policy-unit-books").is_none());
        register_policy("policy-unit-books", Nothing);
        register_policy("policy-unit-books", Nothing);
        assert!(policy("policy-unit-books").is_some());
        assert!(policy("policy-unit-other").is_none());
        assert!(unregister_policy("policy-unit-books"));
        assert!(!unregister_policy("policy-unit-books"));
        assert!(policy("policy-unit-books").is_none());
    }
}
