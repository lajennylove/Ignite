//! The tabs of a list page, as Filament's `getTabs()`: each tab shows its label and the number
//! of rows it matches for the current search, the `tab` query parameter chooses one, and its
//! conditions restrict the rows, beside the search, sort and page. A resource without tabs
//! renders as before. Runs against the test database.

mod common;

use common::snapshots::{assert_snapshot, page};
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

/// A post, derived, with an "All", a "Drafts" and a "Published" tab.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "list_posts")]
#[ignite(table = list_posts, title = "Posts", tabs = post_tabs)]
pub struct ListPost {
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
    ]
}

/// Resets the shared tables and seeds five posts: three drafts and two published.
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    let t = test_db();
    t.exec("DROP TABLE IF EXISTS list_posts");
    t.exec(
        "CREATE TABLE list_posts (id INT AUTO_INCREMENT PRIMARY KEY, title VARCHAR(255) NOT NULL, \
         status VARCHAR(20) NOT NULL)",
    );
    t.exec(
        "INSERT INTO list_posts (title, status) VALUES ('Alpha', 'draft'), ('Beta', 'publish'), \
         ('Gamma', 'draft'), ('Alphabet', 'publish'), ('Delta', 'draft')",
    );
    guard
}

/// The panel with the posts resource beside the widgets, signed in.
fn client() -> Client {
    signed_in_with(|ignite: Ignite| ignite.resource::<ListPost>())
}

fn body(client: &Client, path: &str) -> String {
    client.get(path).dispatch().into_string().unwrap()
}

/// The markup of a tab link.
fn tab_link(url: &str, label: &str, count: i64, active: bool) -> String {
    if active {
        format!(
            "<a class=\"ig-tab ig-tab-active\" href=\"{url}\" aria-current=\"page\">{label} <span class=\"ig-tab-count\">({count})</span></a>"
        )
    } else {
        format!(
            "<a class=\"ig-tab\" href=\"{url}\">{label} <span class=\"ig-tab-count\">({count})</span></a>"
        )
    }
}

/// The titles of the rows on a list page, in order.
fn titles(body: &str) -> Vec<String> {
    let all = ["Alpha", "Alphabet", "Beta", "Gamma", "Delta"];
    let mut found: Vec<(usize, String)> = all
        .iter()
        .filter_map(|title| {
            body.find(&format!("<td>{title}</td>"))
                .map(|at| (at, title.to_string()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, title)| title).collect()
}

#[test]
fn the_tabs_render_with_their_labels_and_counts_and_the_first_is_active() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/list_posts");
    assert!(page.contains("<nav class=\"ig-tabs\" aria-label=\"Posts\">"));
    assert!(page.contains(&tab_link("/admin/list_posts?tab=all", "All", 5, true)));
    assert!(page.contains(&tab_link(
        "/admin/list_posts?tab=drafts",
        "Drafts",
        3,
        false
    )));
    assert!(page.contains(&tab_link(
        "/admin/list_posts?tab=published",
        "Published",
        2,
        false
    )));
    assert!(
        page.contains("/admin/build/assets/list-"),
        "a list with tabs loads the list stylesheet"
    );
    assert_eq!(titles(&page).len(), 5, "the first tab, All, has every row");
}

#[test]
fn the_tab_parameter_marks_its_tab_and_restricts_the_rows() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/list_posts?tab=drafts");
    assert!(page.contains(&tab_link("/admin/list_posts?tab=all", "All", 5, false)));
    assert!(page.contains(&tab_link("/admin/list_posts?tab=drafts", "Drafts", 3, true)));
    assert_eq!(titles(&page), ["Alpha", "Gamma", "Delta"]);
    assert!(page.contains("3 in total"));

    let page = body(&client, "/admin/list_posts?tab=published");
    assert_eq!(titles(&page), ["Beta", "Alphabet"]);
}

