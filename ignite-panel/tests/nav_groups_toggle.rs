//! A navigation entry's group, as a WordPress menu with its submenu: the entry's create link
//! and its child entries (Filament's `$navigationParentItem`) show only while the group is
//! open, and the toggle beside the entry opens and closes it. A resource's create link is part
//! of its group, below the resource, and a resource can leave it out or relabel it
//! (`NAVIGATION_CREATE_ITEM`, `NAVIGATION_CREATE_LABEL`). Runs against the test database; the
//! sidebar needs no rows, so the resources' tables are never created.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, IgniteResource, ListQuery, Page, Row, RuntimeData,
    RuntimeFuture, RuntimeResource,
};
use ignite_panel::{Ignite, NavigationItem};
use orbit_orm::{Database, OrbitModel};
use rocket::http::Status;
use rocket::local::blocking::Client;
use serde::Serialize;

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "ng_posts")]
#[ignite(
    table = ng_posts,
    title = "Posts",
    navigation_group = "Content",
    navigation_sort = 1,
    navigation_create_label = "Add New Post"
)]
pub struct Post {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

/// A child of Posts without a create link, as WordPress lists Categories.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "ng_categories")]
#[ignite(
    table = ng_categories,
    title = "Categories",
    navigation_group = "Content",
    navigation_parent_item = "Posts",
    navigation_sort = 1,
    navigation_create_item = false
)]
pub struct Category {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

/// A child of Posts with the default create link.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "ng_tags")]
#[ignite(
    table = ng_tags,
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

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "ng_pages")]
#[ignite(table = ng_pages, title = "Pages", navigation_group = "Content", navigation_sort = 2)]
pub struct PageRow {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

fn with_groups(ignite: Ignite) -> Ignite {
    ignite
        .resource::<Post>()
        .resource::<Category>()
        .resource::<Tag>()
        .resource::<PageRow>()
        .navigation_groups(["Content", "Settings"])
}

fn page(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    response.into_string().unwrap()
}

fn sidebar(html: &str) -> String {
    let start = html.find("<aside ").expect("the sidebar");
    let end = html[start..].find("</aside>").expect("the end") + start;
    html[start..end].to_string()
}

fn at(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not in:\n{haystack}"))
}

/// The markup of the group whose entry links to `href`: from its wrapper to the end of its
/// links.
fn group<'a>(nav: &'a str, href: &str) -> &'a str {
    let link = at(nav, &format!(r#"href="{href}""#));
    let start = nav[..link]
        .rfind("<div class=\"flex flex-col gap-1\" data-ig-nav-item")
        .unwrap();
    let end = nav[link..].find("</div>\n      </div>").unwrap() + link;
    &nav[start..end]
}

fn sub_link(href: &str, label: &str) -> String {
    format!(r#"<a class="ig-nav-link py-1.5 pl-11 text-[13px]" href="{href}">{label}</a>"#)
}

#[test]
fn the_derive_sets_the_create_link() {
    const { assert!(<Post as IgniteResource>::NAVIGATION_CREATE_ITEM) };
    assert_eq!(
        <Post as IgniteResource>::NAVIGATION_CREATE_LABEL,
        "Add New Post"
    );
    const { assert!(!<Category as IgniteResource>::NAVIGATION_CREATE_ITEM) };
    assert_eq!(<Tag as IgniteResource>::NAVIGATION_CREATE_LABEL, "Add new");
}

#[test]
fn a_group_lists_its_create_link_and_children_in_order() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let nav = sidebar(&page(&client, "/admin/ng_posts/new"));
    let posts = group(&nav, "/admin/ng_posts");
    let order = [
        at(posts, "></i>Posts</a>"),
        at(posts, "data-ig-nav-item-toggle"),
        at(posts, &sub_link("/admin/ng_posts/new", "Add New Post")),
        at(posts, &sub_link("/admin/ng_categories", "Categories")),
        at(posts, &sub_link("/admin/ng_tags", "Tags")),
        at(posts, &sub_link("/admin/ng_tags/new", "Add new")),
    ];
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "out of order: {order:?}\n{posts}"
    );
    // Categories lists no create link.
    assert!(!nav.contains("/admin/ng_categories/new"), "{nav}");
}

