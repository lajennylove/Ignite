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

/// Three saved posts: a draft, a published post with a slug, and a published post with no
/// slug yet. A save that validates fails, so the tests see only the forms.
struct SlugRows;

fn slug_row(id: i64) -> Option<Row> {
    let (title, slug, status) = match id {
        1 => ("Plan", "plan", "draft"),
        2 => ("Ship", "ship-it", "publish"),
        3 => ("Polish", "", "publish"),
        _ => return None,
    };
    serde_json::json!({"id": id, "title": title, "slug": slug, "body": "", "status": status})
        .as_object()
        .cloned()
}

impl RuntimeData for SlugRows {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async {
            Ok(Page {
                rows: Vec::new(),
                total: 0,
            })
        })
    }

    fn find<'a>(&'a self, _: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async move { Ok(slug_row(id)) })
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

/// Posts whose slug stops following the title once the stored post is published.
fn published_posts(slug: &str) -> RuntimeResource {
    let mut resource = posts(slug).slug_locked_when("slug", |form| {
        form.record()
            .and_then(|record| record.get("status"))
            .and_then(|status| status.as_str())
            == Some("publish")
    });
    resource.data = std::sync::Arc::new(SlugRows);
    resource
}

/// The slug input of a form page.
fn slug_input(body: &str) -> &str {
    let start = body
        .find(r#"<input class="ig-input" id="slug""#)
        .expect("the slug input");
    let end = body[start..].find('>').unwrap() + start + 1;
    &body[start..end]
}

#[test]
fn a_saved_published_slug_no_longer_follows_the_title() {
    let _guard = reset();
    let _ = unregister_runtime("slug-locked");
    register_runtime(published_posts("slug-locked")).expect("register");
    let client = signed_in(None);

    // On create, the slug follows the title.
    let body = get(&client, "/admin/slug-locked/new");
    assert_eq!(
        slug_input(&body),
        r#"<input class="ig-input" id="slug" name="slug" type="text"  value="" data-ignite-slug-from="title">"#
    );
    // A draft's slug still follows it.
    let body = get(&client, "/admin/slug-locked/1/edit");
    assert_eq!(
        slug_input(&body),
        r#"<input class="ig-input" id="slug" name="slug" type="text"  value="plan" data-ignite-slug-from="title">"#
    );
    // A published post's stored slug does not.
    let body = get(&client, "/admin/slug-locked/2/edit");
    assert_eq!(
        slug_input(&body),
        r#"<input class="ig-input" id="slug" name="slug" type="text"  value="ship-it" data-ignite-slug-from="title" data-ignite-slug-locked>"#
    );
    // The script leaves a locked slug alone.
    let source = get(&client, &form_script(&body).expect("the form script"));
    assert!(source.contains("data-ignite-slug-locked"), "{source}");
    // A published post without a slug yet gets one from its title.
    let body = get(&client, "/admin/slug-locked/3/edit");
    assert!(
        !slug_input(&body).contains("data-ignite-slug-locked"),
        "{body}"
    );

    // A failed save shows the form again, still locked: the stored post decides.
    let response = client
        .post("/admin/slug-locked/2")
        .header(ContentType::Form)
        .body(with_token(&client, "title=&slug=ship-it&body="))
        .dispatch();
    let body = response.into_string().unwrap();
    assert!(body.contains("is required"), "{body}");
    assert!(
        slug_input(&body).contains("data-ignite-slug-locked"),
        "{body}"
    );

    unregister_runtime("slug-locked").expect("unregister");
}

#[test]
fn without_a_lock_rule_a_published_slug_follows_as_before() {
    let _guard = reset();
    let _ = unregister_runtime("slug-open");
    let mut resource = posts("slug-open");
    resource.data = std::sync::Arc::new(SlugRows);
    register_runtime(resource).expect("register");
    let client = signed_in(None);
    let body = get(&client, "/admin/slug-open/2/edit");
    assert!(!body.contains("data-ignite-slug-locked"), "{body}");
    assert!(body.contains(r#"data-ignite-slug-from="title""#), "{body}");
    unregister_runtime("slug-open").expect("unregister");
}
