//! Dashboard widgets: the stat cards and the latest-records tables a resource
//! can show on the dashboard, and the panel's own widgets.
//!
//! A [`Stat`] is one card, as Filament's `Stat::make($label, $value)` in a stats overview
//! widget: its label is the card's title, its value the number shown, and an optional
//! description the line under it. A resource gives its cards with
//! [`IgniteResource::stats`](crate::IgniteResource::stats) (or a runtime resource's
//! [`RuntimeData::stats`](crate::RuntimeData::stats)); the panel gives cards of its own with a
//! [`StatsOverviewWidget`] passed to `Ignite::widget`, as Filament's panel `widgets()`. The
//! dashboard shows every card in a grid: the panel's widgets first, in the order they were
//! added, then each resource's, in the order of the navigation's resources.

use std::fmt;
use std::sync::Arc;

use orbit_orm::{Database, Error};

use crate::policy::ActingUser;
use crate::query::ListQuery;
use crate::runtime_resource::{Row, RuntimeData, RuntimeFuture};

/// One stat card on the dashboard: a label and a whole-number value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StatWidget {
    /// The card's heading, such as "Users".
    pub label: &'static str,
    /// The number the card shows.
    pub value: i64,
}

/// A dashboard table of the newest records of a resource (added in v1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestRecords {
    /// The table's heading, such as "Latest users".
    pub title: &'static str,
    /// The keys of the resource's columns to show, in order. Each key must be a column key.
    pub columns: &'static [&'static str],
    /// The most records the table shows, newest first.
    pub limit: i64,
}

/// One card of the dashboard, as Filament's `Stat::make($label, $value)`: a title (the label),
/// the number shown and an optional description under it, such as "3 drafts".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    label: String,
    value: String,
    description: Option<String>,
}

impl Stat {
    /// A card titled `label` showing `value`, such as `Stat::make("Posts", 12)`. The value is
    /// shown as its text, so it may be formatted already, such as `"1.2k"`.
    pub fn make(label: impl Into<String>, value: impl ToString) -> Self {
        Stat {
            label: label.into(),
            value: value.to_string(),
            description: None,
        }
    }

    /// The line under the value, as Filament's `description()`.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The card's title.
    pub fn get_label(&self) -> &str {
        &self.label
    }

    /// The value, as shown.
    pub fn get_value(&self) -> &str {
        &self.value
    }

    /// The description, if set.
    pub fn get_description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

impl From<StatWidget> for Stat {
    fn from(widget: StatWidget) -> Self {
        Stat::make(widget.label, widget.value)
    }
}

/// A widget of the panel's own on the dashboard, as Filament's `StatsOverviewWidget` with its
/// `getStats()`: the cards it computes for the signed-in user. Pass it to `Ignite::widget`.
///
/// ```ignore
/// struct SiteStats;
///
/// impl StatsOverviewWidget for SiteStats {
///     fn stats<'a>(&'a self, db: &'a Database, _: &'a ActingUser) -> RuntimeFuture<'a, Vec<Stat>> {
///         Box::pin(async move {
///             let comments = count_pending_comments(db).await?;
///             Ok(vec![Stat::make("Comments", comments).description("awaiting moderation")])
///         })
///     }
/// }
/// ```
pub trait StatsOverviewWidget: Send + Sync + 'static {
    /// The cards, in display order, for `user`.
    fn stats<'a>(&'a self, db: &'a Database, user: &'a ActingUser) -> RuntimeFuture<'a, Vec<Stat>>;

    /// Whether `user` sees the widget, as Filament's `canView()`. The default is every user.
    fn can_view(&self, user: &ActingUser) -> bool {
        let _ = user;
        true
    }
}

/// One row of a [`ListWidget`]: a label, an optional value beside it and an optional link,
/// such as a post's title linking to its edit form and its date as the value, as the rows of
/// WordPress's "Activity" and "Recently Published" boxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListWidgetRow {
    label: String,
    value: Option<String>,
    url: Option<String>,
}

impl ListWidgetRow {
    /// A row showing `label`.
    pub fn make(label: impl Into<String>) -> Self {
        ListWidgetRow {
            label: label.into(),
            value: None,
            url: None,
        }
    }

    /// The text beside the label, such as a date or a count.
    pub fn value(mut self, value: impl ToString) -> Self {
        self.value = Some(value.to_string());
        self
    }