#[test]
fn the_group_of_the_current_page_is_open_and_the_others_closed() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let nav = sidebar(&page(&client, "/admin/ng_tags/new"));

    let posts = group(&nav, "/admin/ng_posts");
    assert!(
        posts.starts_with(r#"<div class="flex flex-col gap-1" data-ig-nav-item data-ig-nav-open>"#),
        "{posts}"
    );
    assert!(posts.contains(r#"aria-expanded="true""#), "{posts}");
    assert!(!posts.contains(" hidden"), "{posts}");
    assert!(
        posts.contains(r#"href="/admin/ng_tags" aria-current="page">Tags</a>"#),
        "{posts}"
    );

    // Pages is closed: its create link is in the page, hidden until the toggle opens it.
    let pages = group(&nav, "/admin/ng_pages");
    assert!(
        pages.starts_with(r#"<div class="flex flex-col gap-1" data-ig-nav-item>"#),
        "{pages}"
    );
    assert!(pages.contains(r#"aria-expanded="false""#), "{pages}");
    let toggle = at(pages, "aria-controls=\"");
    let id = &pages[toggle + 15..];
    let id = &id[..id.find('"').unwrap()];
    assert!(
        pages.contains(&format!(
            r#"<div id="{id}" class="flex flex-col gap-1" hidden>"#
        )),
        "{pages}"
    );
    assert!(
        at(pages, " hidden>") < at(pages, &sub_link("/admin/ng_pages/new", "Add new")),
        "{pages}"
    );
}

#[test]
fn children_are_listed_in_a_closed_group_too() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let nav = sidebar(&page(&client, "/admin"));
    let posts = group(&nav, "/admin/ng_posts");
    // The dashboard is current: the Posts group is closed, and holds its links hidden.
    assert!(posts.contains(r#"aria-expanded="false""#), "{posts}");
    assert!(posts.contains(" hidden>"), "{posts}");
    assert!(posts.contains(&sub_link("/admin/ng_categories", "Categories")));
    assert!(posts.contains(&sub_link("/admin/ng_tags", "Tags")));
}

#[test]
fn the_toggle_names_its_group_and_the_script_toggles_it() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let nav = sidebar(&page(&client, "/admin"));
    assert!(
        nav.contains(r#"aria-label="Posts menu" data-ig-nav-item-toggle>"#),
        "{nav}"
    );
    assert!(
        nav.contains("document.querySelectorAll('[data-ig-nav-item-toggle]')"),
        "{nav}"
    );
    // The dashboard has no group: it is a plain link without a toggle.
    let dashboard = at(&nav, "></i>Dashboard</a>");
    assert!(
        !nav[..dashboard].contains("data-ig-nav-item"),
        "the dashboard is not wrapped in a group:\n{nav}"
    );
}

/// Rows that are never read: the tests only render the sidebar.
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
        Box::pin(async move { Ok(input) })
    }

    fn update<'a>(&'a self, _: &'a Database, _: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move { Ok(input) })
    }

    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }
}

#[test]
fn a_runtime_resource_relabels_or_leaves_out_its_create_link() {
    let _guard = reset();
    let _ = unregister_runtime("ng-books");
    let _ = unregister_runtime("ng-genres");
    register_runtime(
        RuntimeResource::new("ng-books", "Books", "Book", NoRows)
            .navigation_create_label("<Add> & book"),
    )
    .expect("register");
    register_runtime(
        RuntimeResource::new("ng-genres", "Genres", "Genre", NoRows)
            .navigation_parent_item("Books")
            .navigation_create_item(false),
    )
    .expect("register");
    let client = signed_in(None);
    let nav = sidebar(&page(&client, "/admin/ng-genres/new"));
    unregister_runtime("ng-books").expect("unregister");
    unregister_runtime("ng-genres").expect("unregister");

    let books = group(&nav, "/admin/ng-books");
    assert!(
        books.contains(&sub_link("/admin/ng-books/new", "&lt;Add&gt; &amp; book")),
        "{books}"
    );
    assert!(!nav.contains("/admin/ng-genres/new"), "{nav}");
    assert!(books.contains(r#"aria-expanded="true""#), "{books}");
}

#[test]
fn a_label_only_parent_opens_and_closes_its_children() {
    let _guard = reset();
    let _ = unregister_runtime("ng-general");
    register_runtime(
        RuntimeResource::new("ng-general", "General", "General", NoRows)
            .navigation_group("Settings")
            .navigation_parent_item("Settings")
            .navigation_create_item(false),
    )
    .expect("register");
    let client = signed_in_with(|ignite| {
        with_groups(ignite).navigation_items([NavigationItem::make("Settings")
            .group("Settings")
            .icon("settings")])
    });
    let closed = sidebar(&page(&client, "/admin"));
    let open = sidebar(&page(&client, "/admin/ng-general/new"));
    unregister_runtime("ng-general").expect("unregister");

    let settings = group(&closed, "/admin/ng-general");
    assert!(
        settings.contains(r#"aria-label="Settings menu""#),
        "{settings}"
    );
    assert!(settings.contains(" hidden>"), "{settings}");
    assert!(settings.contains(&sub_link("/admin/ng-general", "General")));
    let settings = group(&open, "/admin/ng-general");
    assert!(!settings.contains(" hidden>"), "{settings}");
}
