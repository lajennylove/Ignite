//! Dashboard widgets that render their own view, as Filament's custom widgets: a list widget
//! (WordPress's "Activity" and "Recently Published") and a form widget (WordPress's "Quick
//! Draft"). Every value is escaped, an unsafe link is not linked, and the 0.6.6 stat cards stay
//! as they were. Runs against the `ignite_test` database.

mod common;

use common::*;
use ignite_core::{
    Direction, FormWidget, ListFilter, ListQuery, ListWidget, ListWidgetRow, Page, Row,
    RuntimeData, RuntimeFuture,
};
use ignite_panel::{authz, users};
use orbit_orm::Database;
use rocket::http::{ContentType, Status};
use serde_json::json;

/// Rows kept in memory, filtered by `status` and sorted by `id`, as a runtime resource's data.
struct Posts;

fn posts() -> Vec<Row> {
    [
        (1, "First post", "publish"),
        (2, "A <b>bold</b> draft", "draft"),
        (3, "Third & last", "publish"),
    ]
    .into_iter()
    .map(|(id, title, status)| {
        json!({"id": id, "title": title, "status": status})
            .as_object()
            .unwrap()
            .clone()
    })
    .collect()
}

impl RuntimeData for Posts {
    fn list<'a>(&'a self, _: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async move {
            let mut rows: Vec<Row> = posts()
                .into_iter()
                .filter(|row| query.filter.matches(row))
                .collect();
            if matches!(query.sort, Some((_, Direction::Desc))) {
                rows.reverse();
            }
            let total = rows.len() as i64;
            rows.truncate(query.per_page as usize);
            Ok(Page { rows, total })
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

fn recently_published() -> ListWidget {
    ListWidget::make("Recently Published").rows_from(
        std::sync::Arc::new(Posts),
        ListQuery {
            page: 1,
            per_page: 5,
            search: None,
            sort: Some(("id".into(), Direction::Desc)),
            filter: ListFilter::new().where_eq("status", "publish"),
        },
        |row| {
            ListWidgetRow::make(row["title"].as_str().unwrap_or_default())
                .value(row["status"].as_str().unwrap_or_default())
                .url(format!("/admin/posts/{}/edit", row["id"]))
        },
    )
}

fn activity() -> ListWidget {
    ListWidget::make("Activity <now>").rows(|_, user| {
        let name = user.name().to_string();
        Box::pin(async move {
            Ok(vec![
                ListWidgetRow::make(format!("Signed in as {name}")),
                ListWidgetRow::make("<script>alert(1)</script>").url("javascript:alert(1)"),
                ListWidgetRow::make("Docs").url("https://example.test/?a=1&b=\"2\""),
            ])
        })
    })
}

fn quick_draft() -> FormWidget {
    FormWidget::make("Quick Draft", "widgets")
        .fields(["name", "note"])
        .hidden("price", "5")
        .submit_label("Save Draft")
}

fn body(client: &rocket::local::blocking::Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok);
    response.into_string().unwrap()
}

#[test]
fn a_list_widget_shows_its_rows_escaped_after_the_cards() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        ignite
            .list_widget(activity())
            .list_widget(recently_published())
            .list_widget(ListWidget::make("Empty box").empty_state("No comments yet."))
    });
    let html = body(&client, "/admin");

    // The 0.6.6 cards are unchanged, and come first.
    let cards = html.find("ig-tone-primary").expect("the stat card");
    let first = html
        .find("Activity &lt;now&gt;")
        .expect("the escaped title");
    assert!(cards < first);
    assert!(html.contains("Signed in as Admin"));

    // Escaped, and a script URL is not linked.
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<script>alert(1)"));
    assert!(!html.contains("javascript:"));
    assert!(html.contains("href=\"https://example.test/?a=1&amp;b=&quot;2&quot;\""));

    // Rows from a RuntimeData, filtered and sorted by its query.
    let third = html.find("Third &amp; last").expect("third post");
    let first_post = html.find("First post").expect("first post");
    assert!(third < first_post, "newest first");
    assert!(!html.contains("A &lt;b&gt;bold"), "a draft is filtered out");
    assert!(html.contains("href=\"/admin/posts/3/edit\""));
    assert!(html.contains("<span class=\"shrink-0 text-sm\">publish</span>"));

    // The empty state.
    assert!(html.contains("Empty box"));
    assert!(html.contains("No comments yet."));
}

#[test]
fn a_list_widget_shows_only_to_the_users_its_rule_allows() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        ignite.list_widget(ListWidget::make("Secret box").can_view(|user| !user.is_super_admin()))
    });
    assert!(!body(&client, "/admin").contains("Secret box"));
}

#[test]
fn a_form_widget_creates_a_record_and_comes_back_to_the_dashboard() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.form_widget(quick_draft()));
    let html = body(&client, "/admin");
    assert!(html.contains("Quick Draft"));
    assert!(html.contains("action=\"/admin/widgets\""));
    assert!(html.contains("name=\"_back\" value=\"/admin\""));
    assert!(html.contains("name=\"price\" value=\"5\""));
    assert!(html.contains("name=\"name\""));
    assert!(html.contains("name=\"note\""));
    assert!(!html.contains("id=\"price\""), "only the named fields show");
    assert!(html.contains("Save Draft"));

    let before = count_widgets();
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            "_back=%2Fadmin&price=5&name=Quick+one&note=Hello",
        ))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin"));
    assert_eq!(count_widgets(), before + 1);
    assert!(body(&client, "/admin").contains("Record created."));

    // Any other `_back` is not followed.
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            "_back=https%3A%2F%2Fevil.test&price=5&name=Other",
        ))
        .dispatch();
    assert_eq!(
        response.headers().get_one("Location"),
        Some("/admin/widgets")
    );

    // A form that does not validate shows the resource's form with its errors.
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(&client, "_back=%2Fadmin&price=5&name="))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
}

#[test]
fn a_form_widget_shows_only_to_a_user_who_may_create() {
    let _guard = reset();
    let t = test_db();
    t.run(async {
        users::create_user(&t.db, "Viewer", "viewer@example.test", PASSWORD)
            .await
            .unwrap();
        authz::create_role(&t.db, "viewer").await.unwrap();
    });
    let client = client_with(|ignite| ignite.form_widget(quick_draft()));
    // The panel registers the permissions at start; the client is built, so grant now.
    t.run(async {
        authz::grant_permission(&t.db, "viewer", "widgets.viewAny")
            .await
            .unwrap();
        authz::assign_role(&t.db, "viewer@example.test", "viewer")
            .await
            .unwrap();
    });
    client.get("/admin/login").dispatch();
    client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email=viewer@example.test&password={PASSWORD}"),
        ))
        .dispatch();
    let html = body(&client, "/admin");
    assert!(!html.contains("Quick Draft"));
}

#[test]
fn a_form_widget_of_an_unknown_resource_shows_nothing() {
    let _guard = reset();
    let client =
        signed_in_with(|ignite| ignite.form_widget(FormWidget::make("Nowhere", "no-such")));
    assert!(!body(&client, "/admin").contains("Nowhere"));
}
