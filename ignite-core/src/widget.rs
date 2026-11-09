//! Dashboard widgets: the stat cards and the latest-records tables a resource
//! can show on the dashboard.

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
