//! The form script: unsaved changes alerts, as Filament's `unsavedChangesAlerts()`, and slug
//! fields, as Filament's slug pattern. A form page loads `resources/js/form.js` when its form
//! asks before it is left or has a slug field; other pages do not load it. Runs against the
//! test database, for the signed-in user.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, Database, FieldDef, FieldKindDef, ListQuery,
    Page, Row, RuntimeData, RuntimeFuture, RuntimeResource,
};
use ignite_panel::Ignite;
use orbit_orm::Error as OrbitError;
use rocket::http::ContentType;
use rocket::local::blocking::Client;

/// The question the script asks before the user leaves a changed form.
const MESSAGE: &str =
    "Are you sure you want to leave? Without saving, everything is going to be lost.";

/// Rows that are never written: a save that validates fails, and the tests only render forms
/// and the empty list.
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
        Box::pin(async { Err(OrbitError::NotFound { table: "none" }) })
    }

    fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Err(OrbitError::NotFound { table: "none" }) })
    }

    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }
}

/// A resource at `slug` with a required title and a slug filled from it.
fn posts(slug: &str) -> RuntimeResource {
    let field = |name: &str, required| FieldDef {
        name: name.into(),
        label: name.into(),
        kind: FieldKindDef::Text,
        required,
    };
    RuntimeResource::new(slug, "Posts", "Post", NoRows)
        .field(field("title", true))
        .field(field("slug", false))
        .field(field("body", false))
        .column(ColumnDef {
            key: "title".into(),
            label: "Title".into(),
            sortable: false,
            searchable: false,
        })
        .slug_from("slug", "title")
}

fn get(client: &Client, path: &str) -> String {
    client.get(path).dispatch().into_string().unwrap()
}

/// The URL of the form script a page loads, if it loads one.
fn form_script(body: &str) -> Option<String> {
    let start = body.find("/admin/build/assets/form-")?;
    let end = start + body[start..].find('"')?;
    let url = &body[start..end];
    url.ends_with(".js").then(|| url.to_string())
}

#[test]
fn a_form_asks_before_it_is_left_when_the_panel_has_unsaved_changes_alerts() {
    let _guard = reset();
    let client = signed_in_with(Ignite::unsaved_changes_alerts);

    let body = get(&client, "/admin/widgets/new");
    assert!(
        body.contains(r#"action="/admin/widgets" data-ignite-unsaved-changes-alert>"#),
        "{body}"
    );
    assert!(!body.contains("data-ignite-unsaved>"), "not changed yet");
    let script = form_script(&body).expect("the form script");
    let source = get(&client, &script);
    assert!(source.contains(MESSAGE), "{source}");
    assert!(source.contains("beforeunload"));
    assert!(source.contains("data-ignite-unsaved-changes-alert"));

    let body = get(&client, "/admin/widgets/1/edit");
    assert!(body.contains("data-ignite-unsaved-changes-alert"), "{body}");
    assert!(form_script(&body).is_some());

    // Pages without a form load neither the script nor the mark.
    for page in ["/admin", "/admin/widgets", "/admin/profile"] {
        let body = get(&client, page);
        assert!(form_script(&body).is_none(), "{page}");
        assert!(!body.contains("data-ignite-unsaved"), "{page}");
    }
}

#[test]
fn a_form_shown_again_after_a_failed_save_counts_as_changed() {
    let _guard = reset();
    let client = signed_in_with(Ignite::unsaved_changes_alerts);

    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(&client, "name=&price=12&note=kept"))
        .dispatch();
    let body = response.into_string().unwrap();
    assert!(body.contains("Name is required."), "{body}");
    assert!(
        body.contains("data-ignite-unsaved-changes-alert data-ignite-unsaved>"),
        "{body}"
    );
}

#[test]
fn without_unsaved_changes_alerts_a_form_is_unchanged() {
    let _guard = reset();
    let client = signed_in(None);

    let body = get(&client, "/admin/widgets/new");
    assert!(!body.contains("data-ignite-unsaved"), "{body}");
    assert!(form_script(&body).is_none());
}

#[test]
fn a_slug_field_names_the_field_it_is_filled_from() {
    let _guard = reset();
    let _ = unregister_runtime("slug-posts");
    register_runtime(posts("slug-posts")).expect("register");
    let client = signed_in(None);

    let body = get(&client, "/admin/slug-posts/new");
    assert!(
        body.contains(
            r#"<input class="ig-input" id="slug" name="slug" type="text"  value="" data-ignite-slug-from="title">"#
        ),
        "{body}"
    );
    assert_eq!(body.matches("data-ignite-slug-from").count(), 1);
    // The slug needs the form script, even without unsaved changes alerts.
    let script = form_script(&body).expect("the form script");
    let source = get(&client, &script);
    assert!(source.contains("igniteSlugFrom"), "{source}");
    assert!(!body.contains("data-ignite-unsaved-changes-alert"));

    unregister_runtime("slug-posts").expect("unregister");
}
