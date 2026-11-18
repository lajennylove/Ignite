//! The trait a table implements to be managed by the admin panel.

use std::future::Future;

use orbit_orm::{Database, Error};
use serde::Serialize;

use crate::condition::FieldConditions;
use crate::field::{Column, Field};
use crate::form::{FieldErrors, FormInput};
use crate::policy::ActingUser;
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
    /// The label of the resource's navigation entry, as Filament's `$navigationLabel`. The
    /// default, `None`, is the [`TITLE`](Self::TITLE).
    const NAVIGATION_LABEL: Option<&'static str> = None;
    /// The navigation group the resource is listed under, as Filament's `$navigationGroup`:
    /// the group's name, shown as a heading in the sidebar. The panel's `navigation_groups`
    /// orders the groups. The default, `None`, lists the resource before the groups, as
    /// Filament lists ungrouped items first.
    const NAVIGATION_GROUP: Option<&'static str> = None;
    /// The resource's place in the navigation, as Filament's `$navigationSort`: a lower number
    /// comes first. The default, `None`, comes before any number. In a group, resources with
    /// the same place are listed by label; without a group, in the order they were added.
    const NAVIGATION_SORT: Option<i32> = None;
    /// The icon of the resource's navigation entry, as Filament's `$navigationIcon`: one or
    /// more CSS classes, such as `"dashicons dashicons-admin-post"`. The panel renders it as
    /// `<span class="..." aria-hidden="true"></span>` before the label, in place of the admin
    /// theme's icon. Ignite ships no icon font: the app loads the stylesheet that defines the
    /// classes. The default, `None`, shows the admin theme's icon, as before.
    const NAVIGATION_ICON: Option<&'static str> = None;

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

    /// The conditions that show or hide the form's fields, and the live fields, as Filament's
    /// `visible()`, `hidden()` and `live()` (see [`FieldConditions`]). The panel reads them
    /// once, when it is built. The default is none: every field shows. `#[derive(IgniteResource)]`
    /// writes it for `#[ignite(conditions = path)]`.
    ///
    /// The panel removes a hidden field from the submitted form before it calls
    /// [`validate`](Self::validate) or [`validate_update`](Self::validate_update), so a field
    /// that may be hidden must be one `validate` accepts as missing, such as an `Option` field
    /// of the derive: left out on update, it keeps its stored value.
    fn conditions() -> FieldConditions {
        FieldConditions::new()
    }

    /// The choices of a [`FieldKind::Lookup`](crate::FieldKind::Lookup) field, as
    /// `(value, label)` pairs in display order (added in v1), and of every other field whose
    /// kind [loads its options](crate::FieldKind::loads_options): a post object,
    /// relationship, taxonomy or user field. The panel calls it when it
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

    /// [`list`](Self::list) for the signed-in `user`: the panel's list calls it. Restrict the
    /// rows to the user here, as a Filament resource does in `getEloquentQuery()`. The default
    /// is [`list`](Self::list), for every user.
    fn list_for(
        db: &Database,
        user: &ActingUser,
        query: &ListQuery,
    ) -> impl Future<Output = Result<Page<Self::Record>, Error>> + Send {
        let _ = user;
        Self::list(db, query)
    }

    /// [`find`](Self::find) for the signed-in `user`: the panel finds the record of the edit
    /// form, of its save and of the delete action with it. Return `None` for a row the user's
    /// list leaves out, so the panel answers 404. The default is [`find`](Self::find).
    fn find_for(
        db: &Database,
        user: &ActingUser,
        id: i64,
    ) -> impl Future<Output = Result<Option<Self::Record>, Error>> + Send {
        let _ = user;
        Self::find(db, id)
    }

    /// [`create`](Self::create) for the signed-in `user`. The default is
    /// [`create`](Self::create).
    fn create_for(
        db: &Database,
        user: &ActingUser,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Record, Error>> + Send {
        let _ = user;
        Self::create(db, input)
    }

    /// [`update`](Self::update) for the signed-in `user`. The panel calls it only after
    /// [`find_for`](Self::find_for) found the row and the [`Policy`](crate::Policy) allowed the
    /// update. The default is [`update`](Self::update).
    fn update_for(
        db: &Database,
        user: &ActingUser,
        id: i64,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Record, Error>> + Send {
        let _ = user;
        Self::update(db, id, input)
    }

    /// [`delete`](Self::delete) for the signed-in `user`. The panel calls it only after
    /// [`find_for`](Self::find_for) found the row and the [`Policy`](crate::Policy) allowed the
    /// delete. The default is [`delete`](Self::delete).
    fn delete_for(
        db: &Database,
        user: &ActingUser,
        id: i64,
    ) -> impl Future<Output = Result<u64, Error>> + Send {
        let _ = user;
        Self::delete(db, id)
    }

    /// [`options`](Self::options) for the signed-in `user`: the choices the form shows and
    /// checks a submitted value against. The default is [`options`](Self::options).
    fn options_for(
        db: &Database,
        user: &ActingUser,
        field: &str,
    ) -> impl Future<Output = Result<Vec<(String, String)>, Error>> + Send {
        let _ = user;
        Self::options(db, field)
    }
}
