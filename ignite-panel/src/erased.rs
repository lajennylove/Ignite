//! Type-erased access to a resource, so the routes can serve many resource
//! types from one set of handlers.

use std::marker::PhantomData;

use diesel::mysql::MysqlConnection;
use diesel::result::{DatabaseErrorKind, Error as DieselError, QueryResult};
use ignite_core::{
    Column, Field, FieldErrors, FieldKind, FormInput, IgniteResource, LatestRecords, ListQuery,
    StatWidget,
};
use serde::Serialize;
use serde_json::Value;

/// The operations the routes need from a resource, without its type.
pub(crate) trait Erased: Send + Sync {
    fn slug(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn singular(&self) -> &'static str;
    fn per_page(&self) -> i64;
    fn fields(&self) -> &'static [Field];
    fn columns(&self) -> &'static [Column];
    /// One page of records as JSON, and the total count.
    fn list(&self, conn: &mut MysqlConnection, query: &ListQuery)
        -> QueryResult<(Vec<Value>, i64)>;
    fn find(&self, conn: &mut MysqlConnection, id: i64) -> QueryResult<Option<Value>>;
    /// `Err(errors)` when the form does not validate; the database is not touched then.
    fn create(
        &self,
        conn: &mut MysqlConnection,
        form: &FormInput,
    ) -> QueryResult<Result<Value, FieldErrors>>;
    fn update(
        &self,
        conn: &mut MysqlConnection,
        id: i64,
        form: &FormInput,
    ) -> QueryResult<Result<Value, FieldErrors>>;
    fn delete(&self, conn: &mut MysqlConnection, id: i64) -> QueryResult<usize>;
    /// The dashboard cards this resource provides.
    fn widgets(&self, conn: &mut MysqlConnection) -> QueryResult<Vec<StatWidget>>;
    /// The dashboard table this resource declares, if any.
    fn latest_records(&self) -> Option<LatestRecords>;
    /// The newest records as JSON, up to `limit`.
    fn latest(&self, conn: &mut MysqlConnection, limit: i64) -> QueryResult<Vec<Value>>;
    /// The choices of a lookup field, as `(value, label)` pairs.
    fn options(
        &self,
        conn: &mut MysqlConnection,
        field: &str,
    ) -> QueryResult<Vec<(String, String)>>;
}

/// Wraps an [`IgniteResource`] type as an [`Erased`] value.
pub(crate) struct Adapter<R>(PhantomData<fn() -> R>);

impl<R> Adapter<R> {
    pub(crate) fn new() -> Self {
        Adapter(PhantomData)
    }
}

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

    fn list(
        &self,
        conn: &mut MysqlConnection,
        query: &ListQuery,
    ) -> QueryResult<(Vec<Value>, i64)> {
        let page = R::list(conn, query)?;
        let rows = page
            .rows
            .into_iter()
            .map(to_json)
            .collect::<QueryResult<Vec<_>>>()?;
        Ok((rows, page.total))
    }

    fn find(&self, conn: &mut MysqlConnection, id: i64) -> QueryResult<Option<Value>> {
        R::find(conn, id)?.map(to_json).transpose()
    }

    fn create(
        &self,
        conn: &mut MysqlConnection,
        form: &FormInput,
    ) -> QueryResult<Result<Value, FieldErrors>> {
        match checked::<R>(conn, form, R::validate(form))? {
            Ok(input) => match R::create(conn, input) {
                Ok(record) => to_json(record).map(Ok),
                Err(error) => unique_violation(error, R::SLUG, R::fields()).map(Err),
            },
            Err(errors) => Ok(Err(errors)),
        }
    }

    fn update(
        &self,
        conn: &mut MysqlConnection,
        id: i64,
        form: &FormInput,
    ) -> QueryResult<Result<Value, FieldErrors>> {
        match checked::<R>(conn, form, R::validate_update(form))? {
            Ok(input) => match R::update(conn, id, input) {
                Ok(record) => to_json(record).map(Ok),
                Err(error) => unique_violation(error, R::SLUG, R::fields()).map(Err),
            },
            Err(errors) => Ok(Err(errors)),
        }
    }

