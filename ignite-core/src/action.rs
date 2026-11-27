//! Row actions, as Filament's table actions (`$table->recordActions([...])`): buttons on each
//! row of a resource's list that open a URL or run a task on the record, such as Restore,
//! Move to trash and Delete permanently.
//!
//! ```ignore
//! RuntimeResource::new("posts", "Posts", "Post", PostData)
//!     .row_action(
//!         RowAction::make("restore")
//!             .visible_when(ListFilter::new().where_eq("status", "trash"))
//!             .action(|db, user, id, record| Box::pin(async move {
//!                 restore(db, id).await?;
//!                 Ok(Ok(()))
//!             }))
//!             .success_notification_title("Post restored."),
//!     )
//!     .row_action(
//!         RowAction::make("delete_permanently")
//!             .color(BadgeColor::Danger)
//!             .authorize(ActionAbility::Delete)
//!             .requires_confirmation()
//!             .modal_description("Delete this post permanently? This cannot be undone.")
//!             .visible_when(ListFilter::new().where_eq("status", "trash"))
//!             .action(|db, _, id, _| Box::pin(async move {
//!                 delete(db, id).await?;
//!                 Ok(Ok(()))
//!             })),
//!     )
//! ```
//!
//! An action with [`url`](RowAction::url) is a link. An action with
//! [`action`](RowAction::action) is a form that posts to the panel, at
//! `{mount}/{slug}/{id}/actions/{name}`, with the CSRF token. The panel checks the token, the
//! user's ability on the resource ([`authorize`](RowAction::authorize): update by default, or
//! delete), the resource's policy for the record, and the action's visibility rule for the
//! record, then runs the handler. The handler answers `Ok(())`, or `Err(message)` to refuse:
//! the panel shows the message, as Filament's failure notification.

use std::fmt;
use std::sync::Arc;

use orbit_orm::Database;
use serde_json::Value;

use crate::list::{BadgeColor, ListFilter};
use crate::policy::ActingUser;
use crate::runtime_resource::{Row, RuntimeFuture};

/// What a row action's handler answers: `Ok(())` when it is done, or `Err(message)` when it
/// refuses, such as "This post is locked." The panel shows the message to the user.
pub type ActionResult = Result<(), String>;

/// The ability a row action needs on the resource, as Filament's `authorize('update')`: the
/// same check, ability and policy, as the resource's edit or delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionAbility {
    /// The ability to update the record, as the Edit button needs. The default.
    #[default]
    Update,
    /// The ability to delete the record, as the Delete button needs.
    Delete,
}

/// The default description of a confirmation, as Filament's `requiresConfirmation()` modal.
pub const DEFAULT_CONFIRMATION: &str = "Are you sure you would like to do this?";

type Handler = Arc<
    dyn for<'a> Fn(&'a Database, &'a ActingUser, i64, &'a Row) -> RuntimeFuture<'a, ActionResult>
        + Send
        + Sync,
>;
type Rule = Arc<dyn Fn(&Row) -> bool + Send + Sync>;

/// One action on each row of a resource's list, as Filament's `Action::make($name)` in
/// `recordActions()`. See the [module](self) documentation.
#[derive(Clone)]
pub struct RowAction {
    name: String,
    label: Option<String>,
    url: Option<String>,
    handler: Option<Handler>,
    confirmation: Option<String>,
    rules: Vec<Rule>,
    color: Option<BadgeColor>,
    ability: ActionAbility,
    success: Option<String>,
}

impl fmt::Debug for RowAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowAction")
            .field("name", &self.name)
            .field("label", &self.label)
            .field("url", &self.url)
            .field("posts", &self.handler.is_some())
            .field("confirmation", &self.confirmation)
            .field("rules", &self.rules.len())
            .field("color", &self.color)
            .field("ability", &self.ability)
            .field("success", &self.success)
            .finish()
    }
}

