//! Resources defined at run time, from data: their slug, fields, columns and queries are
//! values, not a Rust type, so an app can add and remove them while the panel serves.
//!
//! A static resource implements [`IgniteResource`](crate::IgniteResource) and is added with
//! `Ignite::resource`. A runtime resource is a [`RuntimeResource`] value, whose rows come from
//! a [`RuntimeData`] the app implements, and is added with [`register_runtime`]. The panel
//! serves both through the same list, form and delete flows, at the same `/admin/<slug>`
//! address, and a slug can belong to only one of them.
//!
//! ```ignore
//! register_runtime(
//!     RuntimeResource::new("books", "Books", "Book", BookData)
//!         .field(FieldDef { name: "title".into(), label: "Title".into(),
//!                           kind: FieldKindDef::Text, required: true })
//!         .column(ColumnDef { key: "title".into(), label: "Title".into(),
//!                             sortable: true, searchable: true }),
//! )?;
//! ```

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, PoisonError, RwLock};

use orbit_orm::{Database, Error};
use serde_json::{Map, Value};

use crate::condition::{FieldConditions, FormContext, Operation};
use crate::field::{self as check, Block, Column, Field, FieldKind};
use crate::form::{FieldErrors, FormInput};
use crate::layout::FormLayout;
use crate::policy::ActingUser;
use crate::query::{ListQuery, Page};

/// One record of a runtime resource: its values by key, as JSON. A row the panel lists or
/// edits must have a numeric `id`, and a key for each field and column it shows.
pub type Row = Map<String, Value>;

/// The future a [`RuntimeData`] method returns: boxed, so the trait can be a trait object.
/// Write the body as `Box::pin(async move { ... })`.
pub type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;

/// Where the rows of a runtime resource live. The app implements it, so Ignite assumes no
/// table layout: a custom post type can keep its rows in a shared `posts` table and its
/// values in a meta table.
///
/// The methods mirror the queries of [`IgniteResource`](crate::IgniteResource), with
/// [`Row`]s for records and for validated input. The panel calls the `*_for` methods, which
/// are given the signed-in [`ActingUser`]; by default each calls its method without the user,
/// so a source that does not depend on the user implements only those.
pub trait RuntimeData: Send + Sync + 'static {
    /// Returns one page of rows that match `query`. The sort key, when there is one, is a
    /// sortable column of the resource.
    fn list<'a>(&'a self, db: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>>;
    /// Returns the row with `id`, or `None` if there is no such row.
    fn find<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>>;
    /// Inserts a row from the validated `input` and returns it. See
    /// [`RegisteredResource::validate`] for the values `input` holds.
    ///
    /// An [`Error::UniqueViolation`] whose index is named `{slug}_{field}_unique` shows on the
    /// form as `"<label> is already taken."`, as for a static resource. Any other error is a
    /// server error. A source whose indexes are named otherwise (or that keeps values in a
    /// meta table without such an index) should return a violation with an index of that
    /// name, or check the value before it writes.
    fn create<'a>(&'a self, db: &'a Database, input: Row) -> RuntimeFuture<'a, Row>;
    /// Updates the row with `id` from the validated `input` and returns it. A key that is
    /// missing from `input`, such as a password left blank or a field the form did not send,
    /// keeps its stored value. A key whose value is `null` is an optional field the user cleared: store NULL, so the value
    /// is cleared, as a static resource does. A unique violation is reported as for
    /// [`create`](Self::create).
    fn update<'a>(&'a self, db: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row>;
    /// Deletes the row with `id` and returns the number of rows deleted.
    fn delete<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, u64>;
    /// The choices of the field `field`, as `(value, label)` pairs in display order, for a
    /// field whose kind [loads its options](FieldKindDef::loads_options): a lookup, post
    /// object, relationship, taxonomy or user field. The default is no choices.
    fn options<'a>(
        &'a self,
        db: &'a Database,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        let _ = (db, field);
        Box::pin(async { Ok(Vec::new()) })
    }

    /// [`list`](Self::list) for the signed-in `user`: the panel's list calls it. Restrict the
    /// rows to the user here, as a Filament resource does in `getEloquentQuery()`, such as an
    /// author who sees only their own posts. The total counts the same rows. The default is
    /// [`list`](Self::list), for every user.
    fn list_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        query: &'a ListQuery,
    ) -> RuntimeFuture<'a, Page<Row>> {
        let _ = user;
        self.list(db, query)
    }

    /// [`find`](Self::find) for the signed-in `user`: the panel finds the record of the edit
    /// form, of its save and of the delete action with it. Return `None` for a row the user's
    /// list leaves out, so the panel answers 404, as Filament resolves a record through
    /// `getEloquentQuery()`. The default is [`find`](Self::find), for every user.
    fn find_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        id: i64,
    ) -> RuntimeFuture<'a, Option<Row>> {
        let _ = user;
        self.find(db, id)
    }

    /// [`create`](Self::create) for the signed-in `user`, such as to make them the author.
    /// The default is [`create`](Self::create).
    fn create_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        input: Row,
    ) -> RuntimeFuture<'a, Row> {
        let _ = user;
        self.create(db, input)
    }

    /// [`update`](Self::update) for the signed-in `user`, such as to keep a status the user
    /// may not set. The panel calls it only after [`find_for`](Self::find_for) found the row
    /// and the [`Policy`](crate::Policy) allowed the update. The default is
    /// [`update`](Self::update).
    fn update_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        id: i64,
        input: Row,
    ) -> RuntimeFuture<'a, Row> {
        let _ = user;
        self.update(db, id, input)
    }

    /// [`delete`](Self::delete) for the signed-in `user`. The panel calls it only after
    /// [`find_for`](Self::find_for) found the row and the [`Policy`](crate::Policy) allowed
    /// the delete. The default is [`delete`](Self::delete).
    fn delete_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        id: i64,
    ) -> RuntimeFuture<'a, u64> {
        let _ = user;
        self.delete(db, id)
    }

    /// [`options`](Self::options) for the signed-in `user`: the panel fills the form's
    /// selects with these choices and checks a submitted value against them, so a choice left
    /// out cannot be saved, as a Filament select whose options depend on the user. The default
    /// is [`options`](Self::options).
    fn options_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        let _ = user;
        self.options(db, field)
    }
}

