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

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use orbit_orm::{Database, Error};
use serde_json::{Map, Value};

use crate::field::{Column, Field, FieldKind};
use crate::form::{FieldErrors, FormInput};
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
/// [`Row`]s for records and for validated input.
pub trait RuntimeData: Send + Sync + 'static {
    /// Returns one page of rows that match `query`. The sort key, when there is one, is a
    /// sortable column of the resource.
    fn list<'a>(&'a self, db: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>>;
    /// Returns the row with `id`, or `None` if there is no such row.
    fn find<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>>;
    /// Inserts a row from the validated `input` and returns it. See
    /// [`RuntimeResource::validate`] for the values `input` holds.
    fn create<'a>(&'a self, db: &'a Database, input: Row) -> RuntimeFuture<'a, Row>;
    /// Updates the row with `id` from the validated `input` and returns it. A key that is
    /// missing from `input`, such as a password left blank, keeps its stored value.
    fn update<'a>(&'a self, db: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row>;
    /// Deletes the row with `id` and returns the number of rows deleted.
    fn delete<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, u64>;
    /// The choices of the [`FieldKindDef::Lookup`] field `field`, as `(value, label)` pairs
    /// in display order. The default is no choices.
    fn options<'a>(
        &'a self,
        db: &'a Database,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        let _ = (db, field);
        Box::pin(async { Ok(Vec::new()) })
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
            FieldKind::Select(choices) => FieldKindDef::Select(
                choices
                    .iter()
                    .map(|(value, label)| (value.to_string(), label.to_string()))
                    .collect(),
            ),
            FieldKind::Lookup => FieldKindDef::Lookup,
            FieldKind::Date => FieldKindDef::Date,
            FieldKind::DateTime => FieldKindDef::DateTime,
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

    /// Turns the submitted form into a [`Row`] for a create, or returns one message per
    /// field. It runs the same checks as a static resource, with the same messages:
    /// required fields ([`Field::check_required`]), whole numbers, decimals, dates,
    /// date-times and select choices. The row holds one key per field:
    ///
    /// - text, textarea, select, lookup: a string. A required value is trimmed.
    /// - number: an integer. Decimal: a number.
    /// - checkbox: `true` or `false`; a checkbox is never blank.
    /// - date: `"YYYY-MM-DD"`. Date-time: `"YYYY-MM-DD HH:MM:SS"`.
    /// - password: the hash of the password ([`hash_password`](crate::hash_password)).
    /// - a blank field that is not required: `null`.
    ///
    /// A lookup value is checked against [`RuntimeData::options`] by the panel, which can
    /// read the database.
    pub fn validate(&self, form: &FormInput) -> Result<Row, FieldErrors> {
        validate_fields(intern_fields(&self.fields), form, true)
    }

    /// Turns the submitted form into a [`Row`] for an update. It is
    /// [`validate`](Self::validate), except that a blank password is not an error: the row
    /// leaves its key out, so the stored password is kept.
    pub fn validate_update(&self, form: &FormInput) -> Result<Row, FieldErrors> {
        validate_fields(intern_fields(&self.fields), form, false)
    }
}

