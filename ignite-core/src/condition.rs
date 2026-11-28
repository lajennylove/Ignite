//! Fields that show only when a condition holds, as Filament's `visible()`, `hidden()`,
//! `visibleOn()`, `hiddenOn()` and `live()`, and fields shown but not editable, as its
//! `disabled()` and `disabledOn()`.
//!
//! A condition is a closure the server runs each time it builds or saves a form, given a
//! [`FormContext`]: the operation, the record being edited (none on create), the signed-in
//! user and the form's current values, as a Filament closure is given `$operation`, `$record`,
//! `auth()->user()` and `$get`. A field whose condition fails is hidden: the panel does not
//! render it, does not check it, and does not save it, so a request that sends it anyway
//! cannot change the stored value.
//!
//! A field marked [`live`](FieldConditions::live) re-renders the form when its value changes in
//! the browser: the panel's script sends the form to the server, which runs the conditions on
//! the new values and returns the fields to show.
//!
//! ```ignore
//! RuntimeResource::new("posts", "Posts", "Post", PostData)
//!     .field(kind_field)
//!     .field(company_field)
//!     .live("kind")
//!     .visible("company", |form| form.get("kind") == Some("business"))
//!     .hidden_on("slug", &[Operation::Create])
//!     .visible("featured", |form| form.user().can("posts.feature"));
//! ```

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::form::FormInput;
use crate::policy::ActingUser;
use crate::runtime_resource::{FieldDef, Row};

/// What the form is for, as Filament's `$operation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    /// The new-record form and its save.
    Create,
    /// The edit form of a record and its save.
    Edit,
}

impl Operation {
    /// The operation's name as Filament spells it: `"create"` or `"edit"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Create => "create",
            Operation::Edit => "edit",
        }
    }
}

/// What a condition is given: the operation, the record, the signed-in user and the form's
/// current values.
#[derive(Debug, Clone, Copy)]
pub struct FormContext<'a> {
    operation: Operation,
    record: Option<&'a Row>,
    user: &'a ActingUser,
    values: &'a FormInput,
}

impl<'a> FormContext<'a> {
    /// The context of a form for `operation`, editing `record` (none on create), for `user`,
    /// whose fields hold `values`. The panel builds it; an app builds one to test a condition.
    pub fn new(
        operation: Operation,
        record: Option<&'a Row>,
        user: &'a ActingUser,
        values: &'a FormInput,
    ) -> Self {
        FormContext {
            operation,
            record,
            user,
            values,
        }
    }

    /// Whether the form creates a record or edits one, as Filament's `$operation`.
    pub fn operation(&self) -> Operation {
        self.operation
    }

    /// The record being edited, as stored, as Filament's `$record`: a runtime resource's
    /// [`Row`], or a static resource's record serialized to JSON. `None` on create.
    pub fn record(&self) -> Option<&'a Row> {
        self.record
    }

    /// The signed-in user, as `auth()->user()` in a Filament closure.
    pub fn user(&self) -> &'a ActingUser {
        self.user
    }

    /// The current value of the field `name`, as Filament's `$get('name')`: the value the
    /// user has typed or chosen, or `None` when it is blank. When the form is first shown it
    /// is the stored value. A field that is itself hidden gives its stored value, never one a
    /// request sent for it. The value is not checked.
    pub fn get(&self, name: &str) -> Option<&'a str> {
        self.values.get(name)
    }

    /// Whether the checkbox `name` is ticked. See [`FormInput::checkbox`].
    pub fn checked(&self, name: &str) -> bool {
        self.values.checkbox(name)
    }
}

/// A condition on a field.
type Rule = Arc<dyn Fn(&FormContext<'_>) -> bool + Send + Sync>;

/// The conditions of a form's fields, by field name, and the fields that are live. Each
/// method takes the field's name and returns the conditions, so they chain.
///
/// As in Filament, a field has one `hidden` condition and one `visible` condition, and shows
/// only when its `hidden` condition fails and its `visible` condition holds. A later
/// [`hidden`](Self::hidden) or [`hidden_on`](Self::hidden_on) for the same field replaces the
/// earlier one, and so does a later [`visible`](Self::visible) or
/// [`visible_on`](Self::visible_on). A field with no condition always shows.
#[derive(Clone, Default)]
pub struct FieldConditions {
    hidden: HashMap<String, Rule>,
    visible: HashMap<String, Rule>,
    live: BTreeSet<String>,
    /// The field each slug field is filled from, by slug field name.
    slugs: HashMap<String, String>,
    /// When each slug field stops following its source, by slug field name.
    slug_locks: HashMap<String, Rule>,
    /// When each field is shown but not editable, by field name.
    disabled: HashMap<String, Rule>,
}

impl fmt::Debug for FieldConditions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = |rules: &HashMap<String, Rule>| rules.keys().cloned().collect::<BTreeSet<_>>();
        f.debug_struct("FieldConditions")
            .field("hidden", &names(&self.hidden))
            .field("visible", &names(&self.visible))
            .field("live", &self.live)
            .field("slugs", &self.slugs)
            .field("slug_locks", &names(&self.slug_locks))
            .field("disabled", &names(&self.disabled))
            .finish()
    }
}

