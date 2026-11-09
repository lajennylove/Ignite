//! Dashboard widgets: the stat cards a resource can show on the dashboard.

/// One stat card on the dashboard: a label and a whole-number value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StatWidget {
    /// The card's heading, such as "Users".
    pub label: &'static str,
    /// The number the card shows.
    pub value: i64,
}
