//! A badge inside another column's cell, shown only on some tabs: a post's status as a badge
//! after its title, on the "All" tab only. The panel renders it on the server: on another tab
//! the badge is not in the page, and the status has no column of its own on any tab. Runs
//! against the test database.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, BadgeColor, ColumnBadges, ColumnDef, Database, FieldDef,
    FieldKindDef, IgniteResource, ListQuery, Page, Row, RuntimeData, RuntimeFuture,
    RuntimeResource, Tab,
};
use ignite_panel::Ignite;
use orbit_orm::OrbitModel;
use rocket::local::blocking::Client;
use serde::Serialize;
use serde_json::json;

/// A post whose status shows as a badge in the title cell, on the All tab only.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "title_posts")]
#[ignite(table = title_posts, title = "Posts", tabs = post_tabs, badges = post_badges)]
pub struct TitlePost {
    id: i32,
    #[orbit(fillable)]
    title: String,
    #[orbit(fillable)]
    status: String,
    #[orbit(fillable)]
    author: String,
}

fn post_tabs() -> Vec<Tab> {
    vec![
        Tab::make("all"),
        Tab::make("drafts").where_eq("status", "draft"),
        Tab::make("published").where_eq("status", "publish"),
    ]
}

fn post_badges() -> ColumnBadges {
    ColumnBadges::new()
        .badge_color("status", |state| match state {
            "draft" => BadgeColor::Gray,
            _ => BadgeColor::Success,
        })
        .badge_inside("status", "title")
        .badge_on_tabs("status", ["all"])
}

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    let t = test_db();
    t.exec("DROP TABLE IF EXISTS title_posts");
    t.exec(
        "CREATE TABLE title_posts (id INT AUTO_INCREMENT PRIMARY KEY, title VARCHAR(255) NOT NULL, \
         status VARCHAR(40) NOT NULL, author VARCHAR(40) NOT NULL)",
    );
    t.exec(
        "INSERT INTO title_posts (title, status, author) VALUES ('Plan', 'draft', 'Ada'), \
         ('Ship', 'publish', 'Grace'), ('<i>Raw</i>', '<b>odd</b>', 'Linus')",
    );
    guard
}

fn client() -> Client {
    signed_in_with(|ignite: Ignite| ignite.resource::<TitlePost>())
}

fn body(client: &Client, path: &str) -> String {
    client.get(path).dispatch().into_string().unwrap()
}

/// The table's head row.
fn head(page: &str) -> &str {
    let start = page.find("<thead>").expect("the table head");
    let end = page[start..].find("</thead>").unwrap() + start;
    &page[start..end]
}

#[test]
fn on_the_all_tab_the_status_is_a_badge_in_the_title_cell() {
    let _guard = setup();
    let client = client();
    let page = body(&client, "/admin/title_posts");
    assert!(
        page.contains(r#"<td>Plan <span class="ig-badge ig-badge-gray">draft</span></td>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<td>Ship <span class="ig-badge ig-badge-success">publish</span></td>"#),
        "{page}"
    );
    // Both values are escaped.
    assert!(page.contains(
        r#"<td>&lt;i&gt;Raw&lt;/i&gt; <span class="ig-badge ig-badge-success">&lt;b&gt;odd&lt;/b&gt;</span></td>"#
    ));
    // The status has no column of its own: no heading, and no cell of its own.
    let head = head(&page);
    assert!(head.contains("Title") && head.contains("Author"), "{head}");
    assert!(!head.contains("Status"), "{head}");
    assert!(!page.contains("<td>draft</td>"), "{page}");
    assert!(page.contains("<td>Ada</td>"), "{page}");
}

#[test]
fn on_another_tab_the_badge_is_not_rendered() {
    let _guard = setup();
    let client = client();
    for tab in ["drafts", "published"] {
        let page = body(&client, &format!("/admin/title_posts?tab={tab}"));
        assert!(!page.contains("ig-badge ig-badge-"), "{tab}:\n{page}");
        assert!(!head(&page).contains("Status"), "{tab}");
        assert!(!page.contains("<td>draft</td>") && !page.contains("<td>publish</td>"));
    }
    let page = body(&client, "/admin/title_posts?tab=drafts");
    assert!(page.contains("<td>Plan</td>"), "{page}");
    // An unknown tab is no tab: the badge shows only on the tabs listed.
    let page = body(&client, "/admin/title_posts?tab=nothing");
    assert!(!page.contains("ig-badge ig-badge-"), "{page}");
}

/// In-memory posts: a status badge column shown on the All tab only, and a kind badge in the
/// title cell on every tab.
struct Notes;

fn notes() -> Vec<Row> {
    [(1, "Plan", "draft", "news"), (2, "Ship", "publish", "")]
        .into_iter()
        .map(|(id, title, status, kind)| {
            json!({"id": id, "title": title, "status": status, "kind": kind})
                .as_object()
                .cloned()
                .unwrap()
        })
        .collect()
}

impl RuntimeData for Notes {
    fn list<'a>(&'a self, _: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move {
            let rows: Vec<Row> = notes()
                .into_iter()
                .filter(|row| query.filter.matches(row))
                .collect();
            let total = rows.len() as i64;
            let rows = if query.per_page == 0 {
                Vec::new()
            } else {
                rows
            };
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

#[test]
fn a_runtime_resource_shows_badges_by_tab_and_inside_cells() {
    let _guard = reset();
    let _ = unregister_runtime("title_notes");
    let column = |key: &str, label: &str| ColumnDef {
        key: key.into(),
        label: label.into(),
        sortable: false,
        searchable: false,
    };
    register_runtime(
        RuntimeResource::new("title_notes", "Notes", "Note", Notes)
            .field(FieldDef {
                name: "title".into(),
                label: "Title".into(),
                kind: FieldKindDef::Text,
                required: true,
            })
            .column(column("title", "Title"))
            .column(column("status", "Status"))
            .tab(Tab::make("all"))
            .tab(Tab::make("drafts").where_eq("status", "draft"))
            .badge("status")
            .badge_on_tabs("status", ["all"])
            .badge_color("kind", |_| BadgeColor::Info)
            .badge_inside("kind", "title"),
    )
    .expect("register");
    let client = signed_in(None);

    let page = body(&client, "/admin/title_notes");
    assert!(head(&page).contains("Status"), "{page}");
    assert!(
        page.contains(r#"<td><span class="ig-badge ig-badge-primary">draft</span></td>"#),
        "{page}"
    );
    // The kind is a badge in the title cell; an empty kind has no badge.
    assert!(
        page.contains(r#"<td>Plan <span class="ig-badge ig-badge-info">news</span></td>"#),
        "{page}"
    );
    assert!(page.contains("<td>Ship</td>"), "{page}");

    // On Drafts the status column is left out, heading and cells, so no column is empty.
    let page = body(&client, "/admin/title_notes?tab=drafts");
    assert!(!head(&page).contains("Status"), "{page}");
    assert!(!page.contains(">draft</span>"), "{page}");
    assert!(
        page.contains(r#"<td>Plan <span class="ig-badge ig-badge-info">news</span></td>"#),
        "{page}"
    );

    unregister_runtime("title_notes").expect("unregister");
}
