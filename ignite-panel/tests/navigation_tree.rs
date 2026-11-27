//! The sidebar's tree, as Filament's navigation builds it: entries listed under a parent entry
//! (`$navigationParentItem`), and collapsible navigation groups (`NavigationGroup::collapsible()`
//! and `collapsed()`), collapsible by default as in Filament. Runs against the `ignite_test`
//! database. The sidebar needs no rows, so the resources' tables are never created.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, IgniteResource, ListQuery, Page, Row, RuntimeData,
    RuntimeFuture, RuntimeResource,
};
use ignite_panel::{Ignite, NavigationGroup};
use orbit_orm::{Database, OrbitModel};
use rocket::http::Status;
use rocket::local::blocking::Client;
use serde::Serialize;

/// The HTML of `path`.
fn page(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    response.into_string().unwrap()
}

/// The sidebar of `html`: the `<aside>` element, with the navigation and its script.
fn sidebar(html: &str) -> &str {
    let start = html.find("<aside ").expect("the sidebar");
    let end = html[start..]
        .find("</aside>")
        .expect("the end of the sidebar")
        + start;
    &html[start..end]
}

/// Where `needle` is in `haystack`, which must hold it.
fn at(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not in:\n{haystack}"))
}

/// A child entry's link, which has no icon.
fn child(slug: &str, label: &str) -> String {
    format!(r#"<a class="ig-nav-link py-1.5 pl-11 text-[13px]" href="/admin/{slug}">{label}</a>"#)
}

/// The link of the child entry of the current page.
fn active_child(slug: &str, label: &str) -> String {
    format!(
        r#"<a class="ig-nav-link ig-nav-link-active py-1.5 pl-11 text-[13px]" href="/admin/{slug}" aria-current="page">{label}</a>"#
    )
}

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "tree_posts")]
#[ignite(
    table = tree_posts,
    title = "Posts",
    navigation_group = "Content",
    navigation_sort = 1
)]
pub struct Post {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

/// Listed under Posts by its label, after Categories.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "tree_tags")]
#[ignite(
    table = tree_tags,
    title = "Tags",
    navigation_group = "Content",
    navigation_parent_item = "Posts",
    navigation_sort = 2
)]
pub struct Tag {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

/// Listed under Posts by its slug, before Tags.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "tree_categories")]
#[ignite(
    table = tree_categories,
    title = "Categories",
    navigation_group = "Content",
    navigation_parent_item = "tree_posts",
    navigation_sort = 1
)]
pub struct Category {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "tree_pages")]
#[ignite(
    table = tree_pages,
    title = "Pages",
    navigation_group = "Content",
    navigation_sort = 2
)]
pub struct PageRow {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "tree_options")]
#[ignite(table = tree_options, title = "General", navigation_group = "Settings")]
pub struct Options {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

#[test]
fn the_derive_sets_the_parent_item() {
    assert_eq!(
        <Tag as IgniteResource>::NAVIGATION_PARENT_ITEM,
        Some("Posts")
    );
    assert_eq!(<Post as IgniteResource>::NAVIGATION_PARENT_ITEM, None);
}

/// The panel of these tests: `Widgets` (no group), Content with Posts (and its children
/// Categories and Tags) and Pages, and Settings.
fn with_tree(ignite: Ignite) -> Ignite {
    ignite
        .resource::<Post>()
        .resource::<Tag>()
        .resource::<Category>()
        .resource::<PageRow>()
        .resource::<Options>()
        .navigation_groups(["Content", "Settings"])
}

#[test]
fn a_child_entry_renders_under_its_parent_in_sort_order() {
    let _guard = reset();
    let client = signed_in_with(with_tree);
    let html = page(&client, "/admin/tree_posts/new");
    let nav = sidebar(&html);
    let order = [
        at(
            nav,
            r#"aria-current="page"><i class="ti ti-table text-lg"></i>Posts</a>"#,
        ),
        at(nav, r#"href="/admin/tree_posts/new">Add new</a>"#),
        at(nav, &child("tree_categories", "Categories")),
        at(nav, &child("tree_tags", "Tags")),
        at(nav, "></i>Pages</a>"),
    ];
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "out of order: {order:?}\n{nav}"
    );
    // A child is not an entry of its own.
    assert!(!nav.contains("></i>Tags</a>"), "{nav}");
}

// Since 0.6.6 the children are in the page in a closed group, hidden until its toggle opens
// it (see nav_groups_toggle.rs); before, they were left out of the page.
#[test]
fn children_show_only_when_their_parent_or_one_of_them_is_the_current_page() {
    let _guard = reset();
    let client = signed_in_with(with_tree);
    let html = page(&client, "/admin/tree_pages/new");
    let nav = sidebar(&html);
    assert!(nav.contains("></i>Posts</a>"), "{nav}");
    let hidden = at(
        nav,
        r#"<div id="ig-nav-items-1-0" class="flex flex-col gap-1" hidden>"#,
    );
    assert!(
        hidden < at(nav, &child("tree_categories", "Categories")),
        "{nav}"
    );
    assert!(hidden < at(nav, &child("tree_tags", "Tags")), "{nav}");
    assert!(
        at(nav, &child("tree_tags", "Tags")) < at(nav, "></i>Pages</a>"),
        "{nav}"
    );
}

#[test]
fn the_active_child_opens_its_parent() {
    let _guard = reset();
    let client = signed_in_with(with_tree);
    let html = page(&client, "/admin/tree_tags/new");
    let nav = sidebar(&html);
    // The child is the current page; its parent shows its children, and is not active itself.
    assert!(nav.contains(&active_child("tree_tags", "Tags")), "{nav}");
    assert!(
        nav.contains(&child("tree_categories", "Categories")),
        "{nav}"
    );
    assert!(
        nav.contains(r#"<a class="ig-nav-link" href="/admin/tree_posts"><i class="ti ti-table text-lg"></i>Posts</a>"#),
        "{nav}"
    );
    assert_eq!(nav.matches(r#"aria-current="page""#).count(), 1, "{nav}");
}

#[test]
fn a_child_whose_parent_is_unknown_is_not_listed() {
    let _guard = reset();
    register_runtime(
        RuntimeResource::new("tree-ghosts", "Ghosts", "Ghost", NoRows)
            .navigation_group("Content")
            .navigation_parent_item("Nowhere"),
    )
    .expect("register");
    register_runtime(
        RuntimeResource::new("tree-media", "Media", "Medium", NoRows)
            .navigation_group("Content")
            .navigation_parent_item("Posts")
            .navigation_sort(0),
    )
    .expect("register");
    let client = signed_in_with(with_tree);
    let html = page(&client, "/admin/tree_posts/new");
    unregister_runtime("tree-ghosts").expect("unregister");
    unregister_runtime("tree-media").expect("unregister");
    let nav = sidebar(&html);
    assert!(!nav.contains("Ghosts"), "{nav}");
    // A runtime resource is listed under its parent too, sorted among the children.
    assert!(
        at(nav, &child("tree-media", "Media")) < at(nav, &child("tree_categories", "Categories")),
        "{nav}"
    );
}

#[test]
fn a_child_label_is_escaped() {
    let _guard = reset();
    register_runtime(
        RuntimeResource::new("tree-raw", "Raw", "Raw", NoRows)
            .navigation_label("<b>Raw</b> & co")
            .navigation_parent_item("Dashboard"),
    )
    .expect("register");
    let client = signed_in_with(with_tree);
    let html = page(&client, "/admin");
    unregister_runtime("tree-raw").expect("unregister");
    let nav = sidebar(&html);
    // The dashboard can be a parent, as a page is in Filament.
    assert!(
        nav.contains(&child("tree-raw", "&lt;b&gt;Raw&lt;/b&gt; &amp; co")),
        "{nav}"
    );
    assert!(!nav.contains("<b>Raw</b>"));
}

/// The opening tag of the collapsible group `label`.
fn group_tag(nav: &str, label: &str) -> String {
    let needle = format!(r#"data-ig-nav-group="{label}""#);
    let start = nav[..at(nav, &needle)].rfind('<').unwrap();
    let end = nav[start..].find('>').unwrap() + start + 1;
    nav[start..end].to_string()
}

#[test]
fn a_collapsible_group_renders_expanded_with_its_state_attributes() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        with_tree(ignite).navigation_groups([
            NavigationGroup::make("Settings").collapsed(true),
            NavigationGroup::make("Content").collapsible(true),
        ])
    });
    let html = page(&client, "/admin/tree_tags/new");
    let nav = sidebar(&html);

    // Content holds the current page; Settings starts collapsed.
    assert_eq!(
        group_tag(nav, "Content"),
        r#"<div class="flex flex-col gap-1" data-ig-nav-group="Content" data-ig-nav-active>"#
    );
    assert_eq!(
        group_tag(nav, "Settings"),
        r#"<div class="flex flex-col gap-1" data-ig-nav-group="Settings" data-ig-nav-collapsed>"#
    );
    // Without JavaScript every group is expanded: the heading says so, and its entries are
    // in the page and not hidden.
    let button = at(nav, r#"<button type="button" class="ig-caption"#);
    assert!(
        nav[button..].starts_with(
            r#"<button type="button" class="ig-caption mt-5 flex w-full items-center justify-between" aria-expanded="true" aria-controls="ig-nav-group-1" data-ig-nav-toggle>Content<i"#
        ),
        "{nav}"
    );
    assert!(nav.contains(r#"<div id="ig-nav-group-2" class="flex flex-col gap-1">"#));
    assert!(
        at(nav, r#"aria-controls="ig-nav-group-2""#) < at(nav, "></i>General</a>"),
        "{nav}"
    );
    // The groups' sections are not hidden (an entry's closed group of links is, since 0.6.6).
    let markup = &nav[..at(nav, "<script>")];
    assert!(!markup.contains(r#"<div id="ig-nav-group-1" class="flex flex-col gap-1" hidden"#));
    assert!(!markup.contains(r#"<div id="ig-nav-group-2" class="flex flex-col gap-1" hidden"#));
    // The script that collapses and remembers the groups is in the sidebar.
    assert!(nav.contains("ignite:sidebar:collapsed_groups"), "{nav}");
    assert!(!nav.contains(r#"<p class="ig-caption mt-5">"#), "{nav}");
}

#[test]
fn collapsible_navigation_groups_makes_every_group_collapsible() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        with_tree(ignite)
            .collapsible_navigation_groups(true)
            .navigation_groups([NavigationGroup::make("Settings").collapsible(false)])
    });
    let html = page(&client, "/admin");
    let nav = sidebar(&html);
    assert!(nav.contains(r#"data-ig-nav-group="Content""#), "{nav}");
    // A group that says otherwise keeps its plain heading.
    assert!(
        nav.contains(r#"<p class="ig-caption mt-5">Settings</p>"#),
        "{nav}"
    );
    assert!(!nav.contains(r#"data-ig-nav-group="Settings""#), "{nav}");
}

#[test]
fn a_group_heading_is_escaped_in_its_attributes() {
    let _guard = reset();
    register_runtime(
        RuntimeResource::new("tree-quotes", "Quotes", "Quote", NoRows)
            .navigation_group(r#"Say "hi" <now>"#),
    )
    .expect("register");
    let client = signed_in_with(|ignite| with_tree(ignite).collapsible_navigation_groups(true));
    let html = page(&client, "/admin");
    unregister_runtime("tree-quotes").expect("unregister");
    let nav = sidebar(&html);
    assert!(
        nav.contains(r#"data-ig-nav-group="Say &quot;hi&quot; &lt;now&gt;""#),
        "{nav}"
    );
    assert!(!nav.contains("<now>"));
}

/// The panel of the sidebar fixtures: Posts and Pages in Content, and General in Settings,
/// with no child entries.
fn with_groups(ignite: Ignite) -> Ignite {
    ignite
        .resource::<Post>()
        .resource::<PageRow>()
        .resource::<Options>()
        .navigation_groups(["Content", "Settings"])
}

/// The sidebar of `/admin/tree_posts/new` in [`with_groups`] with no other option: its groups
/// collapsible, as Filament's are by default.
const SIDEBAR_BEFORE: &str = include_str!("fixtures/navigation/sidebar_before.html");

/// The same sidebar as Ignite 0.6.2 rendered it, before collapsible groups: each group under a
/// plain heading.
const SIDEBAR_PLAIN: &str = include_str!("fixtures/navigation/sidebar_plain.html");

#[test]
fn without_options_the_groups_are_collapsible() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let html = page(&client, "/admin/tree_posts/new");
    let nav = sidebar(&html);
    assert!(nav == SIDEBAR_BEFORE, "{nav}");
    assert_eq!(
        group_tag(nav, "Content"),
        r#"<div class="flex flex-col gap-1" data-ig-nav-group="Content" data-ig-nav-active>"#
    );
    assert_eq!(
        group_tag(nav, "Settings"),
        r#"<div class="flex flex-col gap-1" data-ig-nav-group="Settings">"#
    );
    assert!(!nav.contains(r#"<p class="ig-caption mt-5">"#), "{nav}");
    assert!(nav.contains("ignite:sidebar:collapsed_groups"), "{nav}");
}

#[test]
fn collapsible_navigation_groups_false_renders_plain_groups_as_before() {
    let _guard = reset();
    let client = signed_in_with(|ignite| with_groups(ignite).collapsible_navigation_groups(false));
    let html = page(&client, "/admin/tree_posts/new");
    let nav = sidebar(&html);
    assert!(nav == SIDEBAR_PLAIN, "{nav}");
    assert!(!nav.contains("data-ig-nav-group"), "{nav}");
    // No script for the groups; since 0.6.6 the entries' own groups have theirs.
    assert!(!nav.contains("ignite:sidebar:collapsed_groups"), "{nav}");
}

#[test]
fn a_group_opts_out_of_the_default() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        with_groups(ignite)
            .navigation_groups([NavigationGroup::make("Settings").collapsible(false)])
    });
    let html = page(&client, "/admin/tree_posts/new");
    let nav = sidebar(&html);
    assert!(nav.contains(r#"data-ig-nav-group="Content""#), "{nav}");
    assert!(
        nav.contains(r#"<p class="ig-caption mt-5">Settings</p>"#),
        "{nav}"
    );
    assert!(!nav.contains(r#"data-ig-nav-group="Settings""#), "{nav}");
}

#[test]
fn a_group_opts_in_when_the_panel_opts_out() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        with_groups(ignite)
            .collapsible_navigation_groups(false)
            .navigation_groups([NavigationGroup::make("Settings").collapsible(true)])
    });
    let html = page(&client, "/admin/tree_posts/new");
    let nav = sidebar(&html);
    assert!(nav.contains(r#"data-ig-nav-group="Settings""#), "{nav}");
    assert!(
        nav.contains(r#"<p class="ig-caption mt-5">Content</p>"#),
        "{nav}"
    );
    assert!(!nav.contains(r#"data-ig-nav-group="Content""#), "{nav}");
}

/// A data source with no rows: the sidebar does not read any.
struct NoRows;

impl RuntimeData for NoRows {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async {
            Ok(Page {
                rows: Vec::new(),
                total: 0,
            })
        })
    }
    fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async { Ok(None) })
    }
    fn create<'a>(&'a self, _: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Ok(input) })
    }
    fn update<'a>(&'a self, _: &'a Database, _: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Ok(input) })
    }
    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }
}
