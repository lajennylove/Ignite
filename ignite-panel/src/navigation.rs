//! The order of the sidebar, as Filament's navigation builds it: the entries without a group
//! first, then one section per navigation group under its heading. The groups come in the
//! order the panel declares them ([`Ignite::navigation_groups`](crate::Ignite::navigation_groups)),
//! and a group the panel does not declare comes after the declared ones, as in Filament.

/// An entry of the sidebar with what places it: its label, its group and its sort.
pub(crate) struct Entry<T> {
    pub(crate) item: T,
    pub(crate) label: String,
    pub(crate) group: Option<String>,
    pub(crate) sort: Option<i32>,
}

/// A section of the sidebar: the entries of one group under its heading, or the entries
/// without a group, which have no heading.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Section<T> {
    pub(crate) heading: Option<String>,
    pub(crate) items: Vec<T>,
}

/// Places `entries` in sections. The first section is always the one without a heading, even
/// when it is empty, so the panel can put the dashboard first.
///
/// - The entries without a group (or with a blank one) keep the order they were added in,
///   sorted by their sort; no sort comes before any number, as Filament's default sort does.
/// - The groups come in the order of `declared`. Groups not declared come after them, in the
///   order their first entry comes in when every entry is sorted by its sort.
/// - Inside a group, the entries are sorted by their sort, then by label.
pub(crate) fn arrange<T>(mut entries: Vec<Entry<T>>, declared: &[String]) -> Vec<Section<T>> {
    // A stable sort: entries with the same sort keep the order they were added in.
    entries.sort_by_key(|entry| entry.sort);

    let mut ungrouped = Vec::new();
    let mut groups: Vec<(String, Vec<Entry<T>>)> = Vec::new();
    for entry in entries {
        match entry.group.clone().filter(|group| !group.trim().is_empty()) {
            None => ungrouped.push(entry.item),
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
        items: ungrouped,
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
            items: items.into_iter().map(|entry| entry.item).collect(),
        });
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &'static str, group: Option<&str>, sort: Option<i32>) -> Entry<&'static str> {
        Entry {
            item: label,
            label: label.to_string(),
            group: group.map(str::to_string),
            sort,
        }
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
        assert_eq!(
            sections,
            [Section {
                heading: None,
                items: vec!["Widgets", "Apples", "Users"],
            }]
        );
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
        assert_eq!(sections[0].items, ["Widgets"]);
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
            sections[1].items,
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
        assert_eq!(sections[0].items, ["Zebras", "Apes", "Bears", "Lions"]);
    }

    #[test]
    fn a_blank_group_is_no_group() {
        let sections = arrange(vec![entry("Widgets", Some(" "), None)], &[]);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].items, ["Widgets"]);
    }

    #[test]
    fn with_only_groups_the_first_section_is_empty() {
        let sections = arrange(vec![entry("Posts", Some("Content"), None)], &[]);
        assert_eq!(sections.len(), 2);
        assert!(sections[0].items.is_empty());
        assert_eq!(sections[0].heading, None);
    }
}
