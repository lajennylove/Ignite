//! A checkbox list, as Filament's `CheckboxList` (ACF's checkbox field): several choices shown
//! as checkboxes, stored as a JSON list of the chosen values; a value that is not a choice is
//! refused; a list with no choices of its own loads them each time the form is shown, as
//! Filament's `options(fn () => ...)`; and a checkbox list works inside a repeater row. Runs
//! against the test database for the user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, Database, FieldDef, FieldKindDef, ListQuery,
    Page, Row, RuntimeData, RuntimeFuture, RuntimeResource,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::{json, Value};

const SLUG: &str = "cl-types";

/// The rows in memory, and the taxonomies in use as the loaded choices of `taxonomies`.
#[derive(Clone, Default)]
struct Types(Memory);

impl RuntimeData for Types {
    fn list<'a>(&'a self, db: &'a Database, query: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        self.0.list(db, query)
    }
    fn find<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        self.0.find(db, id)
    }
    fn create<'a>(&'a self, db: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        self.0.create(db, input)
    }
    fn update<'a>(&'a self, db: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row> {
        self.0.update(db, id, input)
    }
    fn delete<'a>(&'a self, db: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        self.0.delete(db, id)
    }
    fn options<'a>(
        &'a self,
        _: &'a Database,
        field: &'a str,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        let choices = match field {
            "taxonomies" => vec![
                ("category".to_string(), "Categories".to_string()),
                ("genre".to_string(), "Genres".to_string()),
            ],
            _ => Vec::new(),
        };
        Box::pin(async move { Ok(choices) })
    }
}

fn field(name: &str, label: &str, kind: FieldKindDef) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind,
        required: false,
    }
}

fn supports() -> FieldKindDef {
    FieldKindDef::CheckboxList(vec![
        ("title".into(), "Title".into()),
        ("editor".into(), "Editor".into()),
        ("thumbnail".into(), "Featured image".into()),
    ])
}

fn setup(rows: Vec<Value>) -> Memory {
    let data = Types(Memory::with(rows));
    let _ = unregister_runtime(SLUG);
    register_runtime(
        RuntimeResource::new(SLUG, "Post types", "Post type", data.clone())
            .field(field("plural", "Plural label", FieldKindDef::Text))
            .field(field("supports", "Supports", supports()))
            .field(field(
                "taxonomies",
                "Taxonomies",
                FieldKindDef::CheckboxList(Vec::new()),
            ))
            .field(field(
                "screens",
                "Screens",
                FieldKindDef::Repeater(vec![field("hide", "Hide on screen", supports())]),
            ))
            .column(ColumnDef {
                key: "plural".into(),
                label: "Plural label".into(),
                sortable: false,
                searchable: false,
            }),
    )
    .expect("register");
    data.0
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

fn list(data: &Memory, key: &str) -> Value {
    match data.value(1, key) {
        Value::String(text) => serde_json::from_str(&text).unwrap(),
        other => other,
    }
}

#[test]
fn the_choices_render_as_checkboxes_with_the_stored_ones_ticked() {
    let _guard = reset();
    setup(vec![json!({
        "id": 1,
        "plural": "Books",
        "supports": r#"["title","thumbnail"]"#,
        "taxonomies": r#"["genre"]"#,
    })]);
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}/1/edit"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(SLUG).expect("unregister");
    assert!(
        html.contains(r#"<input type="hidden" name="supports[]" value="">"#),
        "{html}"
    );
    assert!(html.contains(r#"<div class="grid gap-2" id="supports" role="group">"#));
    let checkbox = |name: &str, value: &str, checked: bool| {
        format!(
            r#"class="h-4 w-4 rounded border-bordergray text-primary focus:ring-primary" name="{name}[]" value="{value}" {}>"#,
            if checked { "checked" } else { "" }
        )
    };
    assert!(
        html.contains(&checkbox("supports", "title", true)),
        "{html}"
    );
    assert!(html.contains(&checkbox("supports", "editor", false)));
    assert!(html.contains(&checkbox("supports", "thumbnail", true)));
    assert!(html.contains("Featured image"));
    // The loaded choices.
    assert!(html.contains(&checkbox("taxonomies", "category", false)));
    assert!(html.contains(&checkbox("taxonomies", "genre", true)));
    assert!(html.contains("Genres"));
}

#[test]
fn the_ticked_values_are_stored_as_a_json_list() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "plural=Books&supports%5B%5D=&supports%5B%5D=title&supports%5B%5D=editor\
         &taxonomies%5B%5D=&taxonomies%5B%5D=genre",
    );
    assert_eq!(status, Status::SeeOther, "{html}");
    assert_eq!(list(&data, "supports"), json!(["title", "editor"]));
    assert_eq!(list(&data, "taxonomies"), json!(["genre"]));

    // Nothing ticked clears the list.
    client.get(format!("/admin/{SLUG}/1/edit")).dispatch();
    let (status, _) = post(
        &client,
        &format!("/admin/{SLUG}/1"),
        "plural=Books&supports%5B%5D=&taxonomies%5B%5D=",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(data.value(1, "supports"), Value::Null);
}

#[test]
fn a_value_that_is_not_a_choice_is_refused() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "plural=Books&supports%5B%5D=title&supports%5B%5D=comments&taxonomies%5B%5D=post_tag",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::UnprocessableEntity);
    assert_eq!(
        html.matches(">Choose one of the options.</p>").count(),
        2,
        "{html}"
    );
    let supports = html.find(r#"id="supports""#).unwrap();
    let taxonomies = html.find(r#"id="taxonomies""#).unwrap();
    let first = html.find(">Choose one of the options.</p>").unwrap();
    assert!(supports < first && first < taxonomies, "{html}");
    assert!(data.rows().is_empty());
}

#[test]
fn a_checkbox_list_works_inside_a_repeater_row() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (_, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        "plural=Books&screens=&_ignite_op=add%3Ascreens",
    );
    assert!(
        html.contains(r#"name="screens[0][hide][]" value="editor""#),
        "{html}"
    );
    let (status, _) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "plural=Books&screens=&screens%5B0%5D%5B_ignite_row%5D=1\
         &screens%5B0%5D%5Bhide%5D%5B%5D=&screens%5B0%5D%5Bhide%5D%5B%5D=editor",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(list(&data, "screens"), json!([{"hide": ["editor"]}]));
}
