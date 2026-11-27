//! The order of the sidebar, as Filament's navigation builds it: the entries without a group
//! first, then one section per navigation group under its heading. The groups come in the
//! order the panel declares them ([`Ignite::navigation_groups`](crate::Ignite::navigation_groups)),
//! and a group the panel does not declare comes after the declared ones, as in Filament. An
//! entry that names a parent item is listed under that parent, as Filament's
//! `$navigationParentItem`.

/// A navigation group as the panel declares it, as Filament's `NavigationGroup`: its label,
/// and whether its entries can be collapsed under its heading. Pass it to
/// [`Ignite::navigation_groups`](crate::Ignite::navigation_groups), beside or instead of a
/// plain label.
///
/// ```ignore
/// Ignite::new().navigation_groups([
///     NavigationGroup::make("Content"),
///     NavigationGroup::make("Settings").collapsed(true),
/// ])
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationGroup {
    label: String,
    collapsible: Option<bool>,
    collapsed: bool,
}

impl NavigationGroup {
    /// The group `label`: the name resources give as their navigation group, shown as its
    /// heading.
    pub fn make(label: impl Into<String>) -> Self {
        NavigationGroup {
            label: label.into(),
            collapsible: None,
            collapsed: false,
        }
    }

    /// Whether the group's heading collapses and expands its entries, as Filament's
    /// `collapsible()`. A group that does not say follows the panel's
    /// [`collapsible_navigation_groups`](crate::Ignite::collapsible_navigation_groups).
    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = Some(collapsible);
        self
    }

    /// Whether the group starts collapsed, as Filament's `collapsed()`, until the user expands
    /// it. As in Filament, it also sets [`collapsible`](Self::collapsible) to the same value.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self.collapsible = Some(collapsed);
        self
    }

    /// The group's label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether the group collapses, when the panel's default is `default`.
    pub(crate) fn is_collapsible(&self, default: bool) -> bool {
        self.collapsible.unwrap_or(default)
    }

    /// Whether the group starts collapsed, when the panel's default is `default`.
    pub(crate) fn is_collapsed(&self, default: bool) -> bool {
        self.collapsed && self.is_collapsible(default)
    }
}

impl From<&str> for NavigationGroup {
    fn from(label: &str) -> Self {
        NavigationGroup::make(label)
    }
}

impl From<String> for NavigationGroup {
    fn from(label: String) -> Self {
        NavigationGroup::make(label)
    }
}

impl From<&String> for NavigationGroup {
    fn from(label: &String) -> Self {
        NavigationGroup::make(label.as_str())
    }
}

impl From<std::borrow::Cow<'_, str>> for NavigationGroup {
    fn from(label: std::borrow::Cow<'_, str>) -> Self {
        NavigationGroup::make(label.into_owned())
    }
}

impl From<Box<str>> for NavigationGroup {
    fn from(label: Box<str>) -> Self {
        NavigationGroup::make(String::from(label))
    }
}

/// A navigation entry the panel declares itself, as Filament's `NavigationItem` in the panel's
/// `navigationItems()`: a label with its own group, sort and icon, and no resource of its own.
///
/// Its main use is a parent entry that is only a label over child entries, as Filament's
/// cluster: resources name it as their parent item by its label
/// ([`NAVIGATION_PARENT_ITEM`](ignite_core::IgniteResource::NAVIGATION_PARENT_ITEM) or a
/// runtime resource's `navigation_parent_item`), and clicking it opens its first child, as
/// clicking a Filament cluster's navigation item opens the first page of the cluster. As with
/// Filament's `$navigationParentItem`, the parent and its children must be in the same group.
/// An entry without a [`url`](Self::url) whose children the user may not see is not listed.
///
/// ```ignore
/// Ignite::new()
///     .navigation_groups(["Content", "Settings"])
///     .navigation_items([NavigationItem::make("Settings").group("Settings").icon("settings")])
/// // Each settings screen is a resource with
/// //     navigation_group = "Settings", navigation_parent_item = "Settings"
/// // and its own navigation_sort, so General (sort 1) is the one "Settings" opens.
/// ```
///
/// An entry with a [`url`](Self::url) is a plain link, as Filament's custom navigation items,
/// and is listed even without children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationItem {
    label: String,
    group: Option<String>,
    sort: Option<i32>,
    icon: Option<String>,
    url: Option<String>,
    ability: Option<String>,
}

