//! Type-erased access to a resource, so the routes can serve many resource
//! types from one set of handlers.

use std::marker::PhantomData;

use diesel::mysql::MysqlConnection;
use diesel::result::{DatabaseErrorKind, Error as DieselError, QueryResult};
use ignite_core::{Column, Field, FieldErrors, FormInput, IgniteResource, ListQuery, StatWidget};
use serde::Serialize;
use serde_json::Value;

/// The operations the routes need from a resource, without its type.
pub(crate) trait Erased: Send + Sync {
    fn slug(&self) -> &'static str;
    fn title(&self) -> &'static str;
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
        match R::validate(form) {
            Ok(input) => match R::create(conn, input) {
                Ok(record) => to_json(record).map(Ok),
                Err(error) => unique_violation(error, R::fields()).map(Err),
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
        match R::validate_update(form) {
            Ok(input) => match R::update(conn, id, input) {
                Ok(record) => to_json(record).map(Ok),
                Err(error) => unique_violation(error, R::fields()).map(Err),
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
}

/// Turns a unique-index violation into a field error on the field the index is
/// named after, such as `users_email_unique` for `email`, so the form shows
/// "Email is already taken." instead of a server error. Any other error, or a
/// violation that names no field, is returned unchanged.
fn unique_violation(error: DieselError, fields: &[Field]) -> QueryResult<FieldErrors> {
    let DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info) = &error else {
        return Err(error);
    };
    let message = info.message();
    let key = message
        .rfind("for key '")
        .map(|start| &message[start + "for key '".len()..])
        .map(|rest| {
            rest.trim_end_matches('\'')
                .trim_end_matches(|c| c == '\'' || c == '`')
        })
        .unwrap_or_default();
    // An exact match wins, then a field whose name appears in the index name.
    let field = fields
        .iter()
        .find(|f| f.name == key)
        .or_else(|| fields.iter().find(|f| key.contains(f.name)));
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