/// The checks of [`RuntimeResource::validate`], on the interned fields, so the error keys are
/// the `&'static str` that [`FieldErrors`] holds.
fn validate_fields(fields: &[Field], form: &FormInput, creating: bool) -> Result<Row, FieldErrors> {
    let mut errors = FieldErrors::new();
    let mut row = Row::new();
    for field in fields {
        // A checkbox is never blank: an unticked checkbox is false.
        if field.kind == FieldKind::Checkbox {
            row.insert(
                field.name.to_string(),
                Value::Bool(form.checkbox(field.name)),
            );
            continue;
        }
        let is_password = field.kind == FieldKind::Password;
        // A password is required only when creating. On an edit a blank one keeps the hash.
        let present = if field.required && (creating || !is_password) {
            field.check_required(form, &mut errors)
        } else {
            form.get(field.name)
        };
        let Some(text) = present else {
            if !is_password || creating {
                row.insert(field.name.to_string(), Value::Null);
            }
            continue;
        };
        let value = match field.kind {
            FieldKind::Password => match crate::hash_password(text) {
                Ok(hash) => Some(Value::String(hash)),
                Err(_) => {
                    errors.insert(field.name, "Could not save this password.".to_string());
                    None
                }
            },
            FieldKind::Number => field
                .check_whole_number::<i64>(text, &mut errors)
                .map(Value::from),
            FieldKind::Decimal(_) => field
                .check_decimal::<f64>(text, &mut errors)
                .map(Value::from),
            FieldKind::Select(choices) => {
                let text = text.trim();
                field
                    .check_choice(text, choices, &mut errors)
                    .then(|| Value::String(text.to_string()))
            }
            FieldKind::Lookup => Some(Value::String(text.trim().to_string())),
            FieldKind::Date => field
                .check_date(text, &mut errors)
                .map(|date| Value::String(date.format("%Y-%m-%d").to_string())),
            FieldKind::DateTime => field
                .check_datetime(text, &mut errors)
                .map(|stamp| Value::String(stamp.format("%Y-%m-%d %H:%M:%S").to_string())),
            FieldKind::Text | FieldKind::Textarea | FieldKind::Checkbox => {
                Some(Value::String(text.to_string()))
            }
        };
        if let Some(value) = value {
            row.insert(field.name.to_string(), value);
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

/// A runtime resource as the registry holds it: the definition, and the same values as the
/// `&'static` types the panel renders static resources with.
pub struct RegisteredResource {
    resource: RuntimeResource,
    slug: &'static str,
    title: &'static str,
    singular: &'static str,
    fields: &'static [Field],
    columns: &'static [Column],
}

impl RegisteredResource {
    /// The definition that was registered.
    pub fn resource(&self) -> &RuntimeResource {
        &self.resource
    }

    /// The slug, as the panel's routes match it.
    pub fn slug(&self) -> &'static str {
        self.slug
    }

    /// The heading and navigation label.
    pub fn title(&self) -> &'static str {
        self.title
    }

    /// The name of one record.
    pub fn singular(&self) -> &'static str {
        self.singular
    }

    /// The form fields, as [`Field`]s.
    pub fn fields(&self) -> &'static [Field] {
        self.fields
    }

    /// The list columns, as [`Column`]s.
    pub fn columns(&self) -> &'static [Column] {
        self.columns
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
/// The panel registers its permissions (`<slug>.viewAny` and the others) on that request.
///
/// Refuses a slug that is not valid, a slug that a static resource, another runtime resource
/// or one of the panel's own pages (such as `login`) uses, and two fields with one name.
pub fn register_runtime(resource: RuntimeResource) -> Result<(), RuntimeError> {
    if !valid_slug(&resource.slug) {
        return Err(RuntimeError::InvalidSlug(resource.slug));
    }
    let mut names = HashSet::new();
    if let Some(field) = resource.fields.iter().find(|f| !names.insert(&f.name)) {
        return Err(RuntimeError::DuplicateField {
            slug: resource.slug.clone(),
            field: field.name.clone(),
        });
    }
    // Interning takes locks of its own, so it runs before the registry is locked.
    let registered = RegisteredResource {
        slug: intern_str(&resource.slug),
        title: intern_str(&resource.title),
        singular: intern_str(&resource.singular),
        fields: intern_fields(&resource.fields),
        columns: intern_columns(&resource.columns),
        resource,
    };
    write(|registry| {
        let slug = registered.slug;
        if registry.reserved.contains(slug) || registry.resources.iter().any(|r| r.slug == slug) {
            return Err(RuntimeError::SlugTaken(slug.to_string()));
        }
        registry.resources.push(Arc::new(registered));
        Ok(())
    })
}

/// Removes the runtime resource at `slug`. From the next request on, it is gone from the
/// navigation and its routes answer 404. Its permission rows stay, so the grants come back
/// if the resource is registered again.
pub fn unregister_runtime(slug: &str) -> Result<(), RuntimeError> {
    write(|registry| {
        let before = registry.resources.len();
        registry.resources.retain(|r| r.slug != slug);
        if registry.resources.len() == before {
            Err(RuntimeError::NotRegistered(slug.to_string()))
        } else {
            Ok(())
        }
    })
}

/// The runtime resource at `slug`, if one is registered.
pub fn runtime_resource(slug: &str) -> Option<Arc<RegisteredResource>> {
    read(|registry| registry.resources.iter().find(|r| r.slug == slug).cloned())
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
            .find(|slug| registry.resources.iter().any(|r| r.slug == **slug))
        {
            return Err(RuntimeError::SlugTaken(slug.to_string()));
        }
        registry
            .reserved
            .extend(slugs.iter().map(|slug| slug.to_string()));
        Ok(())
    })
}

