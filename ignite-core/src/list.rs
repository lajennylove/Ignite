//! The list page's tabs and column badges, as Filament's `getTabs()` on a List page and
//! `badge()` with `color()` on a text column.
//!
//! A [`Tab`] has a key (the value of the list's `tab` query parameter, as Filament's
//! `#[Url(as: 'tab')]`), a label and a [`ListFilter`]: conditions on the resource's rows, as
//! Filament's `modifyQueryUsing()`. The panel gives the active tab's filter to the resource's
//! list query in [`ListQuery::filter`](crate::ListQuery::filter), beside the search and sort,
//! and shows the number of rows each tab matches beside its label, for the current search.
//!
//! ```ignore
//! RuntimeResource::new("posts", "Posts", "Post", PostData)
//!     .tab(Tab::make("all"))
//!     .tab(Tab::make("drafts").where_eq("status", "draft"))
//!     .badge_color("status", |state| match state {
//!         "draft" => BadgeColor::Gray,
//!         "publish" => BadgeColor::Success,
//!         _ => BadgeColor::Primary,
//!     });
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::runtime_resource::Row;

/// One condition of a [`ListFilter`] on a column of the resource's rows. Each value is JSON,
/// as a runtime resource's [`Row`] holds it: a string, a number, a boolean or `null`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterCondition {
    /// `column = value`, as Laravel's `where($column, $value)`. A `null` value is `IS NULL`.
    Eq(String, Value),
    /// `column != value`, as Laravel's `where($column, '!=', $value)`. A row whose column is
    /// NULL does not match, as in SQL. A `null` value is `IS NOT NULL`.
    Ne(String, Value),
    /// `column IN (values)`, as Laravel's `whereIn()`. An empty list matches no rows.
    In(String, Vec<Value>),
    /// `column IS NULL`, as Laravel's `whereNull()`.
    Null(String),
    /// `column IS NOT NULL`, as Laravel's `whereNotNull()`.
    NotNull(String),
}

/// Conditions on a resource's rows, all of which a row must meet: the query of a [`Tab`], as
/// Filament's `modifyQueryUsing()`. An empty filter matches every row.
///
/// `#[derive(IgniteResource)]` applies it in its list query. A resource that writes its own
/// `list` (or a [`RuntimeData`](crate::RuntimeData)) applies it with [`apply`](Self::apply) on
/// an Orbit query, or [`matches`](Self::matches) on a row, or reads its
/// [`conditions`](Self::conditions).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListFilter {
    conditions: Vec<FilterCondition>,
}

impl ListFilter {
    /// A filter with no conditions: it matches every row.
    pub fn new() -> Self {
        ListFilter::default()
    }

    /// Adds `column = value`.
    pub fn where_eq(mut self, column: impl Into<String>, value: impl Into<Value>) -> Self {
        self.conditions
            .push(FilterCondition::Eq(column.into(), value.into()));
        self
    }

    /// Adds `column != value`.
    pub fn where_ne(mut self, column: impl Into<String>, value: impl Into<Value>) -> Self {
        self.conditions
            .push(FilterCondition::Ne(column.into(), value.into()));
        self
    }