impl RowAction {
    /// An action named `name`, as Filament's `Action::make('restore')`. The name is the last
    /// segment of the action's address, so it should be a slug such as `delete_permanently`.
    /// Its label defaults to the name with `_` and `-` as spaces and a capital first letter:
    /// "Delete permanently".
    pub fn make(name: impl Into<String>) -> Self {
        RowAction {
            name: name.into(),
            label: None,
            url: None,
            handler: None,
            confirmation: None,
            rules: Vec::new(),
            color: None,
            ability: ActionAbility::Update,
            success: None,
        }
    }

    /// The button's text, as Filament's `label()`.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Makes the action a link to `template`, as Filament's `url()`. Each `{key}` in it is
    /// replaced by the record's value for `key`, URL-encoded, such as
    /// `"/blog/{slug}?preview=1"` or `"/admin/posts/{id}/revisions"`; a key the record does not
    /// have is replaced by nothing. A link needs no [`authorize`](Self::authorize): the page it
    /// opens checks access, as Filament says of `url()`.
    pub fn url(mut self, template: impl Into<String>) -> Self {
        self.url = Some(template.into());
        self
    }

    /// Makes the action a POST that runs `handler`, as Filament's `action()`. The handler is
    /// given the database, the signed-in user, the record's id and the record, and answers
    /// `Ok(Ok(()))` when done, `Ok(Err(message))` to refuse with a message, or `Err(error)` for
    /// a database error (a server error). Write it as
    /// `|db, user, id, record| Box::pin(async move { ... })`.
    pub fn action<F>(mut self, handler: F) -> Self
    where
        F: for<'a> Fn(
                &'a Database,
                &'a ActingUser,
                i64,
                &'a Row,
            ) -> RuntimeFuture<'a, ActionResult>
            + Send
            + Sync
            + 'static,
    {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Asks the user before the action runs, as Filament's `requiresConfirmation()`, with
    /// Filament's default text, "Are you sure you would like to do this?".
    pub fn requires_confirmation(mut self) -> Self {
        if self.confirmation.is_none() {
            self.confirmation = Some(DEFAULT_CONFIRMATION.to_string());
        }
        self
    }

    /// The question the confirmation asks, as Filament's `modalDescription()`. It also turns
    /// the confirmation on.
    pub fn modal_description(mut self, text: impl Into<String>) -> Self {
        self.confirmation = Some(text.into());
        self
    }

    /// Shows the action only on the rows for which `rule` holds, as Filament's `visible()`
    /// with a `$record` closure. The panel also refuses to run it on another record. Several
    /// rules must all hold.
    pub fn visible(mut self, rule: impl Fn(&Row) -> bool + Send + Sync + 'static) -> Self {
        self.rules.push(Arc::new(rule));
        self
    }

    /// Hides the action on the rows for which `rule` holds, as Filament's `hidden()`.
    pub fn hidden(mut self, rule: impl Fn(&Row) -> bool + Send + Sync + 'static) -> Self {
        self.rules.push(Arc::new(move |row: &Row| !rule(row)));
        self
    }

    /// Shows the action only on the rows that meet `filter`, such as
    /// `ListFilter::new().where_eq("status", "trash")`: [`visible`](Self::visible) for a rule
    /// that is data, as a runtime resource loads it.
    pub fn visible_when(self, filter: ListFilter) -> Self {
        self.visible(move |row| filter.matches(row))
    }

    /// The button's colour, as Filament's `color()`. `Danger` renders the panel's danger
    /// button; the others render its plain button, with the class `ig-action-<color>` for a
    /// theme. The default is none.
    pub fn color(mut self, color: BadgeColor) -> Self {
        self.color = Some(color);
        self
    }

    /// The ability the action needs, as Filament's `authorize('update')` or
    /// `authorize('delete')`. The default is [`ActionAbility::Update`].
    pub fn authorize(mut self, ability: ActionAbility) -> Self {
        self.ability = ability;
        self
    }

    /// The message shown after the action ran, as Filament's `successNotificationTitle()`.
    /// The default is none: the list shows again without a message.
    pub fn success_notification_title(mut self, title: impl Into<String>) -> Self {
        self.success = Some(title.into());
        self
    }

    /// The action's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The button's text: the label, or the name made readable.
    pub fn get_label(&self) -> String {
        match &self.label {
            Some(label) => label.clone(),
            None => {
                let words = self.name.replace(['_', '-'], " ");
                let mut chars = words.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            }
        }
    }

    /// Whether the action posts to the panel: it has a handler.
    pub fn is_post(&self) -> bool {
        self.handler.is_some()
    }

    /// The link of the action for `record`, when it is a link.
    pub fn url_for(&self, record: &Row) -> Option<String> {
        self.url.as_deref().map(|template| fill(template, record))
    }

    /// The confirmation's text, when the action asks first.
    pub fn confirmation(&self) -> Option<&str> {
        self.confirmation.as_deref()
    }

    /// Whether the action shows on `record`: every visibility rule holds.
    pub fn is_visible(&self, record: &Row) -> bool {
        self.rules.iter().all(|rule| rule(record))
    }

    /// The button's colour, if set.
    pub fn get_color(&self) -> Option<BadgeColor> {
        self.color
    }

    /// The ability the action needs.
    pub fn ability(&self) -> ActionAbility {
        self.ability
    }

    /// The message shown after the action ran, if set.
    pub fn success_message(&self) -> Option<&str> {
        self.success.as_deref()
    }

    /// Runs the handler on the record `id`. An action without a handler (a link) answers
    /// `Ok(Err(..))`; the panel does not call it then.
    pub async fn run(
        &self,
        db: &Database,
        user: &ActingUser,
        id: i64,
        record: &Row,
    ) -> Result<ActionResult, orbit_orm::Error> {
        match &self.handler {
            Some(handler) => handler(db, user, id, record).await,
            None => Ok(Err("This action is a link.".to_string())),
        }
    }
}

