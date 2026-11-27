//! The counts of a list page's tabs: each tab counts all its rows, whatever the search, as a
//! Filament tab's `badge()` counts its own query, unless the resource's counts follow the
//! search; and a tab whose count is 0 is left out, except the default tab and the tab shown,
//! unless the resource renders every tab. Runs against the test database.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, Database, FieldDef, FieldKindDef,
    IgniteResource, ListQuery, Page, Row, RuntimeData, RuntimeFuture, RuntimeResource, Tab,
};
use ignite_panel::Ignite;
use orbit_orm::OrbitModel;
use rocket::local::blocking::Client;
use serde::Serialize;
use serde_json::json;

/// Posts with the default options: counts for all rows, empty tabs left out.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "count_posts")]
#[ignite(table = count_posts, title = "Posts", tabs = post_tabs)]
pub struct CountPost {
    id: i32,
    #[orbit(fillable)]
    #[ignite(searchable, sortable)]
    title: String,
    #[orbit(fillable)]
    status: String,
}

/// The same posts, with counts that follow the search.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "count_searched_posts")]
#[ignite(
    table = count_searched_posts,
    title = "Searched posts",
    tabs = post_tabs,
    tab_counts_follow_search = true
)]
pub struct SearchedPost {
    id: i32,
    #[orbit(fillable)]
    #[ignite(searchable, sortable)]
    title: String,
    #[orbit(fillable)]
    status: String,
}

/// The same posts, with every tab rendered.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "count_every_posts")]
#[ignite(
    table = count_every_posts,
    title = "Every tab",
    tabs = post_tabs,
    hide_empty_tabs = false
)]
pub struct EveryTabPost {
    id: i32,
    #[orbit(fillable)]
    #[ignite(searchable, sortable)]
    title: String,
    #[orbit(fillable)]
    status: String,
}

fn post_tabs() -> Vec<Tab> {
    vec![
        Tab::make("all"),
        Tab::make("drafts").where_eq("status", "draft"),
        Tab::make("published").where_eq("status", "publish"),
        Tab::make("trash").where_eq("status", "trash"),
    ]
}

/// Seeds three drafts and two published posts, and none in the trash.
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    let t = test_db();
    for table in ["count_posts", "count_searched_posts", "count_every_posts"] {
        t.exec(format!("DROP TABLE IF EXISTS {table}"));
        t.exec(format!(
            "CREATE TABLE {table} (id INT AUTO_INCREMENT PRIMARY KEY, title VARCHAR(255) NOT NULL, \
             status VARCHAR(20) NOT NULL)"
        ));
        t.exec(format!(
            "INSERT INTO {table} (title, status) VALUES ('Alpha', 'draft'), ('Beta', 'publish'), \
             ('Gamma', 'draft'), ('Alphabet', 'publish'), ('Delta', 'draft')"
        ));
    }
    guard
}

fn client() -> Client {
    signed_in_with(|ignite: Ignite| {
        ignite
            .resource::<CountPost>()
            .resource::<SearchedPost>()
            .resource::<EveryTabPost>()
    })
}

fn body(client: &Client, path: &str) -> String {
    client.get(path).dispatch().into_string().unwrap()
}

/// The label and count of the tab `label`, as the page renders them, if it renders the tab.
fn count(page: &str, label: &str) -> Option<i64> {
    let needle = format!(">{label} <span class=\"ig-tab-count\">(");
    let start = page.find(&needle)? + needle.len();
    let end = start + page[start..].find(')')?;
    Some(page[start..end].parse().unwrap())
}

#[test]
fn without_a_search_the_counts_are_the_tabs_rows() {
    let _guard = setup();
    let client = client();
    for path in ["/admin/count_posts", "/admin/count_posts?tab=drafts"] {
        let page = body(&client, path);
        assert_eq!(count(&page, "All"), Some(5), "{path}");
        assert_eq!(count(&page, "Drafts"), Some(3), "{path}");
        assert_eq!(count(&page, "Published"), Some(2), "{path}");
    }
}

#[test]
fn by_default_the_counts_ignore_the_search() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/count_posts?q=Alph");
    assert_eq!(count(&page, "All"), Some(5));
    assert_eq!(count(&page, "Drafts"), Some(3));
    assert_eq!(count(&page, "Published"), Some(2));
    // The list itself is searched: two rows match.
    assert!(page.contains("2 in total"), "{page}");

    // A search that matches nothing changes no count and hides no tab.
    let page = body(&client, "/admin/count_posts?q=Zeta&tab=published");
    assert_eq!(count(&page, "Drafts"), Some(3));
    assert_eq!(count(&page, "Published"), Some(2));
    assert!(page.contains("0 in total"), "{page}");
}

#[test]
fn counts_can_follow_the_search() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/count_posts?q=Alph");
    assert_eq!(count(&page, "Drafts"), Some(3), "the default ignores it");
    let page = body(&client, "/admin/count_posts?q=Alph&tab=drafts");
    assert_eq!(count(&page, "Drafts"), Some(3));

    // `tab_counts_follow_search = true` counts the rows that match, as Ignite 0.6.4 did.
    assert!(<SearchedPost as IgniteResource>::tab_counts_follow_search());
    assert!(!<CountPost as IgniteResource>::tab_counts_follow_search());
    let page = body(&client, "/admin/count_searched_posts?q=Alph&tab=drafts");
    assert_eq!(count(&page, "All"), Some(2), "{page}");
    assert_eq!(count(&page, "Drafts"), Some(1), "{page}");
    assert_eq!(count(&page, "Published"), Some(1), "{page}");
    let page = body(&client, "/admin/count_searched_posts");
    assert_eq!(count(&page, "Drafts"), Some(3), "{page}");
}