impl NavigationItem {
    /// An entry with the label `label`, no group, no sort, no icon and no URL of its own.
    pub fn make(label: impl Into<String>) -> Self {
        NavigationItem {
            label: label.into(),
            group: None,
            sort: None,
            icon: None,
            url: None,
            ability: None,
        }
    }

    /// The navigation group the entry is listed in, as Filament's `group()`.
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// The entry's place among the entries of its group, as Filament's `sort()`: lower first.
    pub fn sort(mut self, sort: i32) -> Self {
        self.sort = Some(sort);
        self
    }

    /// The name of the entry's icon, such as `settings`, as a resource's `NAVIGATION_ICON`:
    /// the admin theme resolves it.
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// The address the entry links to, as Filament's `url()`. Without one, the entry links to
    /// its first child, and is not listed when it has none.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Lists the entry only for a user who may do `ability`, such as `settings.manage`, as
    /// Filament's `visible(fn (): bool => auth()->user()->can(...))`. The default lists it for
    /// every user. The page it links to checks the ability too, with
    /// [`AdminUser::authorize`](crate::AdminUser::authorize).
    pub fn ability(mut self, ability: impl Into<String>) -> Self {
        self.ability = Some(ability.into());
        self
    }

    /// The entry's label.
    pub fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn group_name(&self) -> Option<&str> {
        self.group.as_deref()
    }

    pub(crate) fn sort_value(&self) -> Option<i32> {
        self.sort
    }

    pub(crate) fn icon_name(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    pub(crate) fn url_value(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub(crate) fn ability_name(&self) -> Option<&str> {
        self.ability.as_deref()
    }
}

/// An entry of the sidebar with what places it: its key (the slug), its label, its group, its
/// sort and the parent it is listed under.
pub(crate) struct Entry<T> {
    pub(crate) item: T,
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) group: Option<String>,
    pub(crate) sort: Option<i32>,
    pub(crate) parent: Option<String>,
}

/// An entry of a section, with the entries listed under it, in order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Node<T> {
    pub(crate) item: T,
    pub(crate) children: Vec<T>,
}

/// A section of the sidebar: the entries of one group under its heading, or the entries
/// without a group, which have no heading.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Section<T> {
    pub(crate) heading: Option<String>,
    pub(crate) items: Vec<Node<T>>,
}

/// Places `entries` in sections. The first section is always the one without a heading, even
/// when it is empty, so the panel can put the dashboard first.
///
/// - The entries without a group (or with a blank one) keep the order they were added in,
///   sorted by their sort; no sort comes before any number, as Filament's default sort does.
/// - The groups come in the order of `declared`. Groups not declared come after them, in the
///   order their first entry comes in when every entry is sorted by its sort.
/// - Inside a group, the entries are sorted by their sort, then by label.
/// - An entry with a parent is listed under the entry of its section whose key or label is the
///   parent's name, in the order above. As in Filament, only an entry without a parent can be
///   a parent, and an entry whose parent is not in its section is left out.
pub(crate) fn arrange<T>(mut entries: Vec<Entry<T>>, declared: &[String]) -> Vec<Section<T>> {
    // A stable sort: entries with the same sort keep the order they were added in.
    entries.sort_by_key(|entry| entry.sort);

    let mut ungrouped = Vec::new();
    let mut groups: Vec<(String, Vec<Entry<T>>)> = Vec::new();
    for entry in entries {
        match entry.group.clone().filter(|group| !group.trim().is_empty()) {
            None => ungrouped.push(entry),
            Some(group) => match groups.iter_mut().find(|(name, _)| *name == group) {
                Some((_, items)) => items.push(entry),
                None => groups.push((group, vec![entry])),
            },
        }
    }

    // Stable again: the groups not declared keep the order they were met in.
    groups.sort_by_key(|(name, _)| {
        declared
            .iter()
            .position(|group| group == name)
            .unwrap_or(declared.len())
    });

    let mut sections = vec![Section {
        heading: None,
        items: nest(ungrouped),
    }];
    for (name, mut items) in groups {
        items.sort_by(|a, b| {
            a.sort.cmp(&b.sort).then_with(|| {
                a.label
                    .to_lowercase()
                    .cmp(&b.label.to_lowercase())
                    .then_with(|| a.label.cmp(&b.label))
            })
        });
        sections.push(Section {
            heading: Some(name),
            items: nest(items),
        });
    }
    sections
}

