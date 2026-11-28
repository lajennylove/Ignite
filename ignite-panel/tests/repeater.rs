//! A repeater edited as rows of sub-fields, as Filament's `Repeater`: the form names each
//! sub-field input after its row (`members[0][name]`), the panel stores the rows as one JSON
//! list, the add, delete and move buttons show the form again through its live URL without
//! saving, `min_items` and `max_items` bound the rows, `item_label` labels each row, and a
//! sub-field's error shows under the row's input. Runs against the test database for the user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, FieldDef, FieldKindDef, RepeaterSettings,
    RuntimeResource,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::{json, Value};

const SLUG: &str = "rp-teams";

fn field(name: &str, label: &str, kind: FieldKindDef, required: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind,
        required,
    }
}

/// Teams with a name and a repeater of members: a required name, a role and an admin box.
fn setup(rows: Vec<Value>) -> Memory {
    let data = Memory::with(rows);
    let _ = unregister_runtime(SLUG);
    let role = FieldKindDef::Select(vec![
        ("dev".into(), "Developer".into()),
        ("pm".into(), "Manager".into()),
    ]);
    register_runtime(
        RuntimeResource::new(SLUG, "Teams", "Team", data.clone())
            .field(field("title", "Title", FieldKindDef::Text, true))
            .field(field(
                "members",
                "Members",
                FieldKindDef::Repeater(vec![
                    field("name", "Name", FieldKindDef::Text, true),
                    field("role", "Role", role, false),
                    field("admin", "Admin", FieldKindDef::Checkbox, false),
                ]),
                false,
            ))
            .repeater(
                "members",
                RepeaterSettings::new()
                    .min_items(1)
                    .max_items(3)
                    .item_label("name")
                    .add_action_label("Add member"),
            )
            .column(ColumnDef {
                key: "title".into(),
                label: "Title".into(),
                sortable: false,
                searchable: false,
            }),
    )
    .expect("register");
    data
}

fn post(client: &Client, path: &str, body: &str) -> (Status, String) {
    let response = client
        .post(path)
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    let status = response.status();
    (status, response.into_string().unwrap_or_default())
}

fn members(data: &Memory, id: i64) -> Value {
    serde_json::from_str(data.value(id, "members").as_str().expect("JSON text")).unwrap()
}

