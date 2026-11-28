//! Type-erased access to a resource, so the routes can serve many resource
//! types from one set of handlers.

use std::collections::BTreeSet;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use ignite_core::{
    ActingUser, ColumnBadges, ColumnDef, Database, FieldConditions, FieldDef, FieldErrors,
    FieldKindDef, FormContext, FormInput, FormLayout, IgniteResource, LatestRecords, ListFilter,
    ListQuery, RegisteredResource, Repeaters, Row, RowAction, Stat, Tab,
};
use orbit_orm::Error;
use serde::Serialize;
use serde_json::Value;

/// The operations the routes need from a resource, without its type.
#[rocket::async_trait]
pub(crate) trait Erased: Send + Sync {
    fn slug(&self) -> &str;
    fn title(&self) -> &str;
    fn singular(&self) -> &str;
    fn per_page(&self) -> i64;
    /// The label of the navigation entry: the navigation label, or the title.
    fn navigation_label(&self) -> &str;
    /// The navigation group the resource is listed under, if any.
    fn navigation_group(&self) -> Option<&str>;
    /// The resource's place in the navigation, if set: lower first.
    fn navigation_sort(&self) -> Option<i32>;
    /// The navigation entry the resource is listed under (its label or slug), if any.
    fn navigation_parent_item(&self) -> Option<&str>;
    /// The CSS classes of the navigation entry's icon, if set.
    fn navigation_icon(&self) -> Option<&str>;
    /// The label of the navigation entry's create link, or `None` when it lists none.
    fn navigation_create_item(&self) -> Option<&str>;
    /// The form fields, owned, so static and runtime resources render the same way.
    fn fields(&self) -> &[FieldDef];
    /// The list columns, owned.
    fn columns(&self) -> &[ColumnDef];
    /// The conditions that show or hide the form's fields.
    fn conditions(&self) -> &FieldConditions;
    /// The layout of the create and edit forms.
    fn form_layout(&self) -> &FormLayout;
    /// The settings of the form's repeaters and flexible content fields.
    fn repeaters(&self) -> &Repeaters;
    /// Every ability `action` on the resource needs: `<slug>.<action>`, or for a runtime
    /// resource the ability it names and its access ability.
    fn required_abilities(&self, action: crate::authz::Action) -> Vec<String> {
        vec![crate::authz::ability(self.slug(), action)]
    }
    /// Every ability the resource's actions need, without repeats: its permissions.
    fn ability_names(&self) -> Vec<String> {
        crate::authz::slug_abilities(self.slug()).to_vec()
    }
    /// Whether the record of the edit form `form` is read-only for its user, as a runtime
    /// resource's `read_only_when` says. A static resource has no read-only records.
    fn is_read_only(&self, form: &FormContext<'_>) -> bool {
        let _ = form;
        false
    }
    /// The line the form of a read-only record shows, if any.
    fn read_only_notice(&self, form: &FormContext<'_>) -> Option<String> {
        let _ = form;
        None
    }
    /// The tabs of the list page, in display order.
    fn tabs(&self) -> &[Tab];
    /// The tabs of the list page for `user` on this request: a runtime resource's
    /// [`RuntimeData::tabs_for`](ignite_core::RuntimeData::tabs_for), else [`tabs`](Self::tabs).
    async fn tabs_for(&self, db: &Database, user: &ActingUser) -> Result<Vec<Tab>, Error> {
        let _ = (db, user);
        Ok(self.tabs().to_vec())
    }
    /// The key of the tab shown when the list has no `tab` parameter, if set.
    fn default_active_tab(&self) -> Option<&str>;
    /// The list columns shown as coloured badges.
    fn column_badges(&self) -> &ColumnBadges;
    /// Whether each tab's count is for the current search.
    fn tab_counts_follow_search(&self) -> bool;
    /// Whether a tab whose count is 0 is left out.
    fn hide_empty_tabs(&self) -> bool;
    /// The actions on each row of the list, in display order.
    fn row_actions(&self) -> &[RowAction];
    /// The records whose built-in Delete button is hidden, if set.
    fn hide_default_delete_when(&self) -> Option<&ListFilter>;
    /// One page of records as JSON, and the total count.
    /// The resource's queries are given the signed-in `user`: they call its `*_for` methods.
    async fn list(
        &self,
        db: &Database,
        user: &ActingUser,
        query: &ListQuery,
    ) -> Result<(Vec<Value>, i64), Error>;
    /// The record addressed by `key` in the panel's URLs: the numeric `id`, or the key of a
    /// runtime resource's [`record_route_key_name`](ignite_core::RuntimeResource::record_route_key_name).
    async fn find(
        &self,
        db: &Database,
        user: &ActingUser,
        key: &str,
    ) -> Result<Option<Value>, Error>;
    /// The row key that addresses a record in the panel's URLs: `id`, or the runtime
    /// resource's `record_route_key_name`.
    fn record_route_key_name(&self) -> &str {
        "id"
    }
    /// `Err(errors)` when the form does not validate; the database is not touched then. The
    /// fields of `hidden` are left out: they are not checked and not written.
    async fn create(
        &self,
        db: &Database,
        user: &ActingUser,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error>;
    async fn update(
        &self,
        db: &Database,
        user: &ActingUser,
        key: &str,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error>;
    async fn delete(&self, db: &Database, user: &ActingUser, key: &str) -> Result<u64, Error>;
    /// The dashboard cards this resource provides for `user`.
    async fn stats(&self, db: &Database, user: &ActingUser) -> Result<Vec<Stat>, Error>;
    /// The dashboard table this resource declares, if any.
    fn latest_records(&self) -> Option<LatestRecords>;
    /// The newest records as JSON, up to `limit`.
    async fn latest(&self, db: &Database, limit: i64) -> Result<Vec<Value>, Error>;
    /// The choices of a lookup field for `user`, as `(value, label)` pairs.
    async fn options(
        &self,
        db: &Database,
        user: &ActingUser,
        field: &str,
    ) -> Result<Vec<(String, String)>, Error>;
    /// The choices of the field at the path `field` for the values `state` around it: a
    /// runtime resource's [`RuntimeData::options_with`](ignite_core::RuntimeData::options_with),
    /// else [`options`](Self::options).
    async fn options_with(
        &self,
        db: &Database,
        user: &ActingUser,
        field: &str,
        state: &Row,
    ) -> Result<Vec<(String, String)>, Error> {
        let _ = state;
        self.options(db, user, field).await
    }
}

/// Wraps an [`IgniteResource`] type as an [`Erased`] value. Its fields and columns are
/// converted to owned definitions once, when the panel is built.
pub(crate) struct Adapter<R> {
    fields: Vec<FieldDef>,
    columns: Vec<ColumnDef>,
    conditions: FieldConditions,
    form_layout: FormLayout,
    repeaters: Repeaters,
    tabs: Vec<Tab>,
    column_badges: ColumnBadges,
    row_actions: Vec<RowAction>,
    hide_default_delete_when: Option<ListFilter>,
    resource: PhantomData<fn() -> R>,
}

impl<R: IgniteResource> Adapter<R> {
    pub(crate) fn new() -> Self {
        Adapter {
            fields: R::fields().iter().copied().map(FieldDef::from).collect(),
            columns: R::columns().iter().copied().map(ColumnDef::from).collect(),
            conditions: R::conditions(),
            form_layout: R::form_layout(),
            repeaters: R::repeaters(),
            tabs: R::tabs(),
            column_badges: R::column_badges(),
            row_actions: R::row_actions(),
            hide_default_delete_when: R::hide_default_delete_when(),
            resource: PhantomData,
        }
    }
}

/// The fields of `fields` that `hidden` does not hide.
fn shown(fields: &[FieldDef], hidden: &BTreeSet<String>) -> Vec<FieldDef> {
    fields
        .iter()
        .filter(|f| !hidden.contains(&f.name))
        .cloned()
        .collect()
}

/// A static resource's validation of a form without its `hidden` fields, without the errors
/// of those fields, which the form does not show. A `validate` that refuses nothing but a
/// hidden field is a server error: the resource must accept a field that may be hidden as
/// missing (see [`IgniteResource::conditions`]).
fn shown_errors<T>(
    validated: Result<T, FieldErrors>,
    hidden: &BTreeSet<String>,
    slug: &str,
) -> Result<Result<T, FieldErrors>, Error> {
    let Err(mut errors) = validated else {
        return Ok(validated);
    };
    errors.retain(|name, _| !hidden.contains(name));
    if errors.is_empty() {
        return Err(Error::Database(orbit_orm::__sqlx::Error::Protocol(
            format!(
                "the resource `{slug}` refused a field its conditions hide; \
             make the field optional in `validate`"
            ),
        )));
    }
    Ok(Err(errors))
}

/// The error key and label of each static field, for [`unique_violation`].
fn static_keys<R: IgniteResource>() -> Vec<(&'static str, &'static str)> {
    R::fields().iter().map(|f| (f.name, f.label)).collect()
}

#[rocket::async_trait]
impl<R: IgniteResource> Erased for Adapter<R> {
    fn slug(&self) -> &str {
        R::SLUG
    }

    fn title(&self) -> &str {
        R::TITLE
    }

    fn singular(&self) -> &str {
        R::SINGULAR
    }

    fn per_page(&self) -> i64 {
        R::PER_PAGE
    }

    fn navigation_label(&self) -> &str {
        R::NAVIGATION_LABEL.unwrap_or(R::TITLE)
    }

    fn navigation_group(&self) -> Option<&str> {
        R::NAVIGATION_GROUP
    }

    fn navigation_sort(&self) -> Option<i32> {
        R::NAVIGATION_SORT
    }

    fn navigation_parent_item(&self) -> Option<&str> {
        R::NAVIGATION_PARENT_ITEM
    }

    fn navigation_icon(&self) -> Option<&str> {
        R::NAVIGATION_ICON
    }

    fn navigation_create_item(&self) -> Option<&str> {
        R::NAVIGATION_CREATE_ITEM.then_some(R::NAVIGATION_CREATE_LABEL)
    }

    fn fields(&self) -> &[FieldDef] {
        &self.fields
    }

    fn columns(&self) -> &[ColumnDef] {
        &self.columns
    }

    fn conditions(&self) -> &FieldConditions {
        &self.conditions
    }

    fn form_layout(&self) -> &FormLayout {
        &self.form_layout
    }

    fn repeaters(&self) -> &Repeaters {
        &self.repeaters
    }

    fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    fn default_active_tab(&self) -> Option<&str> {
        R::default_active_tab()
    }

    fn column_badges(&self) -> &ColumnBadges {
        &self.column_badges
    }

    fn tab_counts_follow_search(&self) -> bool {
        R::tab_counts_follow_search()
    }

    fn hide_empty_tabs(&self) -> bool {
        R::hide_empty_tabs()
    }

    fn row_actions(&self) -> &[RowAction] {
        &self.row_actions
    }

    fn hide_default_delete_when(&self) -> Option<&ListFilter> {
        self.hide_default_delete_when.as_ref()
    }

    async fn list(
        &self,
        db: &Database,
        user: &ActingUser,
        query: &ListQuery,
    ) -> Result<(Vec<Value>, i64), Error> {
        let page = R::list_for(db, user, query).await?;
        let rows = page
            .rows
            .into_iter()
            .map(to_json)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((rows, page.total))
    }

    async fn find(
        &self,
        db: &Database,
        user: &ActingUser,
        key: &str,
    ) -> Result<Option<Value>, Error> {
        // A static resource's records are addressed by their id: other text is no record.
        let Ok(id) = key.parse::<i64>() else {
            return Ok(None);
        };
        R::find_for(db, user, id).await?.map(to_json).transpose()
    }

    async fn create(
        &self,
        db: &Database,
        user: &ActingUser,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        let form = form.without(hidden.iter().map(String::as_str));
        let validated = shown_errors(R::validate(&form), hidden, R::SLUG)?;
        match checked::<R>(db, user, &shown(&self.fields, hidden), &form, validated).await? {
            Ok(input) => match R::create_for(db, user, input).await {
                Ok(record) => to_json(record).map(Ok),
                Err(error) => unique_violation(error, R::SLUG, &static_keys::<R>()).map(Err),
            },
            Err(errors) => Ok(Err(errors)),
        }
    }

    async fn update(
        &self,
        db: &Database,
        user: &ActingUser,
        key: &str,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        let id = key
            .parse::<i64>()
            .map_err(|_| Error::NotFound { table: R::SLUG })?;
        let form = form.without(hidden.iter().map(String::as_str));
        let validated = shown_errors(R::validate_update(&form), hidden, R::SLUG)?;
        match checked::<R>(db, user, &shown(&self.fields, hidden), &form, validated).await? {
            Ok(input) => match R::update_for(db, user, id, input).await {
                Ok(record) => to_json(record).map(Ok),
                Err(error) => unique_violation(error, R::SLUG, &static_keys::<R>()).map(Err),
            },
            Err(errors) => Ok(Err(errors)),
        }
    }

    async fn delete(&self, db: &Database, user: &ActingUser, key: &str) -> Result<u64, Error> {
        match key.parse::<i64>() {
            Ok(id) => R::delete_for(db, user, id).await,
            Err(_) => Ok(0),
        }
    }

    async fn stats(&self, db: &Database, user: &ActingUser) -> Result<Vec<Stat>, Error> {
        R::stats(db, user).await
    }

    fn latest_records(&self) -> Option<LatestRecords> {
        R::latest_records()
    }

    async fn latest(&self, db: &Database, limit: i64) -> Result<Vec<Value>, Error> {
        R::latest(db, limit)
            .await?
            .into_iter()
            .map(to_json)
            .collect()
    }

    async fn options(
        &self,
        db: &Database,
        user: &ActingUser,
        field: &str,
    ) -> Result<Vec<(String, String)>, Error> {
        R::options_for(db, user, field).await
    }
}

/// Adds to a validated form the errors of its lookup and media fields: a submitted value
/// must be one of the field's choices, so a row that does not exist is refused, and a media
/// id must be an item of the registered media library. A field that already has an error
/// keeps it. The database is read only for the fields the form submits.
async fn checked<R: IgniteResource>(
    db: &Database,
    user: &ActingUser,
    fields: &[FieldDef],
    form: &FormInput,
    validated: Result<R::Input, FieldErrors>,
) -> Result<Result<R::Input, FieldErrors>, Error> {
    let errors = validated.as_ref().err().cloned().unwrap_or_default();
    let errors = lookup_errors(fields, form, errors, |field, _| async move {
        R::options_for(db, user, &field).await
    })
    .await?;
    let errors = media_errors(db, fields, form, errors).await?;
    Ok(if errors.is_empty() {
        validated
    } else {
        Err(errors)
    })
}

/// `errors`, with "Choose one of the options." for each field of `fields` that loads its
/// options and whose submitted value is not one of the choices `options` returns for it,
/// given the field's path and the values around it (see [`form_state`] and
/// [`nested_lookups`]). For a relationship or a checkbox list, each value of its JSON list
/// must be a choice. A sub-field's error is keyed by its dotted path, and its structured
/// field gets the message too, so the save is refused. Static and runtime resources share
/// this check.
async fn lookup_errors<F, Fut>(
    fields: &[FieldDef],
    form: &FormInput,
    mut errors: FieldErrors,
    options: F,
) -> Result<FieldErrors, Error>
where
    F: Fn(String, Row) -> Fut,
    Fut: Future<Output = Result<Vec<(String, String)>, Error>>,
{
    let state = form_state(fields, form);
    let mut checks: Vec<(String, Option<String>, String, Row, Value)> = fields
        .iter()
        .filter(|f| f.kind.loads_options())
        .map(|f| {
            let value = state.get(&f.name).cloned().unwrap_or(Value::Null);
            (f.name.clone(), None, f.name.clone(), state.clone(), value)
        })
        .collect();
    checks.extend(nested_lookups(fields, &state).into_iter().map(|lookup| {
        (
            lookup.path,
            Some(lookup.top),
            lookup.schema,
            lookup.container,
            lookup.value,
        )
    }));
    for (key, top, schema, container, value) in checks {
        let values = chosen_values(&value);
        if values.is_empty() {
            continue;
        }
        let choices = options(schema, container).await?;
        if !values
            .iter()
            .all(|value| choices.iter().any(|(choice, _)| choice == value))
        {
            let message = "Choose one of the options.".to_string();
            if let Some(top) = top {
                errors.entry(top).or_insert_with(|| message.clone());
            }
            errors.entry(key).or_insert(message);
        }
    }
    Ok(errors)
}

/// The chosen values of a lookup's value: each item of a list, or the text of one value.
fn chosen_values(value: &Value) -> Vec<String> {
    let text = |item: &Value| match item {
        Value::String(text) => text.trim().to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    match value {
        Value::Array(items) => items.iter().map(text).filter(|t| !t.is_empty()).collect(),
        other => Some(text(other))
            .filter(|t| !t.is_empty())
            .into_iter()
            .collect(),
    }
}

/// The form's values as a dependent field's `state` sees them: each field of `fields` the
/// form sent, as text, a checkbox as `true` or `false`, and a structured or list value as
/// its JSON.
pub(crate) fn form_state(fields: &[FieldDef], form: &FormInput) -> Row {
    let mut state = Row::new();
    for field in fields {
        let value = match &field.kind {
            FieldKindDef::Checkbox => Value::Bool(form.checkbox(&field.name)),
            kind => match form.get(&field.name) {
                None => continue,
                Some(text)
                    if ignite_core::structure::is_structured(kind)
                        || matches!(
                            kind,
                            FieldKindDef::Relationship
                                | FieldKindDef::CheckboxList(_)
                                | FieldKindDef::Gallery
                                | FieldKindDef::Link
                        ) =>
                {
                    serde_json::from_str(text).unwrap_or_else(|_| Value::from(text))
                }
                Some(text) => Value::from(text),
            },
        };
        state.insert(field.name.clone(), value);
    }
    state
}

/// A sub-field that loads its options, inside a repeater, group or flexible content: its
/// dotted path (`rules.0.value`), the field of the form it is in (`rules`), its settings path
/// (`rules.value`), the values of its row or group, and its own value.
pub(crate) struct NestedLookup {
    pub(crate) path: String,
    pub(crate) top: String,
    pub(crate) schema: String,
    pub(crate) container: Row,
    pub(crate) value: Value,
}

/// Every sub-field of the structured fields of `fields` that loads its options, in the values
/// of `state` (see [`form_state`]), at any depth, whether it has a value or not.
pub(crate) fn nested_lookups(fields: &[FieldDef], state: &Row) -> Vec<NestedLookup> {
    let mut found = Vec::new();
    for field in fields {
        if let Some(value) = state.get(&field.name) {
            walk(
                &field.kind,
                value,
                &field.name,
                &field.name,
                &field.name,
                &mut found,
            );
        }
    }
    found
}

/// Adds the lookups inside `value`, a value of `kind` at the dotted `path` with the settings
/// path `schema`, in the form's field `top`.
fn walk(
    kind: &FieldKindDef,
    value: &Value,
    path: &str,
    schema: &str,
    top: &str,
    found: &mut Vec<NestedLookup>,
) {
    let empty = Row::new();
    match kind {
        FieldKindDef::Repeater(fields) => {
            for (index, row) in value.as_array().into_iter().flatten().enumerate() {
                let row = row.as_object().unwrap_or(&empty);
                container(fields, row, &format!("{path}.{index}"), schema, top, found);
            }
        }
        FieldKindDef::Group(fields) | FieldKindDef::Clone(fields) => {
            container(
                fields,
                value.as_object().unwrap_or(&empty),
                path,
                schema,
                top,
                found,
            );
        }
        FieldKindDef::FlexibleContent(blocks) => {
            for (index, row) in value.as_array().into_iter().flatten().enumerate() {
                let kind = row.get("type").and_then(Value::as_str).unwrap_or("");
                let Some(block) = blocks.iter().find(|b| b.name == kind) else {
                    continue;
                };
                let data = row.get("data").and_then(Value::as_object).unwrap_or(&empty);
                container(
                    &block.fields,
                    data,
                    &format!("{path}.{index}.data"),
                    &format!("{schema}.{kind}"),
                    top,
                    found,
                );
            }
        }
        _ => {}
    }
}

/// Adds the lookups among the sub-fields `fields` of one row or group holding `values`.
fn container(
    fields: &[FieldDef],
    values: &Row,
    path: &str,
    schema: &str,
    top: &str,
    found: &mut Vec<NestedLookup>,
) {
    for field in fields {
        let field_path = format!("{path}.{}", field.name);
        let field_schema = format!("{schema}.{}", field.name);
        let value = values.get(&field.name).cloned().unwrap_or(Value::Null);
        if field.kind.loads_options() {
            found.push(NestedLookup {
                path: field_path,
                top: top.to_string(),
                schema: field_schema,
                container: values.clone(),
                value,
            });
        } else {
            walk(&field.kind, &value, &field_path, &field_schema, top, found);
        }
    }
}

/// The media ids a field of `fields` holds in `form`: the id of an image or file field, and
/// each id of a gallery's JSON list. Text that is not an id is skipped.
pub(crate) fn field_media_ids(field: &FieldDef, form: &FormInput) -> Vec<i64> {
    let Some(value) = form.get(&field.name) else {
        return Vec::new();
    };
    match field.kind {
        FieldKindDef::Image | FieldKindDef::File => {
            value.trim().parse::<i64>().ok().into_iter().collect()
        }
        FieldKindDef::Gallery => serde_json::from_str::<Vec<Value>>(value)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| match item {
                Value::Number(number) => number.as_i64(),
                Value::String(text) => text.trim().parse().ok(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Every media id the media fields of `fields` hold in `form`, without repeats.
pub(crate) fn media_ids(fields: &[FieldDef], form: &FormInput) -> Vec<i64> {
    let mut ids: Vec<i64> = fields
        .iter()
        .flat_map(|field| field_media_ids(field, form))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// `errors`, with "<label> must be the id of a media item." for each media field whose ids
/// are not all items of the registered media library. With no library registered, the ids
/// are only checked as numbers, by validation.
async fn media_errors(
    db: &Database,
    fields: &[FieldDef],
    form: &FormInput,
    mut errors: FieldErrors,
) -> Result<FieldErrors, Error> {
    let Some(library) = ignite_core::media_library() else {
        return Ok(errors);
    };
    let ids = media_ids(fields, form);
    if ids.is_empty() {
        return Ok(errors);
    }
    let found: Vec<i64> = library
        .find(db, &ids)
        .await?
        .into_iter()
        .map(|item| item.id)
        .collect();
    for field in fields {
        if field_media_ids(field, form)
            .iter()
            .any(|id| !found.contains(id))
        {
            errors
                .entry(field.name.clone())
                .or_insert_with(|| format!("{} must be the id of a media item.", field.label));
        }
    }
    Ok(errors)
}

/// Wraps a runtime resource from the registry as an [`Erased`] value, so the routes serve
/// it as they serve a static one.
pub(crate) struct RuntimeAdapter(pub(crate) Arc<RegisteredResource>);

impl RuntimeAdapter {
    /// Validates `form` with the resource's definitions, then checks its lookup fields. The
    /// fields of `hidden` are left out.
    async fn checked(
        &self,
        db: &Database,
        user: &ActingUser,
        form: &FormInput,
        creating: bool,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Row, FieldErrors>, Error> {
        let validated = if creating {
            self.0.validate_shown(form, hidden)
        } else {
            self.0.validate_update_shown(form, hidden)
        };
        let operation = if creating {
            ignite_core::Operation::Create
        } else {
            ignite_core::Operation::Edit
        };
        let mut errors = validated.as_ref().err().cloned().unwrap_or_default();
        // The data layer's own rules on the submitted form, for the shown fields only.
        for (name, message) in self.0.resource().data.validate_form(form, operation) {
            if !hidden.contains(&name) {
                errors.entry(name).or_insert(message);
            }
        }
        let validated = match validated {
            Ok(_) if !errors.is_empty() => Err(errors.clone()),
            other => other,
        };
        let fields = &shown(self.0.fields(), hidden);
        let data = &self.0.resource().data;
        let errors = lookup_errors(fields, form, errors, |field, state| async move {
            data.options_with(db, user, &field, &state).await
        })
        .await?;
        let errors = media_errors(db, fields, form, errors).await?;
        Ok(if errors.is_empty() {
            validated
        } else {
            Err(errors)
        })
    }

    /// Saves a validated row through `save`. The data layer's refusal with field messages
    /// ([`ValidationError`](ignite_core::ValidationError)) becomes the form's errors, and so
    /// does a unique-index violation, as for a static resource.
    async fn save<'a>(
        &self,
        validated: Result<Row, FieldErrors>,
        save: impl FnOnce(
            Row,
        )
            -> ignite_core::RuntimeFuture<'a, Result<Row, ignite_core::ValidationError>>,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        match validated {
            Ok(row) => match save(row).await {
                Ok(Ok(record)) => Ok(Ok(Value::Object(record))),
                Ok(Err(refused)) => Ok(Err(refused.into_errors())),
                Err(error) => {
                    let keys: Vec<(&str, &str)> = self
                        .0
                        .fields()
                        .iter()
                        .map(|f| (f.name.as_str(), f.label.as_str()))
                        .collect();
                    unique_violation(error, self.0.slug(), &keys).map(Err)
                }
            },
            Err(errors) => Ok(Err(errors)),
        }
    }
}

#[rocket::async_trait]
impl Erased for RuntimeAdapter {
    fn slug(&self) -> &str {
        self.0.slug()
    }

    fn title(&self) -> &str {
        self.0.title()
    }

    fn singular(&self) -> &str {
        self.0.singular()
    }

    fn per_page(&self) -> i64 {
        self.0.resource().per_page
    }

    fn navigation_label(&self) -> &str {
        self.0.navigation_label()
    }

    fn navigation_group(&self) -> Option<&str> {
        self.0.navigation_group()
    }

    fn navigation_sort(&self) -> Option<i32> {
        self.0.navigation_sort()
    }

    fn navigation_parent_item(&self) -> Option<&str> {
        self.0.navigation_parent_item()
    }

    fn navigation_icon(&self) -> Option<&str> {
        self.0.navigation_icon()
    }

    fn navigation_create_item(&self) -> Option<&str> {
        self.0.navigation_create_item()
    }

    fn fields(&self) -> &[FieldDef] {
        self.0.fields()
    }

    fn columns(&self) -> &[ColumnDef] {
        self.0.columns()
    }

    fn conditions(&self) -> &FieldConditions {
        self.0.conditions()
    }

    fn form_layout(&self) -> &FormLayout {
        self.0.form_layout()
    }

    fn repeaters(&self) -> &Repeaters {
        self.0.repeaters()
    }

    fn is_read_only(&self, form: &FormContext<'_>) -> bool {
        self.0.is_read_only(form)
    }

    fn required_abilities(&self, action: crate::authz::Action) -> Vec<String> {
        self.0.required_abilities(action.resource_action())
    }

    fn ability_names(&self) -> Vec<String> {
        self.0.ability_names()
    }

    fn read_only_notice(&self, form: &FormContext<'_>) -> Option<String> {
        self.0.read_only_notice(form)
    }

    fn tabs(&self) -> &[Tab] {
        self.0.tabs()
    }

    fn default_active_tab(&self) -> Option<&str> {
        self.0.default_active_tab()
    }

    fn column_badges(&self) -> &ColumnBadges {
        self.0.column_badges()
    }

    fn tab_counts_follow_search(&self) -> bool {
        self.0.tab_counts_follow_search()
    }

    fn hide_empty_tabs(&self) -> bool {
        self.0.hide_empty_tabs()
    }

    fn row_actions(&self) -> &[RowAction] {
        self.0.row_actions()
    }

    fn hide_default_delete_when(&self) -> Option<&ListFilter> {
        self.0.hide_default_delete_when()
    }

    async fn tabs_for(&self, db: &Database, user: &ActingUser) -> Result<Vec<Tab>, Error> {
        Ok(match self.0.resource().data.tabs_for(db, user).await? {
            Some(tabs) => tabs,
            None => self.0.tabs().to_vec(),
        })
    }

    async fn list(
        &self,
        db: &Database,
        user: &ActingUser,
        query: &ListQuery,
    ) -> Result<(Vec<Value>, i64), Error> {
        let page = self.0.resource().data.list_for(db, user, query).await?;
        Ok((
            page.rows.into_iter().map(Value::Object).collect(),
            page.total,
        ))
    }

    async fn find(
        &self,
        db: &Database,
        user: &ActingUser,
        key: &str,
    ) -> Result<Option<Value>, Error> {
        Ok(self
            .0
            .resource()
            .data
            .find_by_key_for(db, user, key)
            .await?
            .map(Value::Object))
    }

    fn record_route_key_name(&self) -> &str {
        self.0.record_route_key_name()
    }

    async fn create(
        &self,
        db: &Database,
        user: &ActingUser,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        let validated = self.checked(db, user, form, true, hidden).await?;
        let data = &self.0.resource().data;
        self.save(validated, |row| data.try_create_for(db, user, row))
            .await
    }

    async fn update(
        &self,
        db: &Database,
        user: &ActingUser,
        key: &str,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        let validated = self.checked(db, user, form, false, hidden).await?;
        let data = &self.0.resource().data;
        self.save(validated, |row| {
            data.try_update_by_key_for(db, user, key, row)
        })
        .await
    }

    async fn delete(&self, db: &Database, user: &ActingUser, key: &str) -> Result<u64, Error> {
        self.0
            .resource()
            .data
            .delete_by_key_for(db, user, key)
            .await
    }

    /// The cards its data source gives (none by default).
    async fn stats(&self, db: &Database, user: &ActingUser) -> Result<Vec<Stat>, Error> {
        self.0.resource().data.stats(db, user).await
    }

    /// A runtime resource has no dashboard table.
    fn latest_records(&self) -> Option<LatestRecords> {
        None
    }

    async fn latest(&self, _db: &Database, _limit: i64) -> Result<Vec<Value>, Error> {
        Ok(Vec::new())
    }

    async fn options(
        &self,
        db: &Database,
        user: &ActingUser,
        field: &str,
    ) -> Result<Vec<(String, String)>, Error> {
        self.0.resource().data.options_for(db, user, field).await
    }

    async fn options_with(
        &self,
        db: &Database,
        user: &ActingUser,
        field: &str,
        state: &Row,
    ) -> Result<Vec<(String, String)>, Error> {
        self.0
            .resource()
            .data
            .options_with(db, user, field, state)
            .await
    }
}

/// The name of the unique index on `field` of `table`. The panel's migrations name
/// it `{table}_{field}_unique`, as `users_email_unique` guards `email` on `users`.
/// For a derived resource the table is its [`SLUG`](ignite_core::IgniteResource::SLUG).
pub(crate) fn unique_index_name(table: &str, field: &str) -> String {
    format!("{table}_{field}_unique")
}

/// The field whose unique index is `index`, under the one rule of
/// [`unique_index_name`]: the names must match exactly. A name that only contains
/// a field's name, such as `users_username_unique` for `name`, matches nothing.
fn field_for_index<'a, 'b>(
    index: &str,
    table: &str,
    fields: &'a [(&'b str, &'b str)],
) -> Option<&'a (&'b str, &'b str)> {
    fields
        .iter()
        .find(|(name, _)| unique_index_name(table, name) == index)
}

/// Turns a unique-index violation into a field error, so the form shows "Email is
/// already taken." instead of a server error.
///
/// The rule, the only one: the violated index must be named exactly
/// `{table}_{field}_unique` (see [`unique_index_name`]), where `table` is the
/// resource's SLUG and `field` is a form field's name. Matching is exact, so `name`
/// does not claim `users_username_unique`. Any other error, or a violation whose
/// index names no field, is returned unchanged. `fields` holds each field's error key and
/// label. A runtime resource follows the same rule, with its slug as the table.
fn unique_violation(
    error: Error,
    table: &str,
    fields: &[(&str, &str)],
) -> Result<FieldErrors, Error> {
    let field = match &error {
        Error::UniqueViolation { index: Some(index) } => field_for_index(index, table, fields),
        _ => None,
    };
    match field {
        Some((name, label)) => {
            let mut errors = FieldErrors::new();
            errors.insert(name.to_string(), format!("{label} is already taken."));
            Ok(errors)
        }
        None => Err(error),
    }
}

/// Serializes a row to JSON. A row that cannot serialize is a database error here, as the
/// panel has always reported it.
fn to_json<T: Serialize>(record: T) -> Result<Value, Error> {
    serde_json::to_value(record).map_err(|e| {
        Error::Database(orbit_orm::__sqlx::Error::Protocol(format!(
            "could not serialize a record: {e}"
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME: (&str, &str) = ("name", "Name");
    const EMAIL: (&str, &str) = ("email", "Email");

    #[test]
    fn name_does_not_match_an_index_on_username() {
        let fields = [NAME, EMAIL];
        assert!(field_for_index("users_username_unique", "users", &fields).is_none());
    }

    #[test]
    fn email_matches_the_email_index() {
        let fields = [NAME, EMAIL];
        let field = field_for_index("users_email_unique", "users", &fields).expect("a field");
        assert_eq!(field.0, "email");
    }

    #[test]
    fn a_violation_maps_to_the_field_its_index_names() {
        let fields = [NAME, EMAIL];
        let error = Error::UniqueViolation {
            index: Some("users_email_unique".to_string()),
        };
        let errors = unique_violation(error, "users", &fields).expect("a field error");
        assert_eq!(
            errors.get("email").map(String::as_str),
            Some("Email is already taken.")
        );
    }

    #[test]
    fn an_index_on_another_table_matches_nothing() {
        let fields = [EMAIL];
        assert!(field_for_index("accounts_email_unique", "users", &fields).is_none());
    }
}