/// The kind of a [`FieldDef`]: every [`FieldKind`], with owned values so it can be loaded
/// from a database.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldKindDef {
    /// A single line of text. See [`FieldKind::Text`].
    Text,
    /// Several lines of text. See [`FieldKind::Textarea`].
    Textarea,
    /// A password, hashed on save. See [`FieldKind::Password`].
    Password,
    /// A whole number. See [`FieldKind::Number`].
    Number,
    /// A decimal number, with the browser's step, such as `"any"` or `"0.01"`. See
    /// [`FieldKind::Decimal`].
    Decimal(String),
    /// A yes/no value. See [`FieldKind::Checkbox`].
    Checkbox,
    /// One choice from a list of `(value, label)` pairs. See [`FieldKind::Select`].
    Select(Vec<(String, String)>),
    /// One choice from options that [`RuntimeData::options`] loads. See [`FieldKind::Lookup`].
    Lookup,
    /// A calendar date. See [`FieldKind::Date`].
    Date,
    /// A date and time. See [`FieldKind::DateTime`].
    DateTime,
    /// An email address. See [`FieldKind::Email`].
    Email,
    /// A URL. See [`FieldKind::Url`].
    Url,
    /// One choice from a list of `(value, label)` pairs, as radio buttons. See
    /// [`FieldKind::Radio`].
    Radio(Vec<(String, String)>),
    /// A color, as `#rrggbb`. See [`FieldKind::Color`].
    Color,
    /// An image from the media library. See [`FieldKind::Image`].
    Image,
    /// A file from the media library. See [`FieldKind::File`].
    File,
    /// A link, as JSON text. See [`FieldKind::Link`].
    Link,
    /// Rich text, edited with the app's editor. See [`FieldKind::Wysiwyg`].
    Wysiwyg,
    /// One post, from options [`RuntimeData::options`] loads. See [`FieldKind::PostObject`].
    PostObject,
    /// Several choices, from options [`RuntimeData::options`] loads. See
    /// [`FieldKind::Relationship`].
    Relationship,
    /// One taxonomy term, from loaded options. See [`FieldKind::Taxonomy`].
    Taxonomy,
    /// One user, from loaded options. See [`FieldKind::User`].
    User,
    /// Rows of the given sub-fields, as JSON text. See [`FieldKind::Repeater`].
    Repeater(Vec<FieldDef>),
    /// The given sub-fields, as one JSON object. See [`FieldKind::Group`].
    Group(Vec<FieldDef>),
    /// Rows that each take one of the given blocks, as JSON text. See
    /// [`FieldKind::FlexibleContent`].
    FlexibleContent(Vec<BlockDef>),
    /// Several images from the media library, as a JSON array of ids. See
    /// [`FieldKind::Gallery`].
    Gallery,
    /// The fields of another field group, reused. See [`FieldKind::Clone`].
    Clone(Vec<FieldDef>),
}

impl FieldKindDef {
    /// Whether the field's choices come from [`RuntimeData::options`] each time its form is
    /// shown. See [`FieldKind::loads_options`].
    pub fn loads_options(&self) -> bool {
        matches!(
            self,
            FieldKindDef::Lookup
                | FieldKindDef::PostObject
                | FieldKindDef::Relationship
                | FieldKindDef::Taxonomy
                | FieldKindDef::User
        )
    }

