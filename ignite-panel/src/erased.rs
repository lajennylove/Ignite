//! Type-erased access to a resource, so the routes can serve many resource
//! types from one set of handlers.

use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use ignite_core::{
    ColumnDef, Database, FieldDef, FieldErrors, FieldKind, FieldKindDef, FormInput, IgniteResource,
    LatestRecords, ListQuery, RegisteredResource, Row, StatWidget,
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
    /// The form fields, owned, so static and runtime resources render the same way.
    fn fields(&self) -> &[FieldDef];
    /// The list columns, owned.
    fn columns(&self) -> &[ColumnDef];
    /// One page of records as JSON, and the total count.
    async fn list(&self, db: &Database, query: &ListQuery) -> Result<(Vec<Value>, i64), Error>;
    async fn find(&self, db: &Database, id: i64) -> Result<Option<Value>, Error>;
    /// `Err(errors)` when the form does not validate; the database is not touched then.
    async fn create(
        &self,
        db: &Database,
        form: &FormInput,
    ) -> Result<Result<Value, FieldErrors>, Error>;
    async fn update(
        &self,
        db: &Database,
        id: i64,
        form: &FormInput,
    ) -> Result<Result<Value, FieldErrors>, Error>;
    async fn delete(&self, db: &Database, id: i64) -> Result<u64, Error>;
    /// The dashboard cards this resource provides.
    async fn widgets(&self, db: &Database) -> Result<Vec<StatWidget>, Error>;
    /// The dashboard table this resource declares, if any.
    fn latest_records(&self) -> Option<LatestRecords>;
    /// The newest records as JSON, up to `limit`.
    async fn latest(&self, db: &Database, limit: i64) -> Result<Vec<Value>, Error>;
    /// The choices of a lookup field, as `(value, label)` pairs.
    async fn options(&self, db: &Database, field: &str) -> Result<Vec<(String, String)>, Error>;
}

/// Wraps an [`IgniteResource`] type as an [`Erased`] value. Its fields and columns are
/// converted to owned definitions once, when the panel is built.
pub(crate) struct Adapter<R> {
    fields: Vec<FieldDef>,
    columns: Vec<ColumnDef>,
    resource: PhantomData<fn() -> R>,
}

impl<R: IgniteResource> Adapter<R> {
    pub(crate) fn new() -> Self {
        Adapter {
            fields: R::fields().iter().copied().map(FieldDef::from).collect(),
            columns: R::columns().iter().copied().map(ColumnDef::from).collect(),
            resource: PhantomData,
        }
    }
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

    fn fields(&self) -> &[FieldDef] {
        &self.fields
    }

    fn columns(&self) -> &[ColumnDef] {
        &self.columns
    }

    async fn list(&self, db: &Database, query: &ListQuery) -> Result<(Vec<Value>, i64), Error> {
        let page = R::list(db, query).await?;
        let rows = page
            .rows
            .into_iter()
            .map(to_json)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((rows, page.total))
    }

    async fn find(&self, db: &Database, id: i64) -> Result<Option<Value>, Error> {
        R::find(db, id).await?.map(to_json).transpose()
    }

    async fn create(
        &self,
        db: &Database,
        form: &FormInput,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        match checked::<R>(db, form, R::validate(form)).await? {
            Ok(input) => match R::create(db, input).await {
                Ok(record) => to_json(record).map(Ok),
                Err(error) => unique_violation(error, R::SLUG, &static_keys::<R>()).map(Err),
            },
            Err(errors) => Ok(Err(errors)),
        }
    }

    async fn update(
        &self,
        db: &Database,
        id: i64,
        form: &FormInput,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        match checked::<R>(db, form, R::validate_update(form)).await? {
            Ok(input) => match R::update(db, id, input).await {
                Ok(record) => to_json(record).map(Ok),
                Err(error) => unique_violation(error, R::SLUG, &static_keys::<R>()).map(Err),
            },
            Err(errors) => Ok(Err(errors)),
        }
    }

