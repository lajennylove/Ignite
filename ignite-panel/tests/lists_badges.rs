//! Column badges, as Filament's `badge()` and `color()` on a text column: a badge column shows
//! its value in a badge whose colour the value picks, with the value escaped, and a plain
//! column renders as before. Runs against the test database.

mod common;

use common::snapshots::{assert_snapshot, page};
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, BadgeColor, ColumnBadges, ColumnDef, Database, FieldDef,
    FieldKindDef, IgniteResource, ListQuery, Page, Row, RuntimeData, RuntimeFuture,
    RuntimeResource,
};
use ignite_panel::Ignite;
use orbit_orm::OrbitModel;
use serde::Serialize;
use serde_json::json;

/// An article, derived, whose status shows as a coloured badge and whose kind as a badge in
/// the primary colour.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "badge_articles")]
#[ignite(table = badge_articles, title = "Articles", badges = article_badges)]
pub struct BadgeArticle {
    id: i32,
    #[orbit(fillable)]
    title: String,
    #[orbit(fillable)]
    status: String,
    #[orbit(fillable)]
    kind: Option<String>,
}

fn article_badges() -> ColumnBadges {
    ColumnBadges::new()
        .badge_color("status", |state| match state {
            "draft" => BadgeColor::Gray,
            "review" => BadgeColor::Warning,
            "publish" => BadgeColor::Success,
            _ => BadgeColor::Danger,
        })
        .badge("kind")
}

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    let t = test_db();
    t.exec("DROP TABLE IF EXISTS badge_articles");
    t.exec(
        "CREATE TABLE badge_articles (id INT AUTO_INCREMENT PRIMARY KEY, title VARCHAR(255) NOT NULL, \
         status VARCHAR(40) NOT NULL, kind VARCHAR(40) NULL)",
    );
    t.exec(
        "INSERT INTO badge_articles (title, status, kind) VALUES ('Plan', 'draft', 'News'), \
         ('Ship', 'publish', NULL), ('Hack', '<b>&gone</b>', 'Note')",
    );
    guard
}

fn list() -> String {
    let client = signed_in_with(|ignite: Ignite| ignite.resource::<BadgeArticle>());
    client
        .get("/admin/badge_articles")
        .dispatch()
        .into_string()
        .unwrap()
}

#[test]
fn a_badge_cell_has_the_badge_class_and_the_colour_its_value_picks() {
    let _guard = setup();
    let page = list();
    assert!(page.contains("<td><span class=\"ig-badge ig-badge-gray\">draft</span></td>"));
    assert!(page.contains("<td><span class=\"ig-badge ig-badge-success\">publish</span></td>"));
    // A badge without a colour is primary, as in Filament.
    assert!(page.contains("<td><span class=\"ig-badge ig-badge-primary\">News</span></td>"));
    assert!(
        page.contains("/admin/build/assets/list-"),
        "a list with badges loads the list stylesheet"
    );
}

#[test]
fn a_badge_cell_escapes_its_value() {
    let _guard = setup();
    let page = list();
    assert!(page.contains(
        "<td><span class=\"ig-badge ig-badge-danger\">&lt;b&gt;&amp;gone&lt;/b&gt;</span></td>"
    ));
    assert!(!page.contains("<b>&gone</b>"));
}

#[test]
fn a_plain_cell_and_an_empty_badge_cell_are_plain_text() {
    let _guard = setup();
    let page = list();
    assert!(
        page.contains("<td>Plan</td>"),
        "the title column is not a badge"
    );
    // An empty value has no badge, as Filament shows nothing for a blank state.
    assert_eq!(
        cells(&page, "<td>Ship</td>"),
        [
            "<td>2</td>",
            "<td>Ship</td>",
            "<td><span class=\"ig-badge ig-badge-success\">publish</span></td>",
            "<td></td>",
        ]
    );
}

/// The data cells of the table row that contains `marker`, without the actions cell.
fn cells(page: &str, marker: &str) -> Vec<String> {
    let at = page.find(marker).expect("the row");
    let start = page[..at].rfind("<tr>").expect("the row's start");
    let end = at + page[at..].find("<td class=").expect("the actions cell");
    page[start..end]
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("<td>"))
        .map(str::to_string)
        .collect()
}

#[test]
fn a_list_without_badges_renders_as_before() {
    let _guard = reset();
    let client = signed_in(None);
    let html = page(&client, "/admin/widgets");
    assert!(!html.contains("ig-badge-"));
    assert!(!html.contains("/admin/build/assets/list-"));
    assert_snapshot("index", &html);
}

/// A runtime resource's rows, in memory.
struct Tasks;

impl RuntimeData for Tasks {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move {
            let row = json!({"id": 1, "title": "Write", "state": "late"})
                .as_object()
                .cloned()
                .unwrap();
            Ok(Page {
                rows: vec![row],
                total: 1,
            })
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
fn a_runtime_resource_shows_badge_columns() {
    let _guard = reset();
    let column = |key: &str, label: &str| ColumnDef {
        key: key.into(),
        label: label.into(),
        sortable: false,
        searchable: false,
    };
    register_runtime(
        RuntimeResource::new("badge_tasks", "Tasks", "Task", Tasks)
            .field(FieldDef {
                name: "title".into(),
                label: "Title".into(),
                kind: FieldKindDef::Text,
                required: true,
            })
            .column(column("title", "Title"))
            .column(column("state", "State"))
            .badge_color("state", |state| {
                if state == "late" {
                    BadgeColor::Danger
                } else {
                    BadgeColor::Info
                }
            }),
    )
    .expect("register");
    let client = signed_in(None);
    let page = client
        .get("/admin/badge_tasks")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(page.contains("<td>Write</td>"));
    assert!(page.contains("<td><span class=\"ig-badge ig-badge-danger\">late</span></td>"));
    unregister_runtime("badge_tasks").expect("unregister");
}