// The panel renders and validates with `&'static` fields, columns and error keys. A runtime
// definition is turned into those once, by leaking it, and interned: the same text or the
// same definition is leaked only once in the life of the process, so registering a resource
// again does not grow memory. Only definitions that change add to it.

static STRINGS: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
static CHOICES: Interner<Vec<(String, String)>, Choices> = Mutex::new(None);
static FIELDS: Interner<Vec<FieldDef>, [Field]> = Mutex::new(None);
static COLUMNS: Interner<Vec<ColumnDef>, [Column]> = Mutex::new(None);

type Interner<K, V> = Mutex<Option<HashMap<K, &'static V>>>;
/// The choices of a select field, as `FieldKind::Select` holds them.
type Choices = [(&'static str, &'static str)];

/// `text`, as a `&'static str` leaked at most once per distinct text.
fn intern_str(text: &str) -> &'static str {
    let mut guard = STRINGS.lock().unwrap_or_else(PoisonError::into_inner);
    let set = guard.get_or_insert_with(HashSet::new);
    if let Some(found) = set.get(text) {
        return found;
    }
    let leaked: &'static str = Box::leak(text.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// The interned value of `key`, built by `build` the first time. `build` runs without the
/// lock held, as it interns strings itself.
fn intern<K: Hash + Eq + Clone, V: ?Sized>(
    table: &'static Interner<K, V>,
    key: &K,
    build: impl FnOnce() -> Box<V>,
) -> &'static V {
    let lookup = |table: &'static Interner<K, V>| {
        table
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|map| map.get(key).copied())
    };
    if let Some(found) = lookup(table) {
        return found;
    }
    let built = build();
    let mut guard = table.lock().unwrap_or_else(PoisonError::into_inner);
    let map = guard.get_or_insert_with(HashMap::new);
    map.entry(key.clone()).or_insert_with(|| Box::leak(built))
}

fn intern_choices(choices: &Vec<(String, String)>) -> &'static [(&'static str, &'static str)] {
    intern(&CHOICES, choices, || {
        choices
            .iter()
            .map(|(value, label)| (intern_str(value), intern_str(label)))
            .collect()
    })
}

/// The fields of a runtime definition as [`Field`]s.
fn intern_fields(fields: &Vec<FieldDef>) -> &'static [Field] {
    intern(&FIELDS, fields, || {
        fields
            .iter()
            .map(|def| Field {
                name: intern_str(&def.name),
                label: intern_str(&def.label),
                kind: match &def.kind {
                    FieldKindDef::Text => FieldKind::Text,
                    FieldKindDef::Textarea => FieldKind::Textarea,
                    FieldKindDef::Password => FieldKind::Password,
                    FieldKindDef::Number => FieldKind::Number,
                    FieldKindDef::Decimal(step) => FieldKind::Decimal(intern_str(step)),
                    FieldKindDef::Checkbox => FieldKind::Checkbox,
                    FieldKindDef::Select(choices) => FieldKind::Select(intern_choices(choices)),
                    FieldKindDef::Lookup => FieldKind::Lookup,
                    FieldKindDef::Date => FieldKind::Date,
                    FieldKindDef::DateTime => FieldKind::DateTime,
                },
                required: def.required,
            })
            .collect()
    })
}

/// The columns of a runtime definition as [`Column`]s.
fn intern_columns(columns: &Vec<ColumnDef>) -> &'static [Column] {
    intern(&COLUMNS, columns, || {
        columns
            .iter()
            .map(|def| Column {
                key: intern_str(&def.key),
                label: intern_str(&def.label),
                sortable: def.sortable,
                searchable: def.searchable,
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_definition_is_interned_once() {
        let defs = vec![FieldDef {
            name: "intern_test".into(),
            label: "Intern".into(),
            kind: FieldKindDef::Select(vec![("a".into(), "A".into())]),
            required: false,
        }];
        let first = intern_fields(&defs);
        let second = intern_fields(&defs.clone());
        assert!(std::ptr::eq(first, second));
        assert!(std::ptr::eq(intern_str("intern_test"), first[0].name));
    }
}
