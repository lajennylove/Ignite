//! The roles an app declares, with the abilities each one starts with, as WordPress's
//! `add_role( $role, $display_name, $capabilities )` and the roles a Filament Shield app seeds.

use std::sync::{PoisonError, RwLock};

/// A role the app declares, with its label and its default abilities. Pass it to
/// [`Ignite::role`](crate::Ignite::role):
///
/// ```ignore
/// Ignite::new()
///     .role(Role::make("author").label("Author").abilities(["posts.viewAny", "posts.create"]))
///     .role(Role::make("administrator").label("Administrator").ability("settings.viewAny"))
/// ```
///
/// When the app starts, the panel registers the abilities as permissions and creates each role
/// that does not exist yet with its abilities. A role that exists keeps the abilities it has,
/// as WordPress's `add_role` does nothing for a role that exists, so a change made on the
/// Roles screen is kept when the app starts again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    name: String,
    label: Option<String>,
    abilities: Vec<String>,
}

impl Role {
    /// A role named `name`, such as `author`: lowercase letters, digits, `-` and `_`.
    pub fn make(name: impl Into<String>) -> Self {
        Role {
            name: name.into(),
            label: None,
            abilities: Vec::new(),
        }
    }

    /// The role's name as the screens show it, such as `Author`, as WordPress's display name.
    /// The default is the name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Adds an ability the role starts with, such as `posts.create`: a resource's ability
    /// (`<slug>.viewAny`, `.view`, `.create`, `.update`, `.delete`) or one of the app's own.
    pub fn ability(mut self, ability: impl Into<String>) -> Self {
        self.abilities.push(ability.into());
        self
    }

    /// Adds several abilities the role starts with.
    pub fn abilities<I, S>(mut self, abilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.abilities.extend(abilities.into_iter().map(Into::into));
        self
    }

    /// The role's name.
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// The role's label: the one set, or the name.
    pub fn get_label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }

    /// The abilities the role starts with.
    pub fn get_abilities(&self) -> &[String] {
        &self.abilities
    }
}

/// The labels of the declared roles, by name. Process-wide, as the user callbacks are: the
/// Users and Roles screens read them with only a database. Set by the builder.
static LABELS: RwLock<Vec<(String, String)>> = RwLock::new(Vec::new());

/// Makes the labels of `roles` the ones the screens show.
pub(crate) fn install_labels(roles: &[Role]) {
    *LABELS.write().unwrap_or_else(PoisonError::into_inner) = roles
        .iter()
        .map(|role| (role.get_name().to_string(), role.get_label().to_string()))
        .collect();
}

/// The label of the role `name`: its declared label, `Super Admin` for `super-admin`, or the
/// name.
pub fn role_label(name: &str) -> String {
    let labels = LABELS.read().unwrap_or_else(PoisonError::into_inner);
    match labels.iter().find(|(known, _)| known == name) {
        Some((_, label)) => label.clone(),
        None if name == crate::authz::SUPER_ADMIN => "Super Admin".to_string(),
        None => name.to_string(),
    }
}