#[test]
fn an_empty_tab_is_left_out_unless_it_is_shown() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/count_posts");
    assert_eq!(
        count(&page, "Trash"),
        None,
        "no post is in the trash:\n{page}"
    );
    assert!(!page.contains("tab=trash"), "{page}");

    // The tab being shown renders, even when it is empty.
    let page = body(&client, "/admin/count_posts?tab=trash");
    assert_eq!(count(&page, "Trash"), Some(0), "{page}");
    assert!(page.contains("ig-tab ig-tab-active\" href=\"/admin/count_posts?tab=trash\""));

    // Once it holds a row, it renders.
    test_db().exec("UPDATE count_posts SET status = 'trash' WHERE title = 'Delta'");
    let page = body(&client, "/admin/count_posts");
    assert_eq!(count(&page, "Trash"), Some(1), "{page}");
    assert_eq!(count(&page, "Drafts"), Some(2));
}

#[test]
fn hide_empty_tabs_false_renders_every_tab() {
    let _guard = setup();
    assert!(!<EveryTabPost as IgniteResource>::hide_empty_tabs());
    assert!(<CountPost as IgniteResource>::hide_empty_tabs());
    let client = client();
    let page = body(&client, "/admin/count_every_posts");
    assert_eq!(count(&page, "Trash"), Some(0), "{page}");
    assert!(
        page.contains("href=\"/admin/count_every_posts?tab=trash\""),
        "{page}"
    );
}

/// In-memory notes whose source applies the tab's conditions and the search.
struct Notes;

fn notes() -> Vec<Row> {
    [(1, "Plan", "draft"), (2, "Ship", "publish")]
        .into_iter()
        .map(|(id, title, status)| {
            json!({"id": id, "title": title, "status": status})
                .as_object()
                .cloned()
                .unwrap()
        })
        .collect()
}

impl RuntimeData for Notes {
    fn list<'a>(&'a self, _: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move {
            let matching: Vec<Row> = notes()
                .into_iter()
                .filter(|row| query.filter.matches(row))
                .filter(|row| match &query.search {
                    Some(search) => row["title"].as_str().unwrap().contains(search.as_str()),
                    None => true,
                })
                .collect();
            let total = matching.len() as i64;
            let rows = matching
                .into_iter()
                .skip(query.offset() as usize)
                .take(query.per_page as usize)
                .collect();
            Ok(Page { rows, total })
        })
    }
    fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async { Ok(None) })
    }
    fn create<'a>(&'a self, _: &'a Database, _: Row) -> RuntimeFuture<'a, Row> {
        unreachable!()
    }
    fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
        unreachable!()
    }
    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        unreachable!()
    }
}

fn notes_resource(slug: &str) -> RuntimeResource {
    RuntimeResource::new(slug, "Notes", "Note", Notes)
        .field(FieldDef {
            name: "title".into(),
            label: "Title".into(),
            kind: FieldKindDef::Text,
            required: true,
        })
        .column(ColumnDef {
            key: "title".into(),
            label: "Title".into(),
            sortable: false,
            searchable: true,
        })
        .tab(Tab::make("all"))
        .tab(Tab::make("drafts").where_eq("status", "draft"))
        .tab(Tab::make("pending").where_eq("status", "pending"))
        .tab(Tab::make("trash").where_eq("status", "trash"))
}

#[test]
fn a_runtime_resource_takes_both_options() {
    let _guard = reset();
    for slug in [
        "count_notes",
        "count_searched",
        "count_every",
        "count_default",
    ] {
        let _ = unregister_runtime(slug);
    }
    register_runtime(notes_resource("count_notes")).expect("register");
    register_runtime(notes_resource("count_searched").tab_counts_follow_search(true))
        .expect("register");
    register_runtime(notes_resource("count_every").hide_empty_tabs(false)).expect("register");
    // The default tab is empty: it renders all the same.
    register_runtime(notes_resource("count_default").default_active_tab("pending"))
        .expect("register");
    let client = signed_in(None);

    let page = body(&client, "/admin/count_notes?q=Ship");
    assert_eq!(count(&page, "All"), Some(2), "{page}");
    assert_eq!(count(&page, "Drafts"), Some(1), "{page}");
    assert_eq!(count(&page, "Pending"), None, "{page}");

    let page = body(&client, "/admin/count_searched?q=Ship");
    assert_eq!(count(&page, "All"), Some(1), "{page}");
    // No draft matches the search: its count is 0, so the tab is left out.
    assert_eq!(count(&page, "Drafts"), None, "{page}");
    let page = body(&client, "/admin/count_searched");
    assert_eq!(count(&page, "Drafts"), Some(1), "{page}");

    let page = body(&client, "/admin/count_every");
    assert_eq!(count(&page, "Pending"), Some(0), "{page}");
    assert_eq!(count(&page, "Trash"), Some(0), "{page}");

    let page = body(&client, "/admin/count_default");
    assert_eq!(count(&page, "Pending"), Some(0), "the default tab:\n{page}");
    assert_eq!(count(&page, "Trash"), None, "{page}");

    for slug in [
        "count_notes",
        "count_searched",
        "count_every",
        "count_default",
    ] {
        unregister_runtime(slug).expect("unregister");
    }
}