    fn delete(&self, conn: &mut MysqlConnection, id: i64) -> QueryResult<usize> {
        R::delete(conn, id)
    }

    fn widgets(&self, conn: &mut MysqlConnection) -> QueryResult<Vec<StatWidget>> {
        R::widgets(conn)
    }

    fn latest_records(&self) -> Option<LatestRecords> {
        R::latest_records()
    }

    fn latest(&self, conn: &mut MysqlConnection, limit: i64) -> QueryResult<Vec<Value>> {
        R::latest(conn, limit)?.into_iter().map(to_json).collect()
    }

    fn options(
        &self,
        conn: &mut MysqlConnection,
        field: &str,
    ) -> QueryResult<Vec<(String, String)>> {
        R::options(conn, field)
    }
}

/// Adds to a validated form the errors of its lookup fields: a submitted value must be
/// one of the field's choices, so a row that does not exist is refused. A field that
/// already has an error keeps it. The database is read only for the lookup fields the
/// form submits.
fn checked<R: IgniteResource>(
    conn: &mut MysqlConnection,
    form: &FormInput,
    validated: Result<R::Input, FieldErrors>,
) -> QueryResult<Result<R::Input, FieldErrors>> {
    let mut errors = validated.as_ref().err().cloned().unwrap_or_default();
    for field in R::fields().iter().filter(|f| f.kind == FieldKind::Lookup) {
        let Some(value) = form.get(field.name) else {
            continue;
        };
        let choices = R::options(conn, field.name)?;
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

/// The index a unique-violation message names, if `error` is one. MySQL says
/// `for key 'index'`, and MySQL 8.0.19 and later say `for key 'table.index'`; the
/// table part is dropped.
pub(crate) fn unique_index(error: &DieselError) -> Option<&str> {
    let DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info) = error else {
        return None;
    };
    index_in_message(info.message())
}

fn index_in_message(message: &str) -> Option<&str> {
    let start = message.rfind("for key '")? + "for key '".len();
    let key = message[start..].trim_end_matches(['\'', '`']);
    key.rsplit('.').next()
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
fn unique_violation(error: DieselError, table: &str, fields: &[Field]) -> QueryResult<FieldErrors> {
    let field = unique_index(&error).and_then(|index| field_for_index(index, table, fields));
    match field {
        Some(field) => {
            let mut errors = FieldErrors::new();
            errors.insert(field.name, format!("{} is already taken.", field.label));
            Ok(errors)
        }
        None => Err(error),
    }
}

fn to_json<T: Serialize>(record: T) -> QueryResult<Value> {
    serde_json::to_value(record).map_err(|e| DieselError::SerializationError(Box::new(e)))
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
    fn an_index_on_another_table_matches_nothing() {
        let fields = [EMAIL];
        assert!(field_for_index("accounts_email_unique", "users", &fields).is_none());
    }

    #[test]
    fn mysql_8_table_qualified_key_is_read_as_the_index() {
        let message = "Duplicate entry 'a@example.test' for key 'users.users_email_unique'";
        assert_eq!(index_in_message(message), Some("users_email_unique"));
        let fields = [NAME, EMAIL];
        let index = index_in_message(message).unwrap();
        assert_eq!(
            field_for_index(index, "users", &fields).unwrap().name,
            "email"
        );
    }

    #[test]
    fn mariadb_key_without_table_is_read_as_the_index() {
        let message = "Duplicate entry 'Ada' for key 'users_name_unique'";
        assert_eq!(index_in_message(message), Some("users_name_unique"));
    }

    #[test]
    fn a_message_without_a_key_names_no_index() {
        assert_eq!(index_in_message("Lock wait timeout exceeded"), None);
    }
}