    /// The address the label links to, such as `/admin/posts/7/edit`. Only a path (starting
    /// with `/`, not `//`) or an `http://` or `https://` URL is linked; anything else, such as
    /// a `javascript:` URL, shows the label without a link.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// The label.
    pub fn get_label(&self) -> &str {
        &self.label
    }

    /// The value, if set.
    pub fn get_value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// The link, if set and safe to link (see [`url`](Self::url)).
    pub fn get_url(&self) -> Option<&str> {
        self.url.as_deref().filter(|url| safe_url(url))
    }
}

/// Whether `url` is a path of this site or an `http(s)` URL, so a link cannot run script.
pub fn safe_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    !url.chars().any(char::is_control)
        && ((lower.starts_with('/') && !lower.starts_with("//") && !lower.starts_with("/\\"))
            || lower.starts_with("https://")
            || lower.starts_with("http://"))
}

type RowsFn = Arc<
    dyn for<'a> Fn(&'a Database, &'a ActingUser) -> RuntimeFuture<'a, Vec<ListWidgetRow>>
        + Send
        + Sync,
>;
type ViewRule = Arc<dyn Fn(&ActingUser) -> bool + Send + Sync>;

/// A dashboard widget that lists rows under a title, as a Filament custom widget with its own
/// view: the rows of WordPress's "Activity" or "Recently Published" box. Pass it to
/// `Ignite::list_widget`. The panel renders it with its `partials.widgets.list` view (a theme
/// can replace it), escaping every label, value and link.
///
/// ```ignore
/// ListWidget::make("Recently Published")
///     .rows_from(posts.data.clone(), ListQuery { page: 1, per_page: 5, search: None,
///         sort: Some(("published_at".into(), Direction::Desc)),
///         filter: ListFilter::new().where_eq("status", "publish") },
///         |row| ListWidgetRow::make(row["title"].as_str().unwrap_or_default())
///             .value(row["published_at"].as_str().unwrap_or_default())
///             .url(format!("/admin/posts/{}/edit", row["id"])))
///     .empty_state("No posts published yet.")
/// ```
#[derive(Clone)]
pub struct ListWidget {
    title: String,
    rows: Option<RowsFn>,
    empty_state: String,
    can_view: Option<ViewRule>,
}

impl fmt::Debug for ListWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListWidget")
            .field("title", &self.title)
            .field("rows", &self.rows.is_some())
            .field("empty_state", &self.empty_state)
            .field("can_view", &self.can_view.is_some())
            .finish()
    }
}

/// What a [`ListWidget`] shows when it has no rows, unless it sets its own.
pub const DEFAULT_EMPTY_STATE: &str = "Nothing to show yet.";

impl ListWidget {
    /// A widget titled `title`, with no rows yet.
    pub fn make(title: impl Into<String>) -> Self {
        ListWidget {
            title: title.into(),
            rows: None,
            empty_state: DEFAULT_EMPTY_STATE.to_string(),
            can_view: None,
        }
    }

    /// The rows, computed for the signed-in user on each dashboard request. Write it as
    /// `|db, user| Box::pin(async move { ... })`.
    pub fn rows<F>(mut self, rows: F) -> Self
    where
        F: for<'a> Fn(&'a Database, &'a ActingUser) -> RuntimeFuture<'a, Vec<ListWidgetRow>>
            + Send
            + Sync
            + 'static,
    {
        self.rows = Some(Arc::new(rows));
        self
    }

    /// The rows from a [`RuntimeData`]: its [`list_for`](RuntimeData::list_for) for the
    /// signed-in user with `query` (its page, sort and filter), each row turned into a
    /// [`ListWidgetRow`] by `map`. A runtime resource's data is its `data` field.
    pub fn rows_from<M>(self, data: Arc<dyn RuntimeData>, query: ListQuery, map: M) -> Self
    where
        M: Fn(&Row) -> ListWidgetRow + Send + Sync + 'static,
    {
        let map = Arc::new(map);
        self.rows(move |db, user| {
            let data = Arc::clone(&data);
            let query = query.clone();
            let map = Arc::clone(&map);
            Box::pin(async move {
                let page = data.list_for(db, user, &query).await?;
                Ok(page.rows.iter().map(|row| map(row)).collect())
            })
        })
    }

