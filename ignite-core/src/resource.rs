//! The trait a table implements to be managed by the admin panel.

use diesel::mysql::MysqlConnection;
use diesel::result::QueryResult;
use serde::Serialize;

use crate::field::{Column, Field};
use crate::form::{FieldErrors, FormInput};
use crate::query::{ListQuery, Page};
use crate::widget::StatWidget;

/// A table of records the panel can list, create, edit and delete.
///
/// Implement it by hand, or with `#[derive(IgniteResource)]`. The panel calls the query methods with a connection borrowed
/// from the pool, and renders the forms from [`fields`](Self::fields) and the
/// list from [`columns`](Self::columns).
pub trait IgniteResource: Send + Sync + 'static {
    /// One row, as the database returns it. It must serialize to JSON with
    /// field names matching `fields()` and `columns()`.
    type Record: Serialize + Send;
    /// Validated form input for create and update.
    type Input: Send;

    /// URL segment: `"users"` mounts the resource at `/admin/users`.
    const SLUG: &'static str;
    /// Heading and navigation label: `"Users"`.
    const TITLE: &'static str;
    /// The name of one record, for headings such as "New User" (added in v1).
    const SINGULAR: &'static str = "Record";
    /// Rows per page on the list view.
    const PER_PAGE: i64 = 20;

    /// Stat cards for the dashboard, in display order. The default is none.
    /// Override it to show counts, such as the total number of rows.
    fn widgets(conn: &mut MysqlConnection) -> QueryResult<Vec<StatWidget>> {
        let _ = conn;
        Ok(Vec::new())
    }

    /// Form fields, in display order.
    fn fields() -> &'static [Field];
    /// Table columns, in display order. Each column's `key` is a field name.
    fn columns() -> &'static [Column];

    /// Returns one page of rows that match `query`.
    fn list(conn: &mut MysqlConnection, query: &ListQuery) -> QueryResult<Page<Self::Record>>;
    /// Returns the row with `id`, or `None` if there is no such row.
    fn find(conn: &mut MysqlConnection, id: i64) -> QueryResult<Option<Self::Record>>;
    /// Inserts a row from `input` and returns it.
    fn create(conn: &mut MysqlConnection, input: Self::Input) -> QueryResult<Self::Record>;
    /// Updates the row with `id` from `input` and returns it.
    fn update(conn: &mut MysqlConnection, id: i64, input: Self::Input)
        -> QueryResult<Self::Record>;
    /// Deletes the row with `id` and returns the number of rows deleted.
    fn delete(conn: &mut MysqlConnection, id: i64) -> QueryResult<usize>;

    /// Turns the submitted form into `Input` for a create, or returns one message
    /// per field. Call [`Field::check_required`] for required fields.
    fn validate(form: &FormInput) -> Result<Self::Input, FieldErrors>;

    /// Turns the submitted form into `Input` for an update. The default is
    /// [`validate`](Self::validate). Override it when a field may be left blank
    /// on edit, such as a password that keeps its stored value.
    fn validate_update(form: &FormInput) -> Result<Self::Input, FieldErrors> {
        Self::validate(form)
    }
}