    /// Whether the field holds media library ids: an image, a file or a gallery.
    pub fn uses_media(&self) -> bool {
        matches!(
            self,
            FieldKindDef::Image | FieldKindDef::File | FieldKindDef::Gallery
        )
    }
}

/// One block of a [`FieldKindDef::FlexibleContent`] field: the owned twin of [`Block`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockDef {
    /// The block's name, stored as the `type` of each row that uses it.
    pub name: String,
    /// The label shown when choosing the block.
    pub label: String,
    /// The block's sub-fields, in display order.
    pub fields: Vec<FieldDef>,
}

impl From<Block> for BlockDef {
    fn from(block: Block) -> Self {
        BlockDef {
            name: block.name.to_string(),
            label: block.label.to_string(),
            fields: owned_fields(block.fields),
        }
    }
}

/// Owned definitions of static sub-fields.
fn owned_fields(fields: &[Field]) -> Vec<FieldDef> {
    fields.iter().copied().map(FieldDef::from).collect()
}

/// Owned `(value, label)` pairs of static choices.
fn owned_choices(choices: &[(&str, &str)]) -> Vec<(String, String)> {
    choices
        .iter()
        .map(|(value, label)| (value.to_string(), label.to_string()))
        .collect()
}

impl From<FieldKind> for FieldKindDef {
    fn from(kind: FieldKind) -> Self {
        match kind {
            FieldKind::Text => FieldKindDef::Text,
            FieldKind::Textarea => FieldKindDef::Textarea,
            FieldKind::Password => FieldKindDef::Password,
            FieldKind::Number => FieldKindDef::Number,
            FieldKind::Decimal(step) => FieldKindDef::Decimal(step.to_string()),
            FieldKind::Checkbox => FieldKindDef::Checkbox,
            FieldKind::Select(choices) => FieldKindDef::Select(owned_choices(choices)),
            FieldKind::Lookup => FieldKindDef::Lookup,
            FieldKind::Date => FieldKindDef::Date,
            FieldKind::DateTime => FieldKindDef::DateTime,
            FieldKind::Email => FieldKindDef::Email,
            FieldKind::Url => FieldKindDef::Url,
            FieldKind::Radio(choices) => FieldKindDef::Radio(owned_choices(choices)),
            FieldKind::Color => FieldKindDef::Color,
            FieldKind::Image => FieldKindDef::Image,
            FieldKind::File => FieldKindDef::File,
            FieldKind::Link => FieldKindDef::Link,
            FieldKind::Wysiwyg => FieldKindDef::Wysiwyg,
            FieldKind::PostObject => FieldKindDef::PostObject,
            FieldKind::Relationship => FieldKindDef::Relationship,
            FieldKind::Taxonomy => FieldKindDef::Taxonomy,
            FieldKind::User => FieldKindDef::User,
            FieldKind::Repeater(fields) => FieldKindDef::Repeater(owned_fields(fields)),
            FieldKind::Group(fields) => FieldKindDef::Group(owned_fields(fields)),
            FieldKind::FlexibleContent(blocks) => {
                FieldKindDef::FlexibleContent(blocks.iter().copied().map(BlockDef::from).collect())
            }
            FieldKind::Gallery => FieldKindDef::Gallery,
            FieldKind::Clone(fields) => FieldKindDef::Clone(owned_fields(fields)),
        }
    }
}

/// One input on the form of a runtime resource: the owned twin of [`Field`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldDef {
    /// The form field name. It matches a key of the row.
    pub name: String,
    /// The label shown next to the input.
    pub label: String,
    /// The input type, and how the value is read.
    pub kind: FieldKindDef,
    /// Whether the form rejects a blank value.
    pub required: bool,
}

impl From<Field> for FieldDef {
    fn from(field: Field) -> Self {
        FieldDef {
            name: field.name.to_string(),
            label: field.label.to_string(),
            kind: field.kind.into(),
            required: field.required,
        }
    }
}

/// One column of the list of a runtime resource: the owned twin of [`Column`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ColumnDef {
    /// The row key the column shows. It matches a field name.
    pub key: String,
    /// The column heading.
    pub label: String,
    /// Whether the heading links to a sort by this column.
    pub sortable: bool,
    /// Whether the search box matches this column.
    pub searchable: bool,
}

impl From<Column> for ColumnDef {
    fn from(column: Column) -> Self {
        ColumnDef {
            key: column.key.to_string(),
            label: column.label.to_string(),
            sortable: column.sortable,
            searchable: column.searchable,
        }
    }
}

