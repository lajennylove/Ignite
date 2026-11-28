//! Tabs in a form layout, as Filament's `Tabs` schema component: one card with a tab list of
//! the app's labels and one panel per tab, the first tab open (or the one `active_tab`
//! names), each tab's fields and sections rendered together in its panel, a tab whose fields
//! the conditions all hide left out, and every tab's fields sent and saved with the form. Runs
//! against the test database for the user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::layout::Tab;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, FieldDef, FieldKindDef, FormLayout,
    RuntimeResource, Section, Tabs,
};
use rocket::http::{ContentType, Status};
use serde_json::json;

const SLUG: &str = "tb-types";

fn field(name: &str, label: &str) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind: FieldKindDef::Text,
        required: false,
    }
}

/// Post types whose form has a General, a Labels and an Advanced tab. The Advanced tab's only
/// field is hidden from users who may not `types.advanced`.
fn setup(tabs: Tabs) -> Memory {
    let data = Memory::default();
    let _ = unregister_runtime(SLUG);
    register_runtime(
        RuntimeResource::new(SLUG, "Post types", "Post type", data.clone())
            .field(field("plural", "Plural label"))
            .field(field("description", "Description"))
            .field(field("menu_name", "Menu name"))
            .field(field("rest_base", "REST base"))
            .visible("rest_base", |form| form.user().can("types.advanced"))
            .form_layout(FormLayout::new().schema([tabs]))
            .column(ColumnDef {
                key: "plural".into(),
                label: "Plural label".into(),
                sortable: false,
                searchable: false,
            }),
    )
    .expect("register");
    data
}

fn tabs() -> Tabs {
    Tabs::make("Settings").tabs([
        Tab::make("General").schema(["plural", "description"]),
        Tab::make("Labels").schema([Section::make("Menu").schema(["menu_name"])]),
        Tab::make("Advanced").schema(["rest_base"]),
    ])
}

#[test]
fn the_first_tab_is_open_and_each_tab_renders_its_fields_together() {
    let _guard = reset();
    setup(tabs());
    // A user who may not see the Advanced tab's field: an editor with the resource's abilities.
    let t = test_db();
    t.run(async {
        ignite_panel::authz::create_role(&t.db, "editor")
            .await
            .unwrap();
        ignite_panel::users::create_user(&t.db, "Ed", "ed@example.test", PASSWORD)
            .await
            .unwrap();
        ignite_panel::authz::assign_role(&t.db, "ed@example.test", "editor")
            .await
            .unwrap();
    });
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}/new"))
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        html.contains(r#"role="tablist" aria-label="Settings""#),
        "{html}"
    );
    assert!(html.contains(
        r##"href="#ignite-tabs-1-1" role="tab" id="ignite-tabs-1-1-tab" aria-controls="ignite-tabs-1-1" aria-selected="true" data-ignite-tab="ignite-tabs-1-1">General</a>"##
    ));
    assert!(html.contains(r#"aria-selected="false" data-ignite-tab="ignite-tabs-1-2">Labels</a>"#));
    assert!(html.contains(
        r#"role="tabpanel" id="ignite-tabs-1-1" aria-labelledby="ignite-tabs-1-1-tab">"#
    ));
    assert!(html.contains(
        r#"role="tabpanel" id="ignite-tabs-1-2" aria-labelledby="ignite-tabs-1-2-tab" hidden>"#
    ));
    let general = html.find(r#"id="ignite-tabs-1-1" "#).unwrap();
    let labels = html.find(r#"id="ignite-tabs-1-2" "#).unwrap();
    let plural = html.find(r#"data-ignite-field="plural""#).unwrap();
    let description = html.find(r#"data-ignite-field="description""#).unwrap();
    let menu = html.find(r#"data-ignite-field="menu_name""#).unwrap();
    let heading = html.find(r#"<h3 class="m-0">Menu</h3>"#).unwrap();
    assert!(general < plural && plural < description && description < labels);
    assert!(labels < heading && heading < menu);
    assert!(
        html.contains("Advanced</a>"),
        "a super-admin sees the third tab"
    );
    assert!(
        html.contains("/build/assets/form-"),
        "the form script switches tabs"
    );

    // For a user who sees none of its fields, the Advanced tab is left out.
    t.run(async {
        for ability in ["tb-types.viewAny", "tb-types.create"] {
            ignite_panel::authz::grant_permission(&t.db, "editor", ability)
                .await
                .unwrap();
        }
    });
    let editor = signed_in_as(None, "ed@example.test", PASSWORD);
    let html = editor
        .get(format!("/admin/{SLUG}/new"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(SLUG).expect("unregister");
    assert!(html.contains("Labels</a>"), "{html}");
    assert!(!html.contains("Advanced</a>"));
    assert!(!html.contains(r#"data-ignite-field="rest_base""#));
}

#[test]
fn active_tab_opens_another_tab() {
    let _guard = reset();
    setup(tabs().active_tab(2));
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}/new"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(SLUG).expect("unregister");
    assert!(html.contains(r#"aria-selected="false" data-ignite-tab="ignite-tabs-1-1">General</a>"#));
    assert!(html.contains(r#"aria-selected="true" data-ignite-tab="ignite-tabs-1-2">Labels</a>"#));
    assert!(html.contains(r#"id="ignite-tabs-1-1" aria-labelledby="ignite-tabs-1-1-tab" hidden>"#));
}

#[test]
fn a_save_sends_the_fields_of_every_tab() {
    let _guard = reset();
    let data = setup(tabs());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let status = client
        .post(format!("/admin/{SLUG}"))
        .header(ContentType::Form)
        .body(with_token(
            &client,
            "plural=Books&description=All+books&menu_name=Books&rest_base=books",
        ))
        .dispatch()
        .status();
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    let row = &data.rows()[0];
    assert_eq!(row["plural"], json!("Books"));
    assert_eq!(row["menu_name"], json!("Books"));
    assert_eq!(row["rest_base"], json!("books"));
}