#[test]
fn the_create_form_renders_the_repeater_with_its_add_button() {
    let _guard = reset();
    setup(Vec::new());
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}/new"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(SLUG).expect("unregister");
    assert!(
        html.contains(r#"data-ignite-field="members" data-ignite-structure="repeater""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<input type="hidden" name="members" value="">"#),
        "{html}"
    );
    assert!(html.contains(r#"formaction="/admin/rp-teams/new/live""#));
    assert!(html.contains(r#"value="add:members" data-ignite-op>Add member</button>"#));
    assert!(!html.contains("data-ignite-structured"), "no JSON textarea");
    assert!(
        html.contains("/build/assets/form-"),
        "the form script and styles load"
    );
}

#[test]
fn the_add_button_shows_a_blank_row_and_saves_nothing() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        "title=Core&members=&members%5B0%5D%5B_ignite_row%5D=1&members%5B0%5D%5Bname%5D=Ada&_ignite_op=add%3Amembers",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::Ok);
    assert!(data.rows().is_empty(), "nothing saved");
    assert!(
        html.contains(r#"name="members[0][name]" type="text"  value="Ada""#),
        "{html}"
    );
    assert!(html.contains(r#"name="members[1][name]" type="text"  value="""#));
    assert!(html.contains(r#"<span class="font-semibold text-heading text-sm">Ada</span>"#));
    assert!(html.contains(r#"<span class="font-semibold text-heading text-sm">Row 2</span>"#));
    assert!(html.contains(r#"value="delete:members.1""#));
    assert!(html.contains(r#"value="up:members.1""#));
    assert!(
        !html.contains(r#"value="up:members.0""#),
        "the first row cannot move up"
    );
}

#[test]
fn rows_are_saved_as_one_json_list_in_their_order() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "title=Core&members=\
         &members%5B1%5D%5B_ignite_row%5D=1&members%5B1%5D%5Bname%5D=Bo&members%5B1%5D%5Brole%5D=pm\
         &members%5B0%5D%5B_ignite_row%5D=1&members%5B0%5D%5Bname%5D=Ada&members%5B0%5D%5Badmin%5D=on",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther, "{html}");
    assert_eq!(
        members(&data, 1),
        json!([
            {"name": "Ada", "admin": true},
            {"name": "Bo", "role": "pm", "admin": false}
        ])
    );
}

#[test]
fn a_sub_field_error_shows_under_the_rows_input() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "title=Core&members=&members%5B0%5D%5B_ignite_row%5D=1&members%5B0%5D%5Bname%5D=Ada\
         &members%5B1%5D%5B_ignite_row%5D=1&members%5B1%5D%5Bname%5D=&members%5B1%5D%5Brole%5D=ceo",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(data.rows().is_empty());
    let name = html
        .find(r#"data-ignite-field="members[1][name]""#)
        .expect(&html);
    let role = html
        .find(r#"data-ignite-field="members[1][role]""#)
        .unwrap();
    let required = html.find(">Name is required.</p>").unwrap();
    let choice = html.find(">Choose one of the options.</p>").unwrap();
    assert!(
        name < required && required < role && role < choice,
        "{html}"
    );
    assert!(
        html.contains(">Row 2: Name is required.</p>"),
        "the repeater says which row: {html}"
    );
    assert!(html.contains(r#"value="Ada""#), "the typed rows are kept");
}

#[test]
fn min_and_max_items_bound_the_rows() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(&client, &format!("/admin/{SLUG}"), "title=Core&members=");
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(
        html.contains(">Members must have at least 1 items.</p>"),
        "{html}"
    );

    let rows: String = (0..4)
        .map(|i| format!("&members%5B{i}%5D%5Bname%5D=M{i}"))
        .collect();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        &format!("title=Core&members={rows}"),
    );
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(html.contains(">Members must not have more than 3 items.</p>"));

    // At three rows the add button is gone, and the add operation adds nothing.
    let three: String = (0..3)
        .map(|i| format!("&members%5B{i}%5D%5Bname%5D=M{i}"))
        .collect();
    let (_, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        &format!("title=Core&members={three}&_ignite_op=add%3Amembers"),
    );
    unregister_runtime(SLUG).expect("unregister");
    assert!(!html.contains(r#"name="members[3][name]""#), "{html}");
    assert!(!html.contains("Add member</button>"));
    assert!(data.rows().is_empty());
}

#[test]
fn the_edit_form_shows_the_stored_rows_and_moves_them() {
    let _guard = reset();
    let data = setup(vec![json!({
        "id": 1,
        "title": "Core",
        "members": r#"[{"name":"Ada","admin":true},{"name":"Bo","role":"pm"}]"#,
    })]);
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}/1/edit"))
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        html.contains(r#"name="members[0][name]" type="text"  value="Ada""#),
        "{html}"
    );
    assert!(html.contains(r#"name="members[0][admin]" value="on" checked"#));
    assert!(html.contains(r#"<option value="pm" selected>Manager</option>"#));
    assert!(html.contains(r#"formaction="/admin/rp-teams/1/edit/live""#));

    let (_, html) = post(
        &client,
        &format!("/admin/{SLUG}/1/edit/live"),
        "title=Core&members=&members%5B0%5D%5Bname%5D=Ada&members%5B0%5D%5Badmin%5D=on\
         &members%5B1%5D%5Bname%5D=Bo&members%5B1%5D%5Brole%5D=pm&_ignite_op=up%3Amembers.1",
    );
    let bo = html.find(r#"name="members[0][name]" type="text"  value="Bo""#);
    assert!(bo.is_some(), "Bo moved up: {html}");
    assert!(html.contains(r#"name="members[1][admin]" value="on" checked"#));

    // Saving the moved rows stores them in their new order; deleting one leaves the other.
    let (status, _) = post(
        &client,
        &format!("/admin/{SLUG}/1"),
        "title=Core&members=&members%5B0%5D%5Bname%5D=Bo&members%5B1%5D%5Bname%5D=Ada",
    );
    assert_eq!(status, Status::SeeOther);
    assert_eq!(
        members(&data, 1),
        json!([{"name": "Bo", "admin": false}, {"name": "Ada", "admin": false}])
    );
    let (_, html) = post(
        &client,
        &format!("/admin/{SLUG}/1/edit/live"),
        "title=Core&members=&members%5B0%5D%5Bname%5D=Bo&members%5B1%5D%5Bname%5D=Ada&_ignite_op=delete%3Amembers.0",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert!(html.contains(r#"name="members[0][name]" type="text"  value="Ada""#));
    assert!(!html.contains(r#"name="members[1][name]""#));
}

#[test]
fn a_repeater_sent_as_json_text_still_saves() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, _) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "title=Core&members=%5B%7B%22name%22%3A%22Ada%22%7D%5D",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(members(&data, 1), json!([{"name": "Ada"}]));
}