impl FieldConditions {
    /// No conditions: every field shows, and none is live.
    pub fn new() -> Self {
        FieldConditions::default()
    }

    /// Shows `field` only when `rule` holds, as Filament's `->visible(fn () => ...)`.
    pub fn visible(
        mut self,
        field: impl Into<String>,
        rule: impl Fn(&FormContext<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.visible.insert(field.into(), Arc::new(rule));
        self
    }

    /// Hides `field` when `rule` holds, as Filament's `->hidden(fn () => ...)`.
    pub fn hidden(
        mut self,
        field: impl Into<String>,
        rule: impl Fn(&FormContext<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.hidden.insert(field.into(), Arc::new(rule));
        self
    }

    /// Shows `field` only on the forms of `operations`, as Filament's `->visibleOn('edit')`.
    pub fn visible_on(self, field: impl Into<String>, operations: &[Operation]) -> Self {
        let operations = operations.to_vec();
        self.visible(field, move |form| operations.contains(&form.operation()))
    }

    /// Hides `field` on the forms of `operations`, as Filament's `->hiddenOn('edit')`.
    pub fn hidden_on(self, field: impl Into<String>, operations: &[Operation]) -> Self {
        let operations = operations.to_vec();
        self.hidden(field, move |form| operations.contains(&form.operation()))
    }

    /// Shows `field` but does not let the user change it when `rule` holds, as Filament's
    /// `->disabled(fn () => ...)`: its input is disabled and, as in Filament, a disabled field
    /// is not saved, so its stored value is kept whatever a request sends. A later
    /// [`disabled`](Self::disabled) or [`disabled_on`](Self::disabled_on) for the field
    /// replaces the earlier one.
    pub fn disabled(
        mut self,
        field: impl Into<String>,
        rule: impl Fn(&FormContext<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.disabled.insert(field.into(), Arc::new(rule));
        self
    }

    /// Disables `field` on the forms of `operations`, as Filament's `->disabledOn('edit')`:
    /// such as a key that is set when the record is created and locked after its first save.
    pub fn disabled_on(self, field: impl Into<String>, operations: &[Operation]) -> Self {
        let operations = operations.to_vec();
        self.disabled(field, move |form| operations.contains(&form.operation()))
    }

    /// Whether `field` is disabled on `form`.
    pub fn is_disabled(&self, field: &str, form: &FormContext<'_>) -> bool {
        self.disabled.get(field).is_some_and(|rule| rule(form))
    }

    /// Makes `field` live, as Filament's `->live()`: when its value changes in the browser,
    /// the form's fields are shown again for the new value, so a field whose condition reads
    /// it with [`FormContext::get`] appears or disappears. The browser sends the change when
    /// the input fires `change`: at once for a select, a checkbox or a radio, and when a text
    /// input loses focus.
    pub fn live(mut self, field: impl Into<String>) -> Self {
        self.live.insert(field.into());
        self
    }

    /// Fills the field `field` with the slug of the field `source` as the user types, until the
    /// user edits `field` by hand, as Filament's documented slug pattern:
    /// `->afterStateUpdated(...)` on the title sets `slug` to `Str::slug($state)` unless the slug
    /// no longer matches `Str::slug($old)`. Filament writes that with a closure on the server;
    /// the panel's script does it in the browser, so it needs no round trip. The slug is the
    /// text in lowercase ASCII, accents removed, with each run of other characters turned into
    /// one `-`, as `Str::slug` gives it. The value is not checked: `validate` still checks it.
    ///
    /// On the edit form, a slug that is not the slug of the stored title counts as edited by
    /// hand, so it is not replaced. [`slug_locked_when`](Self::slug_locked_when) stops it from
    /// following the title at all, such as once the record is published.
    pub fn slug_from(mut self, field: impl Into<String>, source: impl Into<String>) -> Self {
        self.slugs.insert(field.into(), source.into());
        self
    }

    /// Stops the slug field `field` from following its source on the edit form of a record
    /// whose stored slug is not blank, when `rule` holds, such as when the stored record is
    /// published: the slug of a published post's URL then stays as it is when the title
    /// changes. On the create form, and while `rule` fails (such as for a draft), the slug
    /// follows the title as [`slug_from`](Self::slug_from) says. The user can still edit the
    /// slug by hand. The input is marked `data-ignite-slug-locked`, which the panel's script
    /// reads.
    ///
    /// Filament's slug example has no such rule: it stops when the user has edited the slug.
    /// Ignite keeps that, and adds this one.
    ///
    /// ```ignore
    /// conditions
    ///     .slug_from("slug", "title")
    ///     .slug_locked_when("slug", |form| {
    ///         form.record().and_then(|r| r.get("status")).and_then(|s| s.as_str()) == Some("publish")
    ///     })
    /// ```
    pub fn slug_locked_when(
        mut self,
        field: impl Into<String>,
        rule: impl Fn(&FormContext<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.slug_locks.insert(field.into(), Arc::new(rule));
        self
    }

    /// Whether the slug field `field` no longer follows its source on `form`: the form edits a
    /// record whose stored `field` is not blank, and the rule of
    /// [`slug_locked_when`](Self::slug_locked_when) holds.
    pub fn is_slug_locked(&self, field: &str, form: &FormContext<'_>) -> bool {
        let Some(rule) = self.slug_locks.get(field) else {
            return false;
        };
        let stored = form
            .record()
            .and_then(|record| record.get(field))
            .is_some_and(|value| match value {
                Value::Null => false,
                Value::String(text) => !text.trim().is_empty(),
                _ => true,
            });
        form.operation() == Operation::Edit && stored && rule(form)
    }

    /// The field `field` is filled from by [`slug_from`](Self::slug_from), if any.
    pub fn slug_source(&self, field: &str) -> Option<&str> {
        self.slugs.get(field).map(String::as_str)
    }

    /// Whether `field` is live.
    pub fn is_live(&self, field: &str) -> bool {
        self.live.contains(field)
    }

    /// Whether no field has a condition or is live.
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.visible.is_empty() && self.live.is_empty()
    }

    /// Whether `field` shows in `form`: its `hidden` condition, if any, fails and its
    /// `visible` condition, if any, holds.
    pub fn is_shown(&self, field: &str, form: &FormContext<'_>) -> bool {
        let hidden = self.hidden.get(field).is_some_and(|rule| rule(form));
        let visible = self.visible.get(field).is_none_or(|rule| rule(form));
        !hidden && visible
    }

    /// The names of the `fields` that are hidden on the form for `operation`, editing
    /// `record`, for `user`, whose fields hold `values`.
    ///
    /// A condition that reads a hidden field gets the field's stored value, not the one in
    /// `values`, so a request cannot show a field by sending a value for a field it may not
    /// see. The conditions run again until the hidden fields settle.
    pub fn hidden_fields(
        &self,
        fields: &[FieldDef],
        operation: Operation,
        record: Option<&Row>,
        user: &ActingUser,
        values: &FormInput,
    ) -> BTreeSet<String> {
        let mut hidden = BTreeSet::new();
        if self.hidden.is_empty() && self.visible.is_empty() {
            return hidden;
        }
        for _ in 0..=fields.len() {
            let state = stored_when_hidden(values, record, &hidden);
            let form = FormContext::new(operation, record, user, &state);
            let next: BTreeSet<String> = fields
                .iter()
                .filter(|field| !self.is_shown(&field.name, &form))
                .map(|field| field.name.clone())
                .collect();
            if next == hidden {
                break;
            }
            hidden = next;
        }
        hidden
    }
}

/// `values`, with each field of `hidden` holding its stored value in `record` instead, or no
/// value when there is none.
fn stored_when_hidden(
    values: &FormInput,
    record: Option<&Row>,
    hidden: &BTreeSet<String>,
) -> FormInput {
    let mut state = values.without(hidden.iter().map(String::as_str));
    for name in hidden {
        let stored = match record.and_then(|record| record.get(name)) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => continue,
            Some(Value::Bool(true)) => "on".to_string(),
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
        };
        state.insert(name.clone(), stored);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_resource::FieldKindDef;

    fn field(name: &str) -> FieldDef {
        FieldDef {
            name: name.into(),
            label: name.into(),
            kind: FieldKindDef::Text,
            required: true,
        }
    }

    fn form(pairs: &[(&str, &str)]) -> FormInput {
        FormInput::new(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    fn user() -> ActingUser {
        ActingUser::new(1, "Ada", "ada@example.test")
    }

    #[test]
    fn a_field_with_no_condition_shows() {
        let conditions = FieldConditions::new();
        let user = user();
        let values = form(&[]);
        let context = FormContext::new(Operation::Create, None, &user, &values);
        assert!(conditions.is_shown("title", &context));
        assert!(conditions.is_empty());
    }

    #[test]
    fn hidden_on_and_visible_on_follow_the_operation() {
        let conditions = FieldConditions::new()
            .hidden_on("slug", &[Operation::Create])
            .visible_on("published_at", &[Operation::Edit]);
        let fields = [field("slug"), field("published_at"), field("title")];
        let user = user();
        let values = form(&[]);
        let on_create = conditions.hidden_fields(&fields, Operation::Create, None, &user, &values);
        assert_eq!(
            on_create,
            BTreeSet::from(["published_at".into(), "slug".into()])
        );
        let record = Row::new();
        let on_edit =
            conditions.hidden_fields(&fields, Operation::Edit, Some(&record), &user, &values);
        assert!(on_edit.is_empty());
    }

    #[test]
    fn a_later_hidden_replaces_the_earlier_one_and_both_slots_must_agree() {
        let conditions = FieldConditions::new()
            .hidden("note", |_| true)
            .hidden_on("note", &[Operation::Create])
            .visible("note", |form| form.user().can("notes.edit"));
        let user = user();
        let editor = ActingUser::new(2, "Bo", "bo@example.test").with_ability("notes.edit");
        let values = form(&[]);
        let record = Row::new();
        let edit = |user| FormContext::new(Operation::Edit, Some(&record), user, &values);
        assert!(!conditions.is_shown("note", &edit(&user)), "not visible");
        assert!(conditions.is_shown("note", &edit(&editor)));
        let create = FormContext::new(Operation::Create, None, &editor, &values);
        assert!(!conditions.is_shown("note", &create), "hidden on create");
    }

    #[test]
    fn a_condition_reads_the_record_and_the_current_values() {
        let conditions = FieldConditions::new()
            .live("kind")
            .visible("company", |form| form.get("kind") == Some("business"))
            .visible("archived_note", |form| {
                form.record()
                    .and_then(|record| record.get("status"))
                    .and_then(Value::as_str)
                    == Some("archived")
            });
        assert!(conditions.is_live("kind"));
        assert!(!conditions.is_live("company"));
        let fields = [field("kind"), field("company"), field("archived_note")];
        let user = user();
        let mut record = Row::new();
        record.insert("status".into(), Value::from("archived"));
        let hidden = conditions.hidden_fields(
            &fields,
            Operation::Edit,
            Some(&record),
            &user,
            &form(&[("kind", "business")]),
        );
        assert!(hidden.is_empty(), "{hidden:?}");
        let hidden = conditions.hidden_fields(
            &fields,
            Operation::Create,
            None,
            &user,
            &form(&[("kind", "person")]),
        );
        assert_eq!(
            hidden,
            BTreeSet::from(["archived_note".into(), "company".into()])
        );
    }

    // `secret` is hidden from this user, and `extra` shows when `secret` is "yes". A request
    // that sends `secret=yes` must not show `extra`: the condition reads the stored value.
    #[test]
    fn a_condition_reads_a_hidden_field_as_stored_not_as_sent() {
        let conditions = FieldConditions::new()
            .visible("secret", |form| form.user().can("secret.edit"))
            .visible("extra", |form| form.get("secret") == Some("yes"));
        let fields = [field("secret"), field("extra")];
        let user = user();
        let sent = form(&[("secret", "yes"), ("extra", "changed")]);

        let mut record = Row::new();
        record.insert("secret".into(), Value::from("no"));
        let hidden =
            conditions.hidden_fields(&fields, Operation::Edit, Some(&record), &user, &sent);
        assert_eq!(hidden, BTreeSet::from(["extra".into(), "secret".into()]));

        record.insert("secret".into(), Value::from("yes"));
        let hidden =
            conditions.hidden_fields(&fields, Operation::Edit, Some(&record), &user, &sent);
        assert_eq!(hidden, BTreeSet::from(["secret".into()]));
    }
}
