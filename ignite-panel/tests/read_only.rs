//! Read-only records, as Filament's View page (a disabled form with the record's data): a
//! resource's `read_only_when` rule makes a record read-only for the signed-in user. Its edit
//! form shows the values with every input disabled, a "View" heading, the resource's notice
//! and no Save; the list's button says "View"; and its save and its live URL answer 403.
//! Mission Control's post types defined in code. Runs against the test database for the user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, FieldDef, FieldKindDef, RuntimeResource,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::{json, Value};

const SLUG: &str = "ro-types";

fn field(name: &str, label: &str) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind: FieldKindDef::Text,
        required: true,
    }
}

/// Post types: Books was made on the admin screens, Products is defined in code.
fn setup() -> Memory {
    let data = Memory::with(vec![
        json!({"id": 1, "label": "Books", "origin": "admin"}),
        json!({"id": 2, "label": "Products", "origin": "code"}),
    ]);
    let _ = unregister_runtime(SLUG);
    let in_code = |form: &ignite_core::FormContext<'_>| {
        form.record().and_then(|record| record.get("origin")) != Some(&json!("admin"))
    };
    register_runtime(
        RuntimeResource::new(SLUG, "Post types", "Post type", data.clone())
            .field(field("label", "Plural label"))
            .read_only_when(in_code)
            .read_only_notice(move |form| {
                in_code(form).then(|| {
                    "This post type is defined in code by the theme. Change it there.".to_string()
                })
            })
            .column(ColumnDef {
                key: "label".into(),
                label: "Plural label".into(),
                sortable: false,
                searchable: false,
            }),
    )
    .expect("register");
    data
}

fn get(client: &Client, path: &str) -> String {
    client.get(path).dispatch().into_string().unwrap()
}

fn post(client: &Client, path: &str, body: &str) -> Status {
    client
        .post(path)
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch()
        .status()
}

#[test]
fn the_list_offers_view_for_a_read_only_record() {
    let _guard = reset();
    setup();
    let client = signed_in(None);
    let html = get(&client, &format!("/admin/{SLUG}"));
    unregister_runtime(SLUG).expect("unregister");
    assert!(
        html.contains(r#"href="/admin/ro-types/1/edit">Edit</a>"#),
        "{html}"
    );
    assert!(html.contains(r#"href="/admin/ro-types/2/edit">View</a>"#));
}

#[test]
fn a_read_only_record_shows_its_values_disabled_with_no_save() {
    let _guard = reset();
    setup();
    let client = signed_in(None);
    let html = get(&client, &format!("/admin/{SLUG}/2/edit"));
    unregister_runtime(SLUG).expect("unregister");
    assert!(
        html.contains(r#"<h1 class="mb-6">View Post type #2</h1>"#),
        "{html}"
    );
    assert!(html.contains(
        r#"<p class="mb-6 text-sm">This post type is defined in code by the theme. Change it there.</p>"#
    ));
    assert!(
        html.contains(r#"<fieldset disabled style="border:0;margin:0;padding:0;min-width:0"><div"#)
    );
    assert!(html.contains(r#"value="Products""#));
    assert!(!html.contains(">Save</button>"));
    assert!(html.contains(r#"href="/admin/ro-types">Back</a>"#));
    assert!(!html.contains("data-ignite-live-url"));
}

#[test]
fn a_read_only_record_refuses_its_save_and_its_live_form_with_403() {
    let _guard = reset();
    let data = setup();
    let client = signed_in(None);
    get(&client, &format!("/admin/{SLUG}/2/edit"));
    assert_eq!(
        post(&client, &format!("/admin/{SLUG}/2"), "label=Goods"),
        Status::Forbidden
    );
    assert_eq!(
        post(
            &client,
            &format!("/admin/{SLUG}/2/edit/live"),
            "label=Goods"
        ),
        Status::Forbidden
    );
    assert_eq!(data.value(2, "label"), Value::from("Products"));

    // Another record of the resource is edited and saved as before.
    let html = get(&client, &format!("/admin/{SLUG}/1/edit"));
    assert!(html.contains(">Save</button>"), "{html}");
    assert!(!html.contains("<fieldset disabled"));
    assert!(!html.contains("defined in code"));
    assert_eq!(
        post(&client, &format!("/admin/{SLUG}/1"), "label=Novels"),
        Status::SeeOther
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(data.value(1, "label"), Value::from("Novels"));
}
