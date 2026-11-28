//! Where a successful save goes, as Filament's `getRedirectUrl()` and the panel's
//! `resourceCreatePageRedirect()` and `resourceEditPageRedirect()`
//! (<https://filamentphp.com/docs/4.x/resources/creating-records> and
//! <https://filamentphp.com/docs/4.x/resources/editing-records>, "Customizing redirects"): a
//! runtime resource's `redirect_after_create` and `redirect_after_update` take
//! `RedirectAfter::Edit` (Filament's default: the new record's edit page after a create, the
//! same page after an update, with the flash message) or `RedirectAfter::List`. A resource that
//! sets neither goes to the list, as in 0.6.7. Runs against the test database for the user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, FieldDef, FieldKindDef, RedirectAfter, RuntimeResource,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::json;

fn resource(slug: &str, data: Memory) -> RuntimeResource {
    RuntimeResource::new(slug, "Notes", "Note", data).field(FieldDef {
        name: "title".into(),
        label: "Title".into(),
        kind: FieldKindDef::Text,
        required: true,
    })
}

fn post(client: &Client, path: &str, body: &str) -> (Status, Option<String>) {
    let response = client
        .post(path.to_string())
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    (
        response.status(),
        response.headers().get_one("Location").map(str::to_string),
    )
}

fn page(client: &Client, path: &str) -> String {
    let response = client.get(path.to_string()).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    response.into_string().unwrap()
}

#[test]
fn edit_stays_on_the_edit_page_and_a_create_opens_the_new_record() {
    let _guard = reset();
    let slug = "ras-edit";
    let data = Memory::with(vec![json!({"id": 1, "title": "First"})]);
    let _ = unregister_runtime(slug);
    register_runtime(
        resource(slug, data.clone())
            .redirect_after_create(RedirectAfter::Edit)
            .redirect_after_update(RedirectAfter::Edit),
    )
    .expect("register");
    let client = signed_in(None);

    // An update stays on the record's edit page, which shows the flash message.
    let (status, location) = post(&client, "/admin/ras-edit/1", "title=Changed");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(location.as_deref(), Some("/admin/ras-edit/1/edit"));
    let html = page(&client, "/admin/ras-edit/1/edit");
    assert!(html.contains("Changes saved."), "{html}");
    assert!(html.contains("value=\"Changed\""), "{html}");

    // A create opens the new record's edit page, which shows the flash message.
    let (status, location) = post(&client, "/admin/ras-edit", "title=Second");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(location.as_deref(), Some("/admin/ras-edit/2/edit"));
    let html = page(&client, "/admin/ras-edit/2/edit");
    assert!(html.contains("Record created."), "{html}");
    assert!(html.contains("value=\"Second\""), "{html}");

    // A dashboard form widget still comes back to the dashboard.
    let (_, location) = post(&client, "/admin/ras-edit", "_back=%2Fadmin&title=Third");
    assert_eq!(location.as_deref(), Some("/admin"));

    unregister_runtime(slug).expect("unregister");
}

#[test]
fn list_goes_to_the_list_page() {
    let _guard = reset();
    let slug = "ras-list";
    let data = Memory::with(vec![json!({"id": 1, "title": "First"})]);
    let _ = unregister_runtime(slug);
    register_runtime(
        resource(slug, data)
            .redirect_after_create(RedirectAfter::List)
            .redirect_after_update(RedirectAfter::List),
    )
    .expect("register");
    let client = signed_in(None);

    let (_, location) = post(&client, "/admin/ras-list/1", "title=Changed");
    assert_eq!(location.as_deref(), Some("/admin/ras-list"));
    let (_, location) = post(&client, "/admin/ras-list", "title=Second");
    assert_eq!(location.as_deref(), Some("/admin/ras-list"));

    unregister_runtime(slug).expect("unregister");
}

#[test]
fn a_resource_that_sets_nothing_keeps_going_to_the_list() {
    let _guard = reset();
    let slug = "ras-default";
    let data = Memory::with(vec![json!({"id": 1, "title": "First"})]);
    let _ = unregister_runtime(slug);
    register_runtime(resource(slug, data)).expect("register");
    let client = signed_in(None);

    let (_, location) = post(&client, "/admin/ras-default/1", "title=Changed");
    assert_eq!(location.as_deref(), Some("/admin/ras-default"));
    let (_, location) = post(&client, "/admin/ras-default", "title=Second");
    assert_eq!(location.as_deref(), Some("/admin/ras-default"));

    // A static resource keeps going to the list too.
    let (_, location) = post(&client, "/admin/widgets", "name=Static&price=3");
    assert_eq!(location.as_deref(), Some("/admin/widgets"));

    unregister_runtime(slug).expect("unregister");
}

#[test]
fn a_refused_save_still_shows_the_form_with_its_errors() {
    let _guard = reset();
    let slug = "ras-refused";
    let data = Memory::with(vec![json!({"id": 1, "title": "First"})]);
    let _ = unregister_runtime(slug);
    register_runtime(resource(slug, data).redirect_after_update(RedirectAfter::Edit))
        .expect("register");
    let client = signed_in(None);

    let response = client
        .post("/admin/ras-refused/1")
        .header(ContentType::Form)
        .body(with_token(&client, "title="))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Title is required."));

    unregister_runtime(slug).expect("unregister");
}

#[test]
fn the_builder_records_the_choice() {
    let built = resource("ras-builder", Memory::default());
    assert_eq!(built.redirect_after_create, None);
    assert_eq!(built.redirect_after_update, None);
    let built = built
        .redirect_after_create(RedirectAfter::Edit)
        .redirect_after_update(RedirectAfter::List);
    assert_eq!(built.redirect_after_create, Some(RedirectAfter::Edit));
    assert_eq!(built.redirect_after_update, Some(RedirectAfter::List));
}