/// `template` with each `{key}` replaced by the record's value for `key`, percent-encoded but
/// for the URL's unreserved characters.
fn fill(template: &str, record: &Row) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        let key = &rest[open + 1..open + close];
        let value = match record.get(key) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        };
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(value: Value) -> Row {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn the_label_defaults_to_the_name_made_readable() {
        assert_eq!(
            RowAction::make("delete_permanently").get_label(),
            "Delete permanently"
        );
        assert_eq!(
            RowAction::make("move-to-trash").get_label(),
            "Move to trash"
        );
        assert_eq!(RowAction::make("x").label("Restore").get_label(), "Restore");
    }

    #[test]
    fn a_url_fills_the_record_values_encoded() {
        let action = RowAction::make("view").url("/blog/{slug}?id={id}&none={missing}");
        let record = row(json!({"id": 7, "slug": "a b&c"}));
        assert_eq!(
            action.url_for(&record).unwrap(),
            "/blog/a%20b%26c?id=7&none="
        );
        assert!(!action.is_post());
    }

    #[test]
    fn visibility_rules_all_hold() {
        let action = RowAction::make("restore")
            .visible_when(ListFilter::new().where_eq("status", "trash"))
            .hidden(|row| row.get("locked") == Some(&json!(true)));
        assert!(action.is_visible(&row(json!({"status": "trash"}))));
        assert!(!action.is_visible(&row(json!({"status": "publish"}))));
        assert!(!action.is_visible(&row(json!({"status": "trash", "locked": true}))));
    }

    #[test]
    fn confirmation_defaults_to_filaments_text() {
        let action = RowAction::make("trash").requires_confirmation();
        assert_eq!(action.confirmation(), Some(DEFAULT_CONFIRMATION));
        let action = RowAction::make("trash").modal_description("Sure?");
        assert_eq!(action.confirmation(), Some("Sure?"));
        assert_eq!(RowAction::make("trash").confirmation(), None);
    }
}
