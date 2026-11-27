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

use orbit_orm::Database;

use crate::policy::ActingUser;
use crate::runtime_resource::RuntimeFuture;

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