/// A resource defined by data. Build it with [`new`](Self::new) and the
/// [`field`](Self::field) and [`column`](Self::column) methods, then add it to the panel
/// with [`register_runtime`].
#[derive(Clone)]
pub struct RuntimeResource {
    /// URL segment: `"books"` serves the resource at `/admin/books`. Lowercase letters,
    /// digits, `-` and `_` only.
    pub slug: String,
    /// Heading and navigation label: `"Books"`.
    pub title: String,
    /// The name of one record, for headings such as "New Book".
    pub singular: String,
    /// Rows per page on the list view. The default is 20, as for a static resource.
    pub per_page: i64,
    /// Form fields, in display order.
    pub fields: Vec<FieldDef>,
    /// Table columns, in display order. Each column's `key` is a field name.
    pub columns: Vec<ColumnDef>,
    /// Where the rows live.
    pub data: Arc<dyn RuntimeData>,
    /// The label of the navigation entry, as Filament's `$navigationLabel`. `None`, the
    /// default, is the [`title`](Self::title).
    pub navigation_label: Option<String>,
    /// The navigation group the resource is listed under, as Filament's `$navigationGroup`.
    /// See [`IgniteResource::NAVIGATION_GROUP`](crate::IgniteResource::NAVIGATION_GROUP).
    pub navigation_group: Option<String>,
    /// The resource's place in the navigation, as Filament's `$navigationSort`: lower first.
    /// See [`IgniteResource::NAVIGATION_SORT`](crate::IgniteResource::NAVIGATION_SORT).
    pub navigation_sort: Option<i32>,
    /// The name of the navigation entry's icon, such as `post`, as Filament's
    /// `$navigationIcon`. See [`IgniteResource::NAVIGATION_ICON`](crate::IgniteResource::NAVIGATION_ICON).
    pub navigation_icon: Option<String>,
    /// The conditions that show or hide the form's fields, and the live fields, as
    /// Filament's `visible()`, `hidden()` and `live()`. See [`FieldConditions`].
    pub conditions: FieldConditions,
    /// The layout of the create and edit forms, as Filament's layout components. See
    /// [`FormLayout`]. The default has none: the form renders as one card of fields.
    pub form_layout: FormLayout,
}

impl fmt::Debug for RuntimeResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeResource")
            .field("slug", &self.slug)
            .field("title", &self.title)
            .field("singular", &self.singular)
            .field("per_page", &self.per_page)
            .field("fields", &self.fields)
            .field("columns", &self.columns)
            .field("navigation_label", &self.navigation_label)
            .field("navigation_group", &self.navigation_group)
            .field("navigation_sort", &self.navigation_sort)
            .field("navigation_icon", &self.navigation_icon)
            .field("conditions", &self.conditions)
            .field("form_layout", &self.form_layout)
            .finish_non_exhaustive()
    }
}

impl RuntimeResource {
    /// A resource at `slug` with no fields or columns yet, whose rows come from `data`.
    pub fn new(
        slug: impl Into<String>,
        title: impl Into<String>,
        singular: impl Into<String>,
        data: impl RuntimeData,
    ) -> Self {
        RuntimeResource {
            slug: slug.into(),
            title: title.into(),
            singular: singular.into(),
            per_page: 20,
            fields: Vec::new(),
            columns: Vec::new(),
            data: Arc::new(data),
            navigation_label: None,
            navigation_group: None,
            navigation_sort: None,
            navigation_icon: None,
            conditions: FieldConditions::new(),
            form_layout: FormLayout::new(),
        }
    }

    /// Adds a form field after the others.
    pub fn field(mut self, field: FieldDef) -> Self {
        self.fields.push(field);
        self
    }

    /// Adds a list column after the others.
    pub fn column(mut self, column: ColumnDef) -> Self {
        self.columns.push(column);
        self
    }

    /// Sets the rows per page of the list view.
    pub fn per_page(mut self, per_page: i64) -> Self {
        self.per_page = per_page;
        self
    }

    /// Sets the label of the navigation entry, as Filament's `navigationLabel`.
    pub fn navigation_label(mut self, label: impl Into<String>) -> Self {
        self.navigation_label = Some(label.into());
        self
    }

    /// Lists the resource under the navigation group `group`, as Filament's `navigationGroup`.
    pub fn navigation_group(mut self, group: impl Into<String>) -> Self {
        self.navigation_group = Some(group.into());
        self
    }

    /// Sets the resource's place in the navigation, as Filament's `navigationSort`: lower first.
    pub fn navigation_sort(mut self, sort: i32) -> Self {
        self.navigation_sort = Some(sort);
        self
    }

    /// Sets the name of the navigation entry's icon, as Filament's `navigationIcon`: a name
    /// such as `"post"` or a custom post type's own, which the admin theme maps to its icon.
    /// See [`IgniteResource::NAVIGATION_ICON`](crate::IgniteResource::NAVIGATION_ICON).
    pub fn navigation_icon(mut self, name: impl Into<String>) -> Self {
        self.navigation_icon = Some(name.into());
        self
    }