    /// Adds `column IN (values)`.
    pub fn where_in<V: Into<Value>>(
        mut self,
        column: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        self.conditions.push(FilterCondition::In(
            column.into(),
            values.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Adds `column IS NULL`.
    pub fn where_null(mut self, column: impl Into<String>) -> Self {
        self.conditions.push(FilterCondition::Null(column.into()));
        self
    }

    /// Adds `column IS NOT NULL`.
    pub fn where_not_null(mut self, column: impl Into<String>) -> Self {
        self.conditions
            .push(FilterCondition::NotNull(column.into()));
        self
    }

    /// Whether the filter has no conditions.
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }

    /// The conditions, in the order they were added.
    pub fn conditions(&self) -> &[FilterCondition] {
        &self.conditions
    }

    /// Adds the conditions to an Orbit query, joined with `AND`.
    pub fn apply<M: orbit_orm::Model>(&self, query: orbit_orm::Query<M>) -> orbit_orm::Query<M> {
        self.conditions
            .iter()
            .fold(query, |query, condition| match condition {
                FilterCondition::Eq(column, value) => query.where_eq(column, orbit_value(value)),
                FilterCondition::Ne(column, Value::Null) => query.where_not_null(column),
                FilterCondition::Ne(column, value) => {
                    query.where_op(column, orbit_orm::Operator::Ne, orbit_value(value))
                }
                FilterCondition::In(column, values) => {
                    query.where_in(column, values.iter().map(orbit_value))
                }
                FilterCondition::Null(column) => query.where_null(column),
                FilterCondition::NotNull(column) => query.where_not_null(column),
            })
    }

    /// Whether `row` meets every condition, as the database would answer: a missing key is
    /// NULL, and a NULL column is neither equal nor unequal to a value.
    pub fn matches(&self, row: &Row) -> bool {
        self.conditions.iter().all(|condition| {
            let get = |column: &str| row.get(column).filter(|v| !v.is_null());
            match condition {
                FilterCondition::Eq(column, Value::Null) | FilterCondition::Null(column) => {
                    get(column).is_none()
                }
                FilterCondition::Ne(column, Value::Null) | FilterCondition::NotNull(column) => {
                    get(column).is_some()
                }
                FilterCondition::Eq(column, value) => get(column) == Some(value),
                FilterCondition::Ne(column, value) => get(column).is_some_and(|v| v != value),
                FilterCondition::In(column, values) => {
                    get(column).is_some_and(|v| values.contains(v))
                }
            }
        })
    }
}

/// A JSON value as an Orbit value: arrays and objects as their JSON text.
fn orbit_value(value: &Value) -> orbit_orm::Value {
    match value {
        Value::Null => orbit_orm::Value::Null,
        Value::Bool(b) => orbit_orm::Value::Bool(*b),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => orbit_orm::Value::Int(i),
            (None, Some(u), _) => orbit_orm::Value::UInt(u),
            (None, None, Some(f)) => orbit_orm::Value::Float(f),
            _ => orbit_orm::Value::Text(n.to_string()),
        },
        Value::String(s) => orbit_orm::Value::Text(s.clone()),
        other => orbit_orm::Value::Text(other.to_string()),
    }
}

/// One tab of a resource's list page, as Filament's `Tab::make()` in `getTabs()`: a key, a
/// label and the conditions the tab's rows meet. The panel shows the number of rows the tab
/// matches beside its label, as in "Drafts (3)", counted for the current search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// The tab's key: the value of the list's `tab` query parameter, as Filament's array key
    /// in `getTabs()`.
    pub key: String,
    /// The label. [`make`](Self::make) generates it from the key, as Filament does.
    pub label: String,
    /// The conditions the tab's rows meet. An empty filter, as for an "All" tab, matches
    /// every row.
    pub filter: ListFilter,
}

impl Tab {
    /// A tab with the key `key`, no conditions, and the label Filament generates from the key:
    /// `_` and `-` become spaces and the first letter is upper case, so `"needs_review"` is
    /// "Needs review".
    pub fn make(key: impl Into<String>) -> Self {
        let key = key.into();
        let label = generate_label(&key);
        Tab {
            key,
            label,
            filter: ListFilter::new(),
        }
    }

    /// Sets the label, as `Tab::make('Label')`.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Sets the tab's conditions, as Filament's `modifyQueryUsing()`.
    pub fn filter(mut self, filter: ListFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Adds `column = value` to the tab's conditions, as
    /// `modifyQueryUsing(fn ($query) => $query->where($column, $value))`.
    pub fn where_eq(mut self, column: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter = self.filter.where_eq(column, value);
        self
    }

    /// Adds `column != value` to the tab's conditions.
    pub fn where_ne(mut self, column: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filter = self.filter.where_ne(column, value);
        self
    }

    /// Adds `column IN (values)` to the tab's conditions.
    pub fn where_in<V: Into<Value>>(
        mut self,
        column: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        self.filter = self.filter.where_in(column, values);
        self
    }

    /// Adds `column IS NULL` to the tab's conditions.
    pub fn where_null(mut self, column: impl Into<String>) -> Self {
        self.filter = self.filter.where_null(column);
        self
    }

    /// Adds `column IS NOT NULL` to the tab's conditions.
    pub fn where_not_null(mut self, column: impl Into<String>) -> Self {
        self.filter = self.filter.where_not_null(column);
        self
    }
}

/// Filament's `generateTabLabel()`: `_` and `-` become spaces, and the first letter is upper
/// case.
fn generate_label(key: &str) -> String {
    let spaced = key.replace(['_', '-'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The tab a list request shows, as Filament's `$activeTab`: the `requested` key when it is
/// one of `tabs`; with no key, the `default` key when it is one of `tabs`, or the first tab,
/// as Filament's `getDefaultActiveTab()`. A requested key that is not a tab shows no tab:
/// the list is not filtered, as Filament ignores an unknown `activeTab`.
pub fn active_tab<'a>(
    tabs: &'a [Tab],
    requested: Option<&str>,
    default: Option<&str>,
) -> Option<&'a Tab> {
    let find = |key: &str| tabs.iter().find(|tab| tab.key == key);
    match requested.filter(|key| !key.is_empty()) {
        Some(key) => find(key),
        None => default.and_then(find).or_else(|| tabs.first()),
    }
}

/// The colour of a badge, by the names Filament gives its colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BadgeColor {
    /// A neutral grey, as for a draft.
    Gray,
    /// Information, in blue.
    Info,
    /// Success, in green.
    Success,
    /// A warning, in amber.
    Warning,
    /// Danger, in red.
    Danger,
    /// The panel's primary colour: a badge's default, as in Filament.
    Primary,
}

impl BadgeColor {
    /// The colour's name, as Filament spells it: `"gray"`, `"info"`, `"success"`,
    /// `"warning"`, `"danger"` or `"primary"`.
    pub fn as_str(self) -> &'static str {
        match self {
            BadgeColor::Gray => "gray",
            BadgeColor::Info => "info",
            BadgeColor::Success => "success",
            BadgeColor::Warning => "warning",
            BadgeColor::Danger => "danger",
            BadgeColor::Primary => "primary",
        }
    }
}