/// Lists each entry of a section that names a parent under that parent, keeping the order of
/// `entries`, as Filament's navigation does. The parent is found by key first, then by label.
fn nest<T>(entries: Vec<Entry<T>>) -> Vec<Node<T>> {
    let (children, parents): (Vec<_>, Vec<_>) = entries.into_iter().partition(|entry| {
        entry
            .parent
            .as_deref()
            .is_some_and(|parent| !parent.trim().is_empty())
    });
    let mut nodes: Vec<(String, String, Node<T>)> = parents
        .into_iter()
        .map(|entry| {
            let node = Node {
                item: entry.item,
                children: Vec::new(),
            };
            (entry.key, entry.label, node)
        })
        .collect();
    for child in children {
        let parent = child.parent.unwrap_or_default();
        let found = nodes
            .iter()
            .position(|(key, _, _)| *key == parent)
            .or_else(|| nodes.iter().position(|(_, label, _)| *label == parent));
        // A child whose parent is not in the section is left out, as in Filament.
        if let Some(index) = found {
            nodes[index].2.children.push(child.item);
        }
    }
    nodes.into_iter().map(|(_, _, node)| node).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &'static str, group: Option<&str>, sort: Option<i32>) -> Entry<&'static str> {
        Entry {
            item: label,
            key: label.to_lowercase(),
            label: label.to_string(),
            group: group.map(str::to_string),
            sort,
            parent: None,
        }
    }

    fn child(
        label: &'static str,
        group: Option<&str>,
        sort: Option<i32>,
        parent: &str,
    ) -> Entry<&'static str> {
        Entry {
            parent: Some(parent.to_string()),
            ..entry(label, group, sort)
        }
    }

    /// The entries of `section` without their children.
    fn items(section: &Section<&'static str>) -> Vec<&'static str> {
        section.items.iter().map(|node| node.item).collect()
    }

    fn declared(groups: &[&str]) -> Vec<String> {
        groups.iter().map(|group| group.to_string()).collect()
    }

    #[test]
    fn without_groups_or_sorts_the_order_is_the_order_added() {
        let sections = arrange(
            vec![
                entry("Widgets", None, None),
                entry("Apples", None, None),
                entry("Users", None, None),
            ],
            &[],
        );
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading, None);
        assert_eq!(items(&sections[0]), ["Widgets", "Apples", "Users"]);
    }

    #[test]
    fn groups_come_in_the_declared_order_after_the_ungrouped_entries() {
        let sections = arrange(
            vec![
                entry("Settings", Some("System"), None),
                entry("Posts", Some("Content"), None),
                entry("Widgets", None, None),
                entry("Orders", Some("Shop"), None),
            ],
            &declared(&["Content", "Shop", "System"]),
        );
        let headings: Vec<Option<&str>> = sections.iter().map(|s| s.heading.as_deref()).collect();
        assert_eq!(
            headings,
            [None, Some("Content"), Some("Shop"), Some("System")]
        );
        assert_eq!(items(&sections[0]), ["Widgets"]);
    }

    #[test]
    fn a_group_not_declared_comes_after_the_declared_ones() {
        let sections = arrange(
            vec![
                entry("Logs", Some("Tools"), None),
                entry("Posts", Some("Content"), None),
                entry("Backups", Some("Backups"), None),
            ],
            &declared(&["Content"]),
        );
        let headings: Vec<Option<&str>> = sections.iter().map(|s| s.heading.as_deref()).collect();
        assert_eq!(
            headings,
            [None, Some("Content"), Some("Tools"), Some("Backups")]
        );
    }

    #[test]
    fn inside_a_group_entries_sort_by_sort_then_label() {
        let sections = arrange(
            vec![
                entry("Tags", Some("Content"), Some(3)),
                entry("Pages", Some("Content"), Some(2)),
                entry("Posts", Some("Content"), Some(1)),
                entry("Media", Some("Content"), Some(3)),
                entry("comments", Some("Content"), Some(3)),
            ],
            &[],
        );
        assert_eq!(
            items(&sections[1]),
            ["Posts", "Pages", "comments", "Media", "Tags"]
        );
    }

    #[test]
    fn ungrouped_entries_sort_by_sort_and_keep_their_order_otherwise() {
        let sections = arrange(
            vec![
                entry("Zebras", None, None),
                entry("Lions", None, Some(2)),
                entry("Apes", None, None),
                entry("Bears", None, Some(1)),
            ],
            &[],
        );
        assert_eq!(items(&sections[0]), ["Zebras", "Apes", "Bears", "Lions"]);
    }

    #[test]
    fn a_blank_group_is_no_group() {
        let sections = arrange(vec![entry("Widgets", Some(" "), None)], &[]);
        assert_eq!(sections.len(), 1);
        assert_eq!(items(&sections[0]), ["Widgets"]);
    }

    #[test]
    fn with_only_groups_the_first_section_is_empty() {
        let sections = arrange(vec![entry("Posts", Some("Content"), None)], &[]);
        assert_eq!(sections.len(), 2);
        assert!(sections[0].items.is_empty());
        assert_eq!(sections[0].heading, None);
    }

    #[test]
    fn a_child_is_listed_under_its_parent_by_label_or_key_in_sort_order() {
        let sections = arrange(
            vec![
                entry("Posts", Some("Content"), Some(1)),
                child("Tags", Some("Content"), Some(3), "Posts"),
                child("Categories", Some("Content"), Some(2), "posts"),
                entry("Pages", Some("Content"), Some(2)),
            ],
            &[],
        );
        assert_eq!(items(&sections[1]), ["Posts", "Pages"]);
        assert_eq!(sections[1].items[0].children, ["Categories", "Tags"]);
        assert!(sections[1].items[1].children.is_empty());
    }

    #[test]
    fn a_child_whose_parent_is_not_in_its_section_is_left_out() {
        let sections = arrange(
            vec![
                entry("Posts", Some("Content"), None),
                // Another group, an unknown parent, and a parent that is a child itself.
                child("Tags", None, None, "Posts"),
                child("Ghosts", Some("Content"), None, "Nowhere"),
                child("Categories", Some("Content"), None, "Posts"),
                child("Colours", Some("Content"), None, "Categories"),
            ],
            &[],
        );
        assert!(sections[0].items.is_empty());
        assert_eq!(items(&sections[1]), ["Posts"]);
        assert_eq!(sections[1].items[0].children, ["Categories"]);
    }

    #[test]
    fn a_blank_parent_is_no_parent() {
        let sections = arrange(vec![child("Widgets", None, None, " ")], &[]);
        assert_eq!(items(&sections[0]), ["Widgets"]);
    }

    #[test]
    fn collapsed_makes_a_group_collapsible_as_in_filament() {
        let group = NavigationGroup::make("Settings");
        assert!(!group.is_collapsible(false) && group.is_collapsible(true));
        assert!(!group.is_collapsed(true));
        let collapsed = NavigationGroup::make("Settings").collapsed(true);
        assert!(collapsed.is_collapsible(false) && collapsed.is_collapsed(false));
        let fixed = NavigationGroup::make("Settings").collapsible(false);
        assert!(!fixed.is_collapsible(true));
    }
}