    /// Shows the field `field` only when `rule` holds, as Filament's `->visible()`. See
    /// [`FieldConditions::visible`].
    pub fn visible(
        mut self,
        field: impl Into<String>,
        rule: impl Fn(&FormContext<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.conditions = self.conditions.visible(field, rule);
        self
    }

    /// Hides the field `field` when `rule` holds, as Filament's `->hidden()`. See
    /// [`FieldConditions::hidden`].
    pub fn hidden(
        mut self,
        field: impl Into<String>,
        rule: impl Fn(&FormContext<'_>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.conditions = self.conditions.hidden(field, rule);
        self
    }

    /// Shows the field `field` only on the forms of `operations`, as Filament's
    /// `->visibleOn()`.
    pub fn visible_on(mut self, field: impl Into<String>, operations: &[Operation]) -> Self {
        self.conditions = self.conditions.visible_on(field, operations);
        self
    }

    /// Hides the field `field` on the forms of `operations`, as Filament's `->hiddenOn()`.
    pub fn hidden_on(mut self, field: impl Into<String>, operations: &[Operation]) -> Self {
        self.conditions = self.conditions.hidden_on(field, operations);
        self
    }

    /// Makes the field `field` live, as Filament's `->live()`: a change to it in the browser
    /// shows the form's fields again for the new value. See [`FieldConditions::live`].
    pub fn live(mut self, field: impl Into<String>) -> Self {
        self.conditions = self.conditions.live(field);
        self
    }

    /// Fills the field `field` with the slug of the field `source` until the user edits it, as
    /// Filament's slug pattern. See [`FieldConditions::slug_from`].
    pub fn slug_from(mut self, field: impl Into<String>, source: impl Into<String>) -> Self {
        self.conditions = self.conditions.slug_from(field, source);
        self
    }

    /// Sets the layout of the create and edit forms: columns, sections and groups, as
    /// Filament's layout components. See [`FormLayout`].
    pub fn form_layout(mut self, layout: FormLayout) -> Self {
        self.form_layout = layout;
        self
    }
}

/// The checks of [`RegisteredResource::validate`]. Each error is keyed by its field's name.
/// The fields of `hidden` are skipped: they are not checked and the row has no key for them.
fn validate_fields(
    fields: &[FieldDef],
    form: &FormInput,
    creating: bool,
    hidden: &BTreeSet<String>,
) -> Result<Row, FieldErrors> {
    let mut errors = FieldErrors::new();
    let mut row = Row::new();
    for field in fields.iter().filter(|f| !hidden.contains(&f.name)) {
        let name = field.name.as_str();
        let label = field.label.as_str();
        // A checkbox is never blank: an unticked checkbox is false.
        if field.kind == FieldKindDef::Checkbox {
            row.insert(name.to_string(), Value::Bool(form.checkbox(name)));
            continue;
        }
        let is_password = field.kind == FieldKindDef::Password;
        // A password is required only when creating. On an edit a blank one keeps the hash.
        let present = if field.required && (creating || !is_password) {
            check::check_required(name, label, form, &mut errors)
        } else {
            form.get(name)
        };
        let Some(text) = present else {
            // On an update, a field the form did not send keeps its stored value, and a blank
            // password keeps its hash. A field sent blank is cleared to NULL.
            let keep = !creating && (is_password || !form.has(name));
            if !keep {
                row.insert(name.to_string(), Value::Null);
            }
            continue;
        };
        let value = match &field.kind {
            FieldKindDef::Password => match crate::hash_password(text) {
                Ok(hash) => Some(Value::String(hash)),
                Err(_) => {
                    errors.insert(
                        name.to_string(),
                        "Could not save this password.".to_string(),
                    );
                    None
                }
            },
            FieldKindDef::Number => {
                check::check_whole_number::<i64>(name, label, text, &mut errors).map(Value::from)
            }
            FieldKindDef::Decimal(_) => {
                check::check_decimal::<f64>(name, label, text, &mut errors).map(Value::from)
            }
            FieldKindDef::Select(choices) => {
                let text = text.trim();
                let choices: Vec<(&str, &str)> = choices
                    .iter()
                    .map(|(value, label)| (value.as_str(), label.as_str()))
                    .collect();
                check::check_choice(name, text, &choices, &mut errors)
                    .then(|| Value::String(text.to_string()))
            }
            FieldKindDef::Lookup => Some(Value::String(text.trim().to_string())),
            FieldKindDef::Date => check::check_date(name, label, text, &mut errors)
                .map(|date| Value::String(date.format("%Y-%m-%d").to_string())),
            FieldKindDef::DateTime => check::check_datetime(name, label, text, &mut errors)
                .map(|stamp| Value::String(stamp.format("%Y-%m-%d %H:%M:%S").to_string())),
            FieldKindDef::Text | FieldKindDef::Textarea | FieldKindDef::Checkbox => {
                Some(Value::String(text.to_string()))
            }
            kind => crate::value::check_value(name, label, field.required, kind, text, &mut errors)
                .map(Value::String),
        };
        if let Some(value) = value {
            row.insert(name.to_string(), value);
        }
    }
    if errors.is_empty() {
        Ok(row)
    } else {
        Err(errors)
    }
}

/// Why the registry refused a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// The slug is not one URL segment of lowercase letters, digits, `-` and `_`.
    InvalidSlug(String),
    /// Another resource, static or runtime, or one of the panel's own pages, has the slug.
    SlugTaken(String),
    /// A field name is not a lowercase identifier: lowercase ASCII letters, digits and `_`,
    /// starting with a letter, at most 64 bytes ([`MAX_FIELD_NAME`]).
    InvalidFieldName {
        /// The slug of the resource.
        slug: String,
        /// The name that was refused.
        field: String,
    },
    /// Two fields of the resource have the same name.
    DuplicateField {
        /// The slug of the resource.
        slug: String,
        /// The name used twice.
        field: String,
    },
    /// No runtime resource has the slug.
    NotRegistered(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::InvalidSlug(slug) => write!(
                f,
                "the slug `{slug}` is not valid: use lowercase letters, digits, `-` and `_`"
            ),
            RuntimeError::SlugTaken(slug) => write!(
                f,
                "the slug `{slug}` is already used by another Ignite resource"
            ),
            RuntimeError::InvalidFieldName { slug, field } => write!(
                f,
                "the field `{field}` of the resource `{slug}` is not valid: use lowercase \
                 letters, digits and `_`, starting with a letter, at most {MAX_FIELD_NAME} bytes"
            ),
            RuntimeError::DuplicateField { slug, field } => {
                write!(f, "the resource `{slug}` has two fields named `{field}`")
            }
            RuntimeError::NotRegistered(slug) => {
                write!(f, "no runtime resource is registered at `{slug}`")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

/// A runtime resource as the registry holds it: the definition, ready for the panel.
pub struct RegisteredResource {
    resource: RuntimeResource,
}

impl RegisteredResource {
    /// The definition that was registered.
    pub fn resource(&self) -> &RuntimeResource {
        &self.resource
    }

    /// The slug, as the panel's routes match it.
    pub fn slug(&self) -> &str {
        &self.resource.slug
    }

    /// The heading and navigation label.
    pub fn title(&self) -> &str {
        &self.resource.title
    }

    /// The name of one record.
    pub fn singular(&self) -> &str {
        &self.resource.singular
    }

    /// The label of the navigation entry: the navigation label, or the title.
    pub fn navigation_label(&self) -> &str {
        self.resource
            .navigation_label
            .as_deref()
            .unwrap_or(&self.resource.title)
    }

    /// The navigation group the resource is listed under, if any.
    pub fn navigation_group(&self) -> Option<&str> {
        self.resource.navigation_group.as_deref()
    }

    /// The resource's place in the navigation, if set.
    pub fn navigation_sort(&self) -> Option<i32> {
        self.resource.navigation_sort
    }

    /// The name of the navigation entry's icon, if set.
    pub fn navigation_icon(&self) -> Option<&str> {
        self.resource.navigation_icon.as_deref()
    }

    /// The form fields, in display order.
    pub fn fields(&self) -> &[FieldDef] {
        &self.resource.fields
    }

    /// The list columns, in display order.
    pub fn columns(&self) -> &[ColumnDef] {
        &self.resource.columns
    }

    /// Turns the submitted form into a [`Row`] for a create, or returns one message per
    /// field. It runs the same checks as a static resource, with the same messages:
    /// required fields ([`Field::check_required`](crate::Field::check_required)), whole
    /// numbers, decimals, dates, date-times and select choices. The row holds one key per
    /// field:
    ///
    /// - text, textarea, select, lookup: a string. A required value is trimmed.
    /// - email, URL, radio, color, image, file, link, wysiwyg, post object, relationship,
    ///   taxonomy, user, repeater, group, flexible content, gallery, clone: the text
    ///   [`Field::check_value`](crate::Field::check_value) returns. The structured kinds
    ///   hold JSON text.
    /// - number: an integer. Decimal: a number.
    /// - checkbox: `true` or `false`; a checkbox is never blank.
    /// - date: `"YYYY-MM-DD"`. Date-time: `"YYYY-MM-DD HH:MM:SS"`.
    /// - password: the hash of the password ([`hash_password`](crate::hash_password)).
    /// - a blank field that is not required: `null`.
    ///
    /// Each error in [`FieldErrors`] is keyed by the field's name, an owned `String`.
    ///
    /// A lookup, post object, relationship, taxonomy or user value is checked against
    /// [`RuntimeData::options`] by the panel, which can read the database, and so is a media
    /// id against the registered [`MediaLibrary`](crate::MediaLibrary).
    pub fn validate(&self, form: &FormInput) -> Result<Row, FieldErrors> {
        validate_fields(&self.resource.fields, form, true, &BTreeSet::new())
    }

    /// Turns the submitted form into a [`Row`] for an update. It is
    /// [`validate`](Self::validate), except that:
    ///
    /// - a blank password is not an error: the row leaves its key out, so the stored
    ///   password is kept;
    /// - an optional field the form did not send at all ([`FormInput::has`] is false) is left
    ///   out of the row, so its stored value is kept;
    /// - an optional field sent blank is `null`, so clearing it on the edit form clears the
    ///   stored value, as `#[derive(IgniteResource)]` does since 0.3.0;
    /// - a checkbox the form did not send is `false`, because an unticked HTML checkbox sends
    ///   nothing.
    pub fn validate_update(&self, form: &FormInput) -> Result<Row, FieldErrors> {
        validate_fields(&self.resource.fields, form, false, &BTreeSet::new())
    }

    /// The conditions that show or hide the form's fields.
    pub fn conditions(&self) -> &FieldConditions {
        &self.resource.conditions
    }

    /// The layout of the create and edit forms.
    pub fn form_layout(&self) -> &FormLayout {
        &self.resource.form_layout
    }

    /// [`validate`](Self::validate) for a form whose fields `hidden` are hidden (see
    /// [`FieldConditions::hidden_fields`]): they are not checked, a required one included, and
    /// the row has no key for them, so the source writes nothing for them. The panel calls it.
    pub fn validate_shown(
        &self,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Row, FieldErrors> {
        validate_fields(&self.resource.fields, form, true, hidden)
    }

    /// [`validate_update`](Self::validate_update) for a form whose fields `hidden` are hidden:
    /// the row has no key for them, so their stored values are kept, whatever the form sent.
    pub fn validate_update_shown(
        &self,
        form: &FormInput,
        hidden: &BTreeSet<String>,
    ) -> Result<Row, FieldErrors> {
        validate_fields(&self.resource.fields, form, false, hidden)
    }
}

/// The registry: the runtime resources in registration order, and the slugs reserved for
/// static resources and for the panel's own pages.
#[derive(Default)]
struct Registry {
    resources: Vec<Arc<RegisteredResource>>,
    reserved: HashSet<String>,
}

static REGISTRY: RwLock<Option<Registry>> = RwLock::new(None);

/// Runs `f` on the registry, read-only. The lock is never held across an `.await`: every
/// caller is synchronous.
fn read<T>(f: impl FnOnce(&Registry) -> T) -> T {
    let guard = REGISTRY.read().unwrap_or_else(PoisonError::into_inner);
    match guard.as_ref() {
        Some(registry) => f(registry),
        None => f(&Registry::default()),
    }
}

/// Runs `f` on the registry, to change it.
fn write<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
    let mut guard = REGISTRY.write().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(Registry::default))
}

/// The longest field name a runtime resource accepts, in bytes, as for a MySQL or MariaDB
/// column name. The limit is a validation rule: names are owned by their resource and freed
/// with it.
pub const MAX_FIELD_NAME: usize = 64;

/// Whether `name` is a field name a runtime resource accepts: lowercase ASCII letters, digits
/// and `_`, starting with a letter, at most [`MAX_FIELD_NAME`] bytes, as a column name is.
fn valid_field_name(name: &str) -> bool {
    name.len() <= MAX_FIELD_NAME
        && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Whether `slug` is one URL segment the panel's `/<slug>` routes match: lowercase ASCII
/// letters, digits, `-` and `_`, as Filament's kebab-case slugs are.
fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Adds a runtime resource to the panel. It shows in the navigation and the dashboard, and
/// its list, forms and delete action answer at `/<mount>/<slug>`, from the next request on.
/// Create its permissions (`<slug>.viewAny` and the others) in the same request with
/// `ignite_panel::register_runtime_permissions`; otherwise the panel creates them on the next
/// signed-in request.
///
/// Refuses a slug that is not valid, a slug that a static resource, another runtime resource
/// or one of the panel's own pages (such as `login`) uses, and two fields with one name.
pub fn register_runtime(resource: RuntimeResource) -> Result<(), RuntimeError> {
    if !valid_slug(&resource.slug) {
        return Err(RuntimeError::InvalidSlug(resource.slug));
    }
    if let Some(field) = resource.fields.iter().find(|f| !valid_field_name(&f.name)) {
        return Err(RuntimeError::InvalidFieldName {
            slug: resource.slug.clone(),
            field: field.name.clone(),
        });
    }
    let mut names = HashSet::new();
    if let Some(field) = resource.fields.iter().find(|f| !names.insert(&f.name)) {
        return Err(RuntimeError::DuplicateField {
            slug: resource.slug.clone(),
            field: field.name.clone(),
        });
    }
    write(|registry| {
        let slug = resource.slug.as_str();
        if registry.reserved.contains(slug) || registry.resources.iter().any(|r| r.slug() == slug) {
            return Err(RuntimeError::SlugTaken(resource.slug.clone()));
        }
        registry
            .resources
            .push(Arc::new(RegisteredResource { resource }));
        Ok(())
    })
}

/// Removes the runtime resource at `slug`. From the next request on, it is gone from the
/// navigation and its routes answer 404. Its permission rows stay, so the grants come back
/// if the resource is registered again.
pub fn unregister_runtime(slug: &str) -> Result<(), RuntimeError> {
    write(|registry| {
        let before = registry.resources.len();
        registry.resources.retain(|r| r.slug() != slug);
        if registry.resources.len() == before {
            Err(RuntimeError::NotRegistered(slug.to_string()))
        } else {
            Ok(())
        }
    })
}

/// The runtime resource at `slug`, if one is registered.
pub fn runtime_resource(slug: &str) -> Option<Arc<RegisteredResource>> {
    read(|registry| {
        registry
            .resources
            .iter()
            .find(|r| r.slug() == slug)
            .cloned()
    })
}

/// Every runtime resource, in registration order.
pub fn runtime_resources() -> Vec<Arc<RegisteredResource>> {
    read(|registry| registry.resources.clone())
}

/// Reserves `slugs`, so no runtime resource can take them. The panel reserves the slugs of
/// its static resources and of its own pages when it is built. Reserving a slug twice is
/// fine. Refuses, and reserves none, when a runtime resource already has one of them.
pub fn reserve_slugs(slugs: &[&str]) -> Result<(), RuntimeError> {
    write(|registry| {
        if let Some(slug) = slugs
            .iter()
            .find(|slug| registry.resources.iter().any(|r| r.slug() == **slug))
        {
            return Err(RuntimeError::SlugTaken(slug.to_string()));
        }
        registry
            .reserved
            .extend(slugs.iter().map(|slug| slug.to_string()));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoData;

    impl RuntimeData for NoData {
        fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
            unreachable!()
        }
        fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
            unreachable!()
        }
        fn create<'a>(&'a self, _: &'a Database, _: Row) -> RuntimeFuture<'a, Row> {
            unreachable!()
        }
        fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
            unreachable!()
        }
        fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
            unreachable!()
        }
    }

    /// A resource with the fields `growth_title` and `growth_status`, whose label and
    /// choices change with `n`.
    fn edited(slug: &str, n: usize) -> RuntimeResource {
        RuntimeResource::new(slug, format!("Title {n}"), format!("One {n}"), NoData)
            .field(FieldDef {
                name: "growth_title".into(),
                label: format!("Title {n}"),
                kind: FieldKindDef::Text,
                required: n.is_multiple_of(2),
            })
            .field(FieldDef {
                name: "growth_status".into(),
                label: format!("Status {n}"),
                kind: FieldKindDef::Select(vec![(format!("v{n}"), format!("Value {n}"))]),
                required: false,
            })
            .column(ColumnDef {
                key: "growth_title".into(),
                label: format!("Column {n}"),
                sortable: false,
                searchable: false,
            })
    }

    // Field names are owned by their resource: editing a definition a thousand times keeps
    // nothing of the old ones, and an error is keyed by the field's own name.
    #[test]
    fn editing_a_definition_a_thousand_times_keeps_owned_names() {
        register_runtime(edited("growth", 0)).expect("register");
        for n in 1..=1000 {
            unregister_runtime("growth").expect("unregister");
            register_runtime(edited("growth", n)).expect("register again");
            let registered = runtime_resource("growth").expect("registered");
            let mut values = std::collections::HashMap::new();
            values.insert("growth_status".to_string(), "not a choice".to_string());
            let errors = registered
                .validate(&FormInput::new(values.clone()))
                .unwrap_err();
            assert!(errors.contains_key("growth_status"));
            let _ = registered.validate_update(&FormInput::new(values));
        }
        // The last registration is the only one the registry holds.
        let registered = runtime_resource("growth").expect("registered");
        assert_eq!(registered.title(), "Title 1000");
        assert_eq!(
            runtime_resources()
                .iter()
                .filter(|r| r.slug() == "growth")
                .count(),
            1
        );
        unregister_runtime("growth").expect("unregister");
    }
}
