//! The trait a table implements to be managed by the admin panel.

use std::future::Future;

use orbit_orm::{Database, Error};
use serde::Serialize;

use crate::field::{Column, Field};
use crate::form::{FieldErrors, FormInput};
use crate::query::{ListQuery, Page};
use crate::widget::{LatestRecords, StatWidget};

/// Checks at compile time that `table` is `expected`. The derive uses it to check that the
/// `table` attribute names the same table as the Orbit model's `TABLE`.
pub const fn table_name_is(table: &str, expected: &str) -> bool {
    let (a, b) = (table.as_bytes(), expected.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// A table of records the panel can list, create, edit and delete.
///
/// Implement it by hand, or with `#[derive(IgniteResource)]`. The panel calls the query methods
/// with the app's [`Database`], and renders the forms from [`fields`](Self::fields) and the list
/// from [`columns`](Self::columns). The query methods are async, so each returns a future.
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
    fn widgets(db: &Database) -> impl Future<Output = Result<Vec<StatWidget>, Error>> + Send {
        let _ = db;
        async { Ok(Vec::new()) }
    }

    /// The dashboard table of the newest records, or `None` for no table (added in v1).
    fn latest_records() -> Option<LatestRecords> {
        None
    }

    /// Up to `limit` records, newest first (added in v1). The panel calls it with the
    /// `limit` of [`latest_records`](Self::latest_records). The default is no records.
    fn latest(
        db: &Database,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Self::Record>, Error>> + Send {
        let _ = (db, limit);
        async { Ok(Vec::new()) }
    }

    /// Form fields, in display order.
    fn fields() -> &'static [Field];
    /// Table columns, in display order. Each column's `key` is a field name.
    fn columns() -> &'static [Column];

    /// The choices of a [`FieldKind::Lookup`](crate::FieldKind::Lookup) field, as
    /// `(value, label)` pairs in display order (added in v1). The panel calls it when it
    /// shows the form, to fill the select, and when it checks a submitted value. The
    /// default is no choices. `#[derive(IgniteResource)]` writes it for `options = ...`
    /// and `belongs_to = ...`.
    fn options(
        db: &Database,
        field: &str,
    ) -> impl Future<Output = Result<Vec<(String, String)>, Error>> + Send {
        let _ = (db, field);
        async { Ok(Vec::new()) }
    }

    /// Returns one page of rows that match `query`.
    fn list(
        db: &Database,
        query: &ListQuery,
    ) -> impl Future<Output = Result<Page<Self::Record>, Error>> + Send;
    /// Returns the row with `id`, or `None` if there is no such row.
    fn find(
        db: &Database,
        id: i64,
    ) -> impl Future<Output = Result<Option<Self::Record>, Error>> + Send;
    /// Inserts a row from `input` and returns it.
    fn create(
        db: &Database,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Record, Error>> + Send;
    /// Updates the row with `id` from `input` and returns it.
    fn update(
        db: &Database,
        id: i64,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Record, Error>> + Send;
    /// Deletes the row with `id` and returns the number of rows deleted.
    fn delete(db: &Database, id: i64) -> impl Future<Output = Result<u64, Error>> + Send;

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