    async fn delete(&self, db: &Database, id: i64) -> Result<u64, Error> {
        R::delete(db, id).await
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

    async fn options(&self, db: &Database, field: &str) -> Result<Vec<(String, String)>, Error> {
        R::options(db, field).await
    }
}

/// Adds to a validated form the errors of its lookup fields: a submitted value must be
/// one of the field's choices, so a row that does not exist is refused. A field that
/// already has an error keeps it. The database is read only for the lookup fields the
/// form submits.
async fn checked<R: IgniteResource>(
    db: &Database,
    form: &FormInput,
    validated: Result<R::Input, FieldErrors>,
) -> Result<Result<R::Input, FieldErrors>, Error> {
    let errors = validated.as_ref().err().cloned().unwrap_or_default();
    let lookups: Vec<&'static str> = R::fields()
        .iter()
        .filter(|f| f.kind == FieldKind::Lookup)
        .map(|f| f.name)
        .collect();
    let errors = lookup_errors(&lookups, form, errors, |field| R::options(db, field)).await?;
    Ok(if errors.is_empty() {
        validated
    } else {
        Err(errors)
    })
}

/// `errors`, with "Choose one of the options." for each lookup field in `lookups` (their
/// error keys) whose submitted value is not one of the choices `options` returns for it.
/// Static and runtime resources share this check.
async fn lookup_errors<F, Fut>(
    lookups: &[&'static str],
    form: &FormInput,
    mut errors: FieldErrors,
    options: F,
) -> Result<FieldErrors, Error>
where
    F: Fn(&'static str) -> Fut,
    Fut: Future<Output = Result<Vec<(String, String)>, Error>>,
{
    for &name in lookups {
        let Some(value) = form.get(name) else {
            continue;
        };
        let choices = options(name).await?;
        if !choices.iter().any(|(choice, _)| choice == value.trim()) {
            errors
                .entry(name)
                .or_insert_with(|| "Choose one of the options.".to_string());
        }
    }
    Ok(errors)
}

/// Wraps a runtime resource from the registry as an [`Erased`] value, so the routes serve
/// it as they serve a static one.
pub(crate) struct RuntimeAdapter(pub(crate) Arc<RegisteredResource>);

impl RuntimeAdapter {
    /// Validates `form` with the resource's definitions, then checks its lookup fields.
    async fn checked(
        &self,
        db: &Database,
        form: &FormInput,
        creating: bool,
    ) -> Result<Result<Row, FieldErrors>, Error> {
        let validated = if creating {
            self.0.validate(form)
        } else {
            self.0.validate_update(form)
        };
        let errors = validated.as_ref().err().cloned().unwrap_or_default();
        let lookups: Vec<&'static str> = self
            .0
            .fields()
            .iter()
            .filter(|f| f.kind == FieldKindDef::Lookup)
            .filter_map(|f| self.0.error_key(&f.name))
            .collect();
        let data = &self.0.resource().data;
        let errors = lookup_errors(&lookups, form, errors, |field| data.options(db, field)).await?;
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
                    let keys: Vec<(&'static str, &str)> = self
                        .0
                        .fields()
                        .iter()
                        .filter_map(|f| Some((self.0.error_key(&f.name)?, f.label.as_str())))
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

    fn fields(&self) -> &[FieldDef] {
        self.0.fields()
    }

    fn columns(&self) -> &[ColumnDef] {
        self.0.columns()
    }

    async fn list(&self, db: &Database, query: &ListQuery) -> Result<(Vec<Value>, i64), Error> {
        let page = self.0.resource().data.list(db, query).await?;
        Ok((
            page.rows.into_iter().map(Value::Object).collect(),
            page.total,
        ))
    }

    async fn find(&self, db: &Database, id: i64) -> Result<Option<Value>, Error> {
        Ok(self
            .0
            .resource()
            .data
            .find(db, id)
            .await?
            .map(Value::Object))
    }

    async fn create(
        &self,
        db: &Database,
        form: &FormInput,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        let validated = self.checked(db, form, true).await?;
        let data = &self.0.resource().data;
        self.save(validated, |row| data.create(db, row)).await
    }

    async fn update(
        &self,
        db: &Database,
        id: i64,
        form: &FormInput,
    ) -> Result<Result<Value, FieldErrors>, Error> {
        let validated = self.checked(db, form, false).await?;
        let data = &self.0.resource().data;
        self.save(validated, |row| data.update(db, id, row)).await
    }

    async fn delete(&self, db: &Database, id: i64) -> Result<u64, Error> {
        self.0.resource().data.delete(db, id).await
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

    async fn options(&self, db: &Database, field: &str) -> Result<Vec<(String, String)>, Error> {
        self.0.resource().data.options(db, field).await
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
    fields: &'a [(&'static str, &'b str)],
) -> Option<&'a (&'static str, &'b str)> {
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
    fields: &[(&'static str, &str)],
) -> Result<FieldErrors, Error> {
    let field = match &error {
        Error::UniqueViolation { index: Some(index) } => field_for_index(index, table, fields),
        _ => None,
    };
    match field {
        Some((name, label)) => {
            let mut errors = FieldErrors::new();
            errors.insert(name, format!("{label} is already taken."));
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
