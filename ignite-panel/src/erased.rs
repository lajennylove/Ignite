//! Type-erased access to a resource, so the routes can serve many resource
//! types from one set of handlers.

use std::collections::BTreeSet;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use ignite_core::{
    ActingUser, ColumnDef, Database, FieldConditions, FieldDef, FieldErrors, FieldKindDef,
    FormInput, FormLayout, IgniteResource, LatestRecords, ListQuery, RegisteredResource, Row,
    StatWidget,
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
    /// The CSS classes of the navigation entry's icon, if set.
    fn navigation_icon(&self) -> Option<&str>;
    /// The form fields, owned, so static and runtime resources render the same way.
    fn fields(&self) -> &[FieldDef];
    /// The list columns, owned.
    fn columns(&self) -> &[ColumnDef];
    /// The conditions that show or hide the form's fields.
    fn conditions(&self) -> &FieldConditions;
    /// The layout of the create and edit forms.
    fn form_layout(&self) -> &FormLayout;
    /// One page of records as JSON, and the total count.
    /// The resource's queries are given the signed-in `user`: they call its `*_for` methods.
    async fn list(
        &self,
        db: &Database,
        user: &ActingUser,
        query: &ListQuery,
    ) -> Result<(Vec<Value>, i64), Error>;
    async fn find(&self, db: &Database, user: &ActingUser, id: i64)
        -> Result<Option<Value>, Error>;
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
        id: i64,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error>;
    async fn delete(&self, db: &Database, user: &ActingUser, id: i64) -> Result<u64, Error>;
    /// The dashboard cards this resource provides.
    async fn widgets(&self, db: &Database) -> Result<Vec<StatWidget>, Error>;
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
}

/// Wraps an [`IgniteResource`] type as an [`Erased`] value. Its fields and columns are
/// converted to owned definitions once, when the panel is built.
pub(crate) struct Adapter<R> {
    fields: Vec<FieldDef>,
    columns: Vec<ColumnDef>,
    conditions: FieldConditions,
    form_layout: FormLayout,
    resource: PhantomData<fn() -> R>,
}

impl<R: IgniteResource> Adapter<R> {
    pub(crate) fn new() -> Self {
        Adapter {
            fields: R::fields().iter().copied().map(FieldDef::from).collect(),
            columns: R::columns().iter().copied().map(ColumnDef::from).collect(),
            conditions: R::conditions(),
            form_layout: R::form_layout(),
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

    fn navigation_icon(&self) -> Option<&str> {
        R::NAVIGATION_ICON
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
        id: i64,
    ) -> Result<Option<Value>, Error> {
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
        id: i64,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error> {
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

    async fn delete(&self, db: &Database, user: &ActingUser, id: i64) -> Result<u64, Error> {
        R::delete_for(db, user, id).await
    }

    async fn widgets(&self, db: &Database) -> Result<Vec<StatWidget>, Error> {
        R::widgets(db).await
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
    let errors = lookup_errors(fields, form, errors, |field| {
        R::options_for(db, user, field)
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
/// options and whose submitted value is not one of the choices `options` returns for it. For
/// a relationship, each value of its JSON list must be a choice. Static and runtime
/// resources share this check.
async fn lookup_errors<'a, F, Fut>(
    fields: &'a [FieldDef],
    form: &FormInput,
    mut errors: FieldErrors,
    options: F,
) -> Result<FieldErrors, Error>
where
    F: Fn(&'a str) -> Fut,
    Fut: Future<Output = Result<Vec<(String, String)>, Error>>,
{
    for field in fields.iter().filter(|f| f.kind.loads_options()) {
        let name = field.name.as_str();
        let Some(value) = form.get(name) else {
            continue;
        };
        let values = if field.kind == FieldKindDef::Relationship {
            match serde_json::from_str::<Vec<Value>>(value) {
                Ok(items) => items
                    .iter()
                    .map(|item| match item {
                        Value::String(text) => text.trim().to_string(),
                        other => other.to_string(),
                    })
                    .collect(),
                // Validation has already refused a value that is not a list.
                Err(_) => continue,
            }
        } else {
            vec![value.trim().to_string()]
        };
        if values.is_empty() {
            continue;
        }
        let choices = options(name).await?;
        if !values
            .iter()
            .all(|value| choices.iter().any(|(choice, _)| choice == value))
        {
            errors
                .entry(name.to_string())
                .or_insert_with(|| "Choose one of the options.".to_string());
        }
    }
    Ok(errors)
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
        let errors = validated.as_ref().err().cloned().unwrap_or_default();
        let fields = &shown(self.0.fields(), hidden);
        let data = &self.0.resource().data;
        let errors = lookup_errors(fields, form, errors, |field| {
            data.options_for(db, user, field)
        })
        .await?;
        let errors = media_errors(db, fields, form, errors).await?;
        Ok(if errors.is_empty() {
            validated
        } else {
            Err(errors)
        })
    }

    /// Saves a validated row through `save`, and turns a unique-index violation into a
    /// field error, as for a static resource.
    async fn save<'a>(
        &self,
        validated: Result<Row, FieldErrors>,
        save: impl FnOnce(Row) -> ignite_core::RuntimeFuture<'a, Row>,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        match validated {
            Ok(row) => match save(row).await {
                Ok(record) => Ok(Ok(Value::Object(record))),
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

    fn navigation_icon(&self) -> Option<&str> {
        self.0.navigation_icon()
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
        id: i64,
    ) -> Result<Option<Value>, Error> {
        Ok(self
            .0
            .resource()
            .data
            .find_for(db, user, id)
            .await?
            .map(Value::Object))
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
        self.save(validated, |row| data.create_for(db, user, row))
            .await
    }

    async fn update(
        &self,
        db: &Database,
        user: &ActingUser,
        id: i64,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        let validated = self.checked(db, user, form, false, hidden).await?;
        let data = &self.0.resource().data;
        self.save(validated, |row| data.update_for(db, user, id, row))
            .await
    }

    async fn delete(&self, db: &Database, user: &ActingUser, id: i64) -> Result<u64, Error> {
        self.0.resource().data.delete_for(db, user, id).await
    }

    /// A runtime resource has no dashboard cards.
    async fn widgets(&self, _db: &Database) -> Result<Vec<StatWidget>, Error> {
        Ok(Vec::new())
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