    /// The text shown when there are no rows. The default is [`DEFAULT_EMPTY_STATE`].
    pub fn empty_state(mut self, text: impl Into<String>) -> Self {
        self.empty_state = text.into();
        self
    }

    /// Who sees the widget, as Filament's `canView()`. The default is every user.
    pub fn can_view(mut self, rule: impl Fn(&ActingUser) -> bool + Send + Sync + 'static) -> Self {
        self.can_view = Some(Arc::new(rule));
        self
    }

    /// The title.
    pub fn get_title(&self) -> &str {
        &self.title
    }

    /// The text shown without rows.
    pub fn get_empty_state(&self) -> &str {
        &self.empty_state
    }

    /// Whether `user` sees the widget.
    pub fn is_visible_to(&self, user: &ActingUser) -> bool {
        self.can_view.as_ref().is_none_or(|rule| rule(user))
    }

    /// The rows for `user`. None when the widget sets none.
    pub async fn load(
        &self,
        db: &Database,
        user: &ActingUser,
    ) -> Result<Vec<ListWidgetRow>, Error> {
        match &self.rows {
            Some(rows) => rows(db, user).await,
            None => Ok(Vec::new()),
        }
    }
}

/// A dashboard widget with a short form that creates a record of a resource, as a Filament
/// custom widget with a form: WordPress's "Quick Draft" (a title, the content and "Save
/// Draft"). Pass it to `Ignite::form_widget`. The form shows the resource's fields (all of
/// them, or the ones [`fields`](Self::fields) names, in that order) and posts to the
/// resource's create route with the CSRF token, so the resource's validation, policy and
/// `create_for` run as for its own form; the panel then comes back to the dashboard with the
/// flash message. A form that does not validate shows the resource's create form with its
/// errors. The widget shows only to a user who may create a record of the resource.
///
/// ```ignore
/// FormWidget::make("Quick Draft", "posts")
///     .fields(["title", "content"])
///     .hidden("status", "draft")
///     .submit_label("Save Draft")
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormWidget {
    title: String,
    resource: String,
    fields: Vec<String>,
    hidden: Vec<(String, String)>,
    submit_label: String,
}

impl FormWidget {
    /// A widget titled `title` whose form creates a record of the resource at `slug`.
    pub fn make(title: impl Into<String>, slug: impl Into<String>) -> Self {
        FormWidget {
            title: title.into(),
            resource: slug.into(),
            fields: Vec::new(),
            hidden: Vec::new(),
            submit_label: "Save".to_string(),
        }
    }

    /// The resource's fields the form shows, in order. The default is every field.
    pub fn fields<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fields = names.into_iter().map(Into::into).collect();
        self
    }

    /// A value the form sends without showing it, such as `status` = `draft`. The resource's
    /// validation reads it as any other field's value.
    pub fn hidden(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.hidden.push((name.into(), value.into()));
        self
    }

    /// The button's text. The default is "Save".
    pub fn submit_label(mut self, label: impl Into<String>) -> Self {
        self.submit_label = label.into();
        self
    }

    /// The title.
    pub fn get_title(&self) -> &str {
        &self.title
    }

    /// The slug of the resource the form creates a record of.
    pub fn get_resource(&self) -> &str {
        &self.resource
    }

    /// The names of the fields the form shows; empty for every field.
    pub fn get_fields(&self) -> &[String] {
        &self.fields
    }

    /// The values the form sends without showing them.
    pub fn get_hidden(&self) -> &[(String, String)] {
        &self.hidden
    }

    /// The button's text.
    pub fn get_submit_label(&self) -> &str {
        &self.submit_label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_and_web_urls_are_linked() {
        assert!(safe_url("/admin/posts/1/edit"));
        assert!(safe_url("https://example.test/a"));
        assert!(safe_url("HTTP://example.test"));
        assert!(!safe_url("javascript:alert(1)"));
        assert!(!safe_url(" JavaScript:alert(1)"));
        assert!(!safe_url("//evil.test"));
        assert!(!safe_url("/\\evil.test"));
        assert!(!safe_url("data:text/html,x"));
        assert!(!safe_url("/a\nb"));
        assert_eq!(ListWidgetRow::make("x").url("javascript:x").get_url(), None);
    }
}
