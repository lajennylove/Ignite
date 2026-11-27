//! A parent entry that is only a label, as a Filament cluster's navigation item: the panel
//! declares it with `Ignite::navigation_items`, resources list themselves under it with their
//! navigation parent item, and it opens its first child. Runs against the test database; the
//! sidebar needs no rows.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ListQuery, Page, Row, RuntimeData, RuntimeFuture,
    RuntimeResource,
};
use ignite_panel::{Ignite, NavigationItem};
use orbit_orm::Database;
use rocket::http::Status;
use rocket::local::blocking::Client;

/// The settings screens, in the order WordPress lists them, with their sorts.
const SCREENS: [(&str, &str, i32); 7] = [
    ("np-general", "General", 1),
    ("np-writing", "Writing", 2),
    ("np-reading", "Reading", 3),
    ("np-discussion", "Discussion", 4),
    ("np-media", "Media", 5),
    ("np-permalinks", "Permalinks", 6),
    ("np-privacy", "Privacy", 7),
];

/// Rows that are never written: the tests only render lists.
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

    fn create<'a>(&'a self, _: &'a Database, _: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Err(orbit_orm::Error::NotFound { table: "none" }) })
    }

    fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Err(orbit_orm::Error::NotFound { table: "none" }) })
    }

    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }
}

/// Registers the settings screens under the parent "Settings", in reverse order, so the
/// sidebar's order comes from their sorts.
fn register_screens() {
    for (slug, label, sort) in SCREENS.iter().rev() {
        let _ = unregister_runtime(slug);
        register_runtime(
            RuntimeResource::new(*slug, *label, *label, NoRows)
                .navigation_group("Settings")
                .navigation_parent_item("Settings")
                .navigation_sort(*sort),
        )
        .expect("register");
    }
}

fn unregister_screens() {
    for (slug, _, _) in SCREENS {
        let _ = unregister_runtime(slug);
    }
}

/// The panel with the label-only parent "Settings" in the Settings group.
fn with_settings(ignite: Ignite) -> Ignite {
    ignite
        .navigation_groups(["Settings"])
        .navigation_items([NavigationItem::make("Settings")
            .group("Settings")
            .sort(1)
            .icon("settings")])
}

fn sidebar(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    let html = response.into_string().unwrap();
    let start = html.find("<aside ").expect("the sidebar");
    let end = html[start..].find("</aside>").expect("the end") + start;
    html[start..end].to_string()
}

fn at(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not in:\n{haystack}"))
}

fn child(slug: &str, label: &str) -> String {
    format!(r#"<a class="ig-nav-link py-1.5 pl-11 text-[13px]" href="/admin/{slug}">{label}</a>"#)
}

#[test]
fn a_label_only_parent_opens_its_first_child() {
    let _guard = reset();
    register_screens();
    let client = signed_in_with(with_settings);
    let nav = sidebar(&client, "/admin");
    unregister_screens();

    // "Settings" links to General, its first child by sort, and is not a page of its own.
    assert!(
        nav.contains(r#"<a class="ig-nav-link" href="/admin/np-general"><i class="ti ti-settings text-lg"></i>Settings</a>"#),
        "{nav}"
    );
    // It has no "Add new" link, and its children show only when one is the current page:
    // since 0.6.6 they are in the page, in its group, hidden until the toggle opens it.
    // (Each child lists its own create link after it, since 0.6.6.)
    assert!(
        at(&nav, &child("np-general", "General")) < at(&nav, "/admin/np-general/new"),
        "{nav}"
    );
    let hidden = at(&nav, r#"class="flex flex-col gap-1" hidden>"#);
    assert!(hidden < at(&nav, &child("np-writing", "Writing")), "{nav}");
    // The children are not entries of their own.
    assert!(!nav.contains("></i>General</a>"), "{nav}");
}

#[test]
fn the_children_show_under_the_parent_in_sort_order() {
    let _guard = reset();
    register_screens();
    let client = signed_in_with(with_settings);
    let nav = sidebar(&client, "/admin/np-writing");
    let general = sidebar(&client, "/admin/np-general");
    unregister_screens();

    let parent = at(&nav, "></i>Settings</a>");
    let mut last = parent;
    for (slug, label, _) in SCREENS {
        let place = if slug == "np-writing" {
            at(
                &nav,
                r#"<a class="ig-nav-link ig-nav-link-active py-1.5 pl-11 text-[13px]" href="/admin/np-writing" aria-current="page">Writing</a>"#,
            )
        } else {
            at(&nav, &child(slug, label))
        };
        assert!(place > last, "{label} is out of order:\n{nav}");
        last = place;
    }
    // The parent is not the current page: its child is.
    assert_eq!(nav.matches(r#"aria-current="page""#).count(), 1, "{nav}");
    assert!(
        nav.contains(r#"<a class="ig-nav-link" href="/admin/np-general"><i class="#),
        "{nav}"
    );
    // Clicking "Settings" lands on General, which shows the children too.
    assert!(general.contains("Privacy</a>"), "{general}");
    assert!(
        general.contains(r#"href="/admin/np-general" aria-current="page">General</a>"#),
        "{general}"
    );
}

#[test]
fn a_label_only_parent_without_children_is_not_listed() {
    let _guard = reset();
    unregister_screens();
    let client = signed_in_with(with_settings);
    let nav = sidebar(&client, "/admin");
    assert!(!nav.contains("Settings</a>"), "{nav}");
}

#[test]
fn the_parent_follows_its_own_group_and_sort() {
    let _guard = reset();
    register_screens();
    let _ = unregister_runtime("np-tools");
    register_runtime(
        RuntimeResource::new("np-tools", "Tools", "Tool", NoRows)
            .navigation_group("Settings")
            .navigation_sort(0),
    )
    .expect("register");
    let client = signed_in_with(with_settings);
    let nav = sidebar(&client, "/admin");
    unregister_screens();
    unregister_runtime("np-tools").expect("unregister");

    // Tools (sort 0) comes before Settings (sort 1) in the Settings group.
    assert!(
        at(&nav, "></i>Tools</a>") < at(&nav, "></i>Settings</a>"),
        "{nav}"
    );
    assert!(at(&nav, r#"data-ig-nav-group="Settings""#) < at(&nav, "></i>Tools</a>"));
}

#[test]
fn an_item_with_a_url_is_a_plain_link() {
    let _guard = reset();
    unregister_screens();
    let client = signed_in_with(|ignite| {
        ignite.navigation_items([NavigationItem::make("Site")
            .url("https://example.com/")
            .icon("home")])
    });
    let nav = sidebar(&client, "/admin");
    assert!(
        nav.contains(r#"href="https://example.com/"><i class="#),
        "{nav}"
    );
    assert!(nav.contains("></i>Site</a>"), "{nav}");
}

#[test]
fn the_parent_label_is_escaped() {
    let _guard = reset();
    let _ = unregister_runtime("np-raw");
    register_runtime(
        RuntimeResource::new("np-raw", "Raw", "Raw", NoRows).navigation_parent_item("<b>A</b> & B"),
    )
    .expect("register");
    let client =
        signed_in_with(|ignite| ignite.navigation_items([NavigationItem::make("<b>A</b> & B")]));
    let nav = sidebar(&client, "/admin/np-raw");
    unregister_runtime("np-raw").expect("unregister");
    assert!(nav.contains("&lt;b&gt;A&lt;/b&gt; &amp; B</a>"), "{nav}");
    assert!(!nav.contains("<b>A</b>"), "{nav}");
}
