//! Type-erased access to a resource, so the routes can serve many resource
//! types from one set of handlers.

use std::marker::PhantomData;

use ignite_core::{
    Column, Database, Field, FieldErrors, FieldKind, FormInput, IgniteResource, LatestRecords,
    ListQuery, StatWidget,
};
use orbit_orm::Error;
use serde::Serialize;
use serde_json::Value;

/// The operations the routes need from a resource, without its type.
#[rocket::async_trait]
pub(crate) trait Erased: Send + Sync {
    fn slug(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn singular(&self) -> &'static str;
    fn per_page(&self) -> i64;
    fn fields(&self) -> &'static [Field];
    fn columns(&self) -> &'static [Column];
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

/// Wraps an [`IgniteResource`] type as an [`Erased`] value.
pub(crate) struct Adapter<R>(PhantomData<fn() -> R>);

impl<R> Adapter<R> {
    pub(crate) fn new() -> Self {
        Adapter(PhantomData)
    }
}

#[rocket::async_trait]
impl<R: IgniteResource> Erased for Adapter<R> {
    fn slug(&self) -> &'static str {
        R::SLUG
    }

    fn title(&self) -> &'static str {
        R::TITLE
    }

    fn singular(&self) -> &'static str {
        R::SINGULAR
    }

    fn per_page(&self) -> i64 {
        R::PER_PAGE
    }

    fn fields(&self) -> &'static [Field] {
        R::fields()
    }

    fn columns(&self) -> &'static [Column] {
        R::columns()
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
                Err(error) => unique_violation(error, R::SLUG, R::fields()).map(Err),
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
                Err(error) => unique_violation(error, R::SLUG, R::fields()).map(Err),
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
    let mut errors = validated.as_ref().err().cloned().unwrap_or_default();
    for field in R::fields().iter().filter(|f| f.kind == FieldKind::Lookup) {
        let Some(value) = form.get(field.name) else {
            continue;
        };
        let choices = R::options(db, field.name).await?;
        if !choices.iter().any(|(choice, _)| choice == value.trim()) {
            errors
                .entry(field.name)
                .or_insert_with(|| "Choose one of the options.".to_string());
        }
    }
    Ok(if errors.is_empty() {
        validated
    } else {
        Err(errors)
    })
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
fn field_for_index<'a>(index: &str, table: &str, fields: &'a [Field]) -> Option<&'a Field> {
    fields
        .iter()
        .find(|f| unique_index_name(table, f.name) == index)
}

/// Turns a unique-index violation into a field error, so the form shows "Email is
/// already taken." instead of a server error.
///
/// The rule, the only one: the violated index must be named exactly
/// `{table}_{field}_unique` (see [`unique_index_name`]), where `table` is the
/// resource's SLUG and `field` is a form field's name. Matching is exact, so `name`
/// does not claim `users_username_unique`. Any other error, or a violation whose
/// index names no field, is returned unchanged.
fn unique_violation(error: Error, table: &str, fields: &[Field]) -> Result<FieldErrors, Error> {
    let field = match &error {
        Error::UniqueViolation { index: Some(index) } => field_for_index(index, table, fields),
        _ => None,
    };
    match field {
        Some(field) => {
            let mut errors = FieldErrors::new();
            errors.insert(field.name, format!("{} is already taken.", field.label));
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
    use ignite_core::FieldKind;

    const NAME: Field = Field {
        name: "name",
        label: "Name",
        kind: FieldKind::Text,
        required: true,
    };
    const EMAIL: Field = Field {
        name: "email",
        label: "Email",
        kind: FieldKind::Text,
        required: true,
    };

    #[test]
    fn name_does_not_match_an_index_on_username() {
        let fields = [NAME, EMAIL];
        assert!(field_for_index("users_username_unique", "users", &fields).is_none());
    }

    #[test]
    fn email_matches_the_email_index() {
        let fields = [NAME, EMAIL];
        let field = field_for_index("users_email_unique", "users", &fields).expect("a field");
        assert_eq!(field.name, "email");
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