/// How a badge column picks its colour from the cell's state.
type ColorRule = Arc<dyn Fn(&str) -> BadgeColor + Send + Sync>;

/// One badge column: its colour rule, the column whose cell shows it, and the tabs it shows on.
#[derive(Clone, Default)]
struct Badge {
    color: Option<ColorRule>,
    inside: Option<String>,
    tabs: Option<Vec<String>>,
}

/// The list columns that show their value as a coloured badge, by column key, as Filament's
/// `TextColumn::make($key)->badge()->color(fn (string $state) => ...)`. A column that is not
/// listed shows its value as plain text.
///
/// A badge can also show inside another column's cell, after its text, instead of in a column
/// of its own ([`badge_inside`](Self::badge_inside)), such as a post's status beside its
/// title; and only on some tabs of the list ([`badge_on_tabs`](Self::badge_on_tabs)). The
/// panel decides both when it renders the page, so the page needs no script for them.
#[derive(Clone, Default)]
pub struct ColumnBadges {
    columns: BTreeMap<String, Badge>,
}

impl fmt::Debug for ColumnBadges {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ColumnBadges")
            .field("columns", &self.columns.keys().collect::<BTreeSet<_>>())
            .finish()
    }
}

impl ColumnBadges {
    /// No badge columns.
    pub fn new() -> Self {
        ColumnBadges::default()
    }

    /// Shows the column `column` as a badge in the primary colour, as Filament's `->badge()`
    /// with no `color()`.
    pub fn badge(mut self, column: impl Into<String>) -> Self {
        self.columns.entry(column.into()).or_default().color = None;
        self
    }

    /// Shows the column `column` as a badge whose colour `color` picks from the cell's state,
    /// as Filament's `->badge()->color(fn (string $state): string => match ($state) { ... })`.
    /// The state is the stored value as text, such as `"draft"`, before a select's or a
    /// lookup's label replaces it for display.
    pub fn badge_color(
        mut self,
        column: impl Into<String>,
        color: impl Fn(&str) -> BadgeColor + Send + Sync + 'static,
    ) -> Self {
        self.columns.entry(column.into()).or_default().color = Some(Arc::new(color));
        self
    }

    /// Shows the badge of the column `column` inside the cell of the column `target`, after
    /// its text, and not as a column of its own: the list has no column for `column`, even
    /// when the resource lists it. `column` need not be a list column: its value is read from
    /// the row. It is a badge in the primary colour unless [`badge_color`](Self::badge_color)
    /// gives it a colour.
    pub fn badge_inside(mut self, column: impl Into<String>, target: impl Into<String>) -> Self {
        self.columns.entry(column.into()).or_default().inside = Some(target.into());
        self
    }

    /// Shows the badge of the column `column` only on the list's tabs `tabs`, by key, as a
    /// Filament column's `visible()` that reads the active tab. On another tab, or on a list
    /// with no tab active, the badge is not rendered: a badge column is left out of the table,
    /// heading and all, and a badge inside another cell is left out of the cell.
    pub fn badge_on_tabs<S: Into<String>>(
        mut self,
        column: impl Into<String>,
        tabs: impl IntoIterator<Item = S>,
    ) -> Self {
        self.columns.entry(column.into()).or_default().tabs =
            Some(tabs.into_iter().map(Into::into).collect());
        self
    }

    /// Whether no column is a badge.
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// Whether the column `column` is a badge.
    pub fn is_badge(&self, column: &str) -> bool {
        self.columns.contains_key(column)
    }

    /// The colour of the badge of the column `column` for `state`, or `None` when the column
    /// is not a badge.
    pub fn color(&self, column: &str, state: &str) -> Option<BadgeColor> {
        self.columns.get(column).map(|badge| {
            badge
                .color
                .as_ref()
                .map_or(BadgeColor::Primary, |rule| rule(state))
        })
    }