#[test]
fn the_counts_are_the_rows_each_tab_shows_for_the_current_search() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/list_posts?q=Alph&tab=drafts");
    assert!(page.contains(&tab_link(
        "/admin/list_posts?q=Alph&amp;tab=all",
        "All",
        2,
        false
    )));
    assert!(page.contains(&tab_link(
        "/admin/list_posts?q=Alph&amp;tab=drafts",
        "Drafts",
        1,
        true
    )));
    assert!(page.contains(&tab_link(
        "/admin/list_posts?q=Alph&amp;tab=published",
        "Published",
        1,
        false
    )));
    assert_eq!(titles(&page), ["Alpha"]);

    // Each tab's count is the number of rows the tab lists for the same search.
    for (tab, count) in [("all", 2), ("drafts", 1), ("published", 1)] {
        let page = body(&client, &format!("/admin/list_posts?q=Alph&tab={tab}"));
        assert_eq!(titles(&page).len(), count, "the rows of the tab {tab}");
    }
}

#[test]
fn the_search_sort_and_page_keep_the_tab() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/list_posts?tab=drafts&sort=title&dir=desc");
    assert_eq!(
        titles(&page),
        ["Gamma", "Delta", "Alpha"],
        "sorted inside the tab"
    );
    // The search form sends the tab, the sort links keep it, and the tab links keep the sort
    // and go back to page 1.
    assert!(page.contains("<input type=\"hidden\" name=\"tab\" value=\"drafts\">"));
    assert!(page.contains("href=\"/admin/list_posts?tab=drafts&amp;sort=title&amp;dir=asc\""));
    assert!(page.contains(&tab_link(
        "/admin/list_posts?tab=published&amp;sort=title&amp;dir=desc",
        "Published",
        2,
        false
    )));
    let page = body(&client, "/admin/list_posts?tab=drafts&q=a");
    assert_eq!(titles(&page), ["Alpha", "Gamma", "Delta"]);
}

#[test]
fn an_unknown_tab_shows_every_row_and_no_active_tab() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/list_posts?tab=nothing");
    assert_eq!(titles(&page).len(), 5);
    assert!(!page.contains("ig-tab-active"));
}

#[test]
fn a_resource_without_tabs_renders_as_before() {
    let _guard = reset();
    let client = signed_in(None);
    assert_snapshot("index", &page(&client, "/admin/widgets"));
    // The parameter means nothing to a list without tabs.
    assert_snapshot("index", &page(&client, "/admin/widgets?tab=drafts"));
}

/// The rows of a runtime resource, in memory: the source applies the tab's conditions with
/// `ListFilter::matches` and the search to the title.
struct Notes;

fn notes() -> Vec<Row> {
    [
        (1, "Plan", "draft"),
        (2, "Ship", "publish"),
        (3, "Polish", "draft"),
    ]
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
    fn find<'a>(&'a self, _: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async move { Ok(notes().into_iter().find(|r| r["id"] == json!(id))) })
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

#[test]
fn a_runtime_resource_has_tabs_and_a_default_tab() {
    let _guard = reset();
    register_runtime(
        RuntimeResource::new("tab_notes", "Notes", "Note", Notes)
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
            .tab(
                Tab::make("drafts")
                    .label("My drafts")
                    .where_eq("status", "draft"),
            )
            .default_active_tab("drafts"),
    )
    .expect("register");
    let client = signed_in(None);

    let page = body(&client, "/admin/tab_notes");
    assert!(page.contains(&tab_link("/admin/tab_notes?tab=all", "All", 3, false)));
    assert!(page.contains(&tab_link(
        "/admin/tab_notes?tab=drafts",
        "My drafts",
        2,
        true
    )));
    assert!(page.contains("<td>Plan</td>") && page.contains("<td>Polish</td>"));
    assert!(
        !page.contains("<td>Ship</td>"),
        "the default tab filters the rows"
    );

    let page = body(&client, "/admin/tab_notes?tab=all&q=P");
    assert!(page.contains(&tab_link(
        "/admin/tab_notes?q=P&amp;tab=all",
        "All",
        2,
        true
    )));
    assert!(page.contains(&tab_link(
        "/admin/tab_notes?q=P&amp;tab=drafts",
        "My drafts",
        2,
        false
    )));

    unregister_runtime("tab_notes").expect("unregister");
}