    /// The column whose cell shows the badge of `column`, if it shows inside another cell.
    pub fn inside_of(&self, column: &str) -> Option<&str> {
        self.columns.get(column)?.inside.as_deref()
    }

    /// The badge columns shown inside the cell of `target`, in key order.
    pub fn inside(&self, target: &str) -> Vec<&str> {
        self.columns
            .iter()
            .filter(|(_, badge)| badge.inside.as_deref() == Some(target))
            .map(|(column, _)| column.as_str())
            .collect()
    }

    /// Whether the badge of `column` shows when the tab `tab` is active (`None` for none): a
    /// badge without [`badge_on_tabs`](Self::badge_on_tabs) shows on every tab. A column that
    /// is not a badge shows.
    pub fn shows_on(&self, column: &str, tab: Option<&str>) -> bool {
        match self
            .columns
            .get(column)
            .and_then(|badge| badge.tabs.as_ref())
        {
            None => true,
            Some(tabs) => tab.is_some_and(|tab| tabs.iter().any(|key| key == tab)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(value: Value) -> Row {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn a_tab_label_is_generated_from_its_key_as_filament_does() {
        assert_eq!(Tab::make("needs_review").label, "Needs review");
        assert_eq!(Tab::make("all-posts").label, "All posts");
        assert_eq!(Tab::make("drafts").label("My drafts").label, "My drafts");
    }

    #[test]
    fn the_active_tab_is_the_requested_one_or_the_default_or_the_first() {
        let tabs = [Tab::make("all"), Tab::make("drafts")];
        assert_eq!(
            active_tab(&tabs, Some("drafts"), None).unwrap().key,
            "drafts"
        );
        assert_eq!(active_tab(&tabs, None, None).unwrap().key, "all");
        assert_eq!(
            active_tab(&tabs, Some(""), Some("drafts")).unwrap().key,
            "drafts"
        );
        assert_eq!(active_tab(&tabs, None, Some("missing")).unwrap().key, "all");
        assert!(active_tab(&tabs, Some("missing"), None).is_none());
        assert!(active_tab(&[], None, None).is_none());
    }

    #[test]
    fn a_filter_matches_rows_as_the_database_would() {
        let draft = row(json!({"status": "draft", "author": null, "id": 3}));
        let published = row(json!({"status": "publish", "author": 1, "id": 4}));
        let drafts = ListFilter::new().where_eq("status", "draft");
        assert!(drafts.matches(&draft));
        assert!(!drafts.matches(&published));
        assert!(ListFilter::new().where_null("author").matches(&draft));
        assert!(ListFilter::new()
            .where_not_null("author")
            .matches(&published));
        assert!(!ListFilter::new().where_ne("author", 2).matches(&draft));
        assert!(ListFilter::new().where_ne("author", 2).matches(&published));
        assert!(ListFilter::new().where_in("id", [3, 5]).matches(&draft));
        assert!(!ListFilter::new()
            .where_in("id", Vec::<i64>::new())
            .matches(&draft));
        assert!(ListFilter::new().matches(&published));
    }

    #[test]
    fn a_badge_column_takes_its_colour_from_the_state() {
        let badges = ColumnBadges::new()
            .badge("kind")
            .badge_color("status", |state| match state {
                "draft" => BadgeColor::Gray,
                _ => BadgeColor::Success,
            });
        assert_eq!(badges.color("status", "draft"), Some(BadgeColor::Gray));
        assert_eq!(badges.color("status", "publish"), Some(BadgeColor::Success));
        assert_eq!(badges.color("kind", "x"), Some(BadgeColor::Primary));
        assert_eq!(badges.color("title", "x"), None);
        assert_eq!(BadgeColor::Danger.as_str(), "danger");
    }

    #[test]
    fn a_badge_shows_inside_another_cell_and_on_its_tabs() {
        let badges = ColumnBadges::new()
            .badge_color("status", |_| BadgeColor::Gray)
            .badge_inside("status", "title")
            .badge_on_tabs("status", ["all"])
            .badge("kind");
        assert_eq!(badges.inside_of("status"), Some("title"));
        assert_eq!(badges.inside("title"), ["status"]);
        assert_eq!(badges.inside_of("kind"), None);
        assert_eq!(badges.color("status", "x"), Some(BadgeColor::Gray));
        assert!(badges.shows_on("status", Some("all")));
        assert!(!badges.shows_on("status", Some("drafts")));
        assert!(!badges.shows_on("status", None));
        assert!(badges.shows_on("kind", None) && badges.shows_on("title", Some("drafts")));
        // `badge` after `badge_inside` keeps the place and resets the colour only.
        let badges = ColumnBadges::new()
            .badge_inside("status", "title")
            .badge("status");
        assert_eq!(badges.inside_of("status"), Some("title"));
    }
}
