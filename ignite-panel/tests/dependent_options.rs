//! Dependent options, as Filament's dependent select: a field's choices are a function of the
//! form's current values (`RuntimeData::options_with`, Filament's `options(fn (Get $get) =>
//! ...)`), shown again when the live field they depend on changes, and checked on save
//! against the values sent. Inside a repeater each row's choices follow that row's values, as
//! `$get` reads a row's siblings: Mission Control's location rules, whose value depends on the
//! rule's parameter. Runs against the test database for the user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ActingUser, ColumnDef, Database, FieldDef, FieldKindDef,
    ListQuery, Page, Row, RuntimeData, RuntimeFuture, RuntimeResource,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::{json, Value};

const SLUG: &str = "do-places";

/// The rows in memory; the cities of the chosen country, and each rule's values for its
/// parameter.
#[derive(Clone, Default)]
struct Places(Memory);

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(value, label)| (value.to_string(), label.to_string()))
        .collect()
}

impl RuntimeData for Places {
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
    fn options_with<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        field: &'a str,
        state: &'a Row,
    ) -> RuntimeFuture<'a, Vec<(String, String)>> {
        let get = |key: &str| state.get(key).and_then(Value::as_str).unwrap_or("");
        let choices = match (field, get("country"), get("param")) {
            ("city", "fr", _) => pairs(&[("paris", "Paris"), ("lyon", "Lyon")]),
            ("city", "jp", _) => pairs(&[("tokyo", "Tokyo")]),
            ("rules.value", _, "post_type") => pairs(&[("book", "Book"), ("page", "Page")]),
            ("rules.value", _, "page_template") => pairs(&[("wide", "Wide")]),
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

fn setup() -> Memory {
    let data = Places::default();
    let _ = unregister_runtime(SLUG);
    let country = FieldKindDef::Select(vec![
        ("fr".into(), "France".into()),
        ("jp".into(), "Japan".into()),
    ]);
    let param = FieldKindDef::Select(vec![
        ("post_type".into(), "Post type".into()),
        ("page_template".into(), "Page template".into()),
    ]);
    register_runtime(
        RuntimeResource::new(SLUG, "Places", "Place", data.clone())
            .field(field("name", "Name", FieldKindDef::Text))
            .field(field("country", "Country", country))
            .field(field("city", "City", FieldKindDef::Lookup))
            .field(field(
                "rules",
                "Rules",
                FieldKindDef::Repeater(vec![
                    field("param", "Parameter", param),
                    field("value", "Value", FieldKindDef::Lookup),
                ]),
            ))
            .live("country")
            .live("rules.param")
            .column(ColumnDef {
                key: "name".into(),
                label: "Name".into(),
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

#[test]
fn the_choices_follow_the_live_field_they_depend_on() {
    let _guard = reset();
    setup();
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}/new"))
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        html.contains(r#"<div data-ignite-field="country" data-ignite-live>"#),
        "{html}"
    );
    assert!(
        !html.contains(r#"<option value="paris""#),
        "no country, no cities"
    );

    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        "name=Home&country=fr&city=&rules=&_ignite_shown=name%2Ccountry%2Ccity%2Crules",
    );
    assert_eq!(status, Status::Ok);
    assert!(
        html.contains(r#"<option value="paris" >Paris</option>"#),
        "{html}"
    );
    assert!(html.contains(r#"<option value="lyon" >Lyon</option>"#));
    assert!(!html.contains(r#"<option value="tokyo""#));

    // The country changes: Paris is no longer a choice, so nothing is chosen.
    let (_, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        "name=Home&country=jp&city=paris&rules=&_ignite_shown=name%2Ccountry%2Ccity%2Crules",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert!(
        html.contains(r#"<option value="tokyo" >Tokyo</option>"#),
        "{html}"
    );
    assert!(!html.contains(r#"value="paris""#));
}

#[test]
fn a_save_checks_the_value_against_the_choices_for_the_values_sent() {
    let _guard = reset();
    let data = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "name=Home&country=jp&city=paris&rules=",
    );
    assert_eq!(status, Status::UnprocessableEntity);
    let city = html.find(r#"data-ignite-field="city""#).unwrap();
    let error = html.find(">Choose one of the options.</p>").expect(&html);
    assert!(city < error, "{html}");
    assert!(data.rows().is_empty());

    let (status, _) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "name=Home&country=fr&city=paris&rules=",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(data.rows()[0]["city"], json!("paris"));
}

#[test]
fn each_repeater_row_has_the_choices_of_its_own_values() {
    let _guard = reset();
    let data = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let rows = "rules=&rules%5B0%5D%5Bparam%5D=post_type&rules%5B0%5D%5Bvalue%5D=book\
                &rules%5B1%5D%5Bparam%5D=page_template&rules%5B1%5D%5Bvalue%5D=book";
    let (_, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        &format!("name=Home&{rows}&_ignite_op=add%3Arules"),
    );
    assert!(
        html.contains(r#"<div data-ignite-field="rules[0][param]" data-ignite-live>"#),
        "{html}"
    );
    let first = html.find(r#"id="rules[0][value]""#).unwrap();
    let second = html.find(r#"id="rules[1][value]""#).unwrap();
    let book = html
        .find(r#"<option value="book" selected>Book</option>"#)
        .unwrap();
    let wide = html.find(r#"<option value="wide" >Wide</option>"#).unwrap();
    assert!(first < book && book < second && second < wide, "{html}");

    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        &format!("name=Home&{rows}"),
    );
    assert_eq!(status, Status::UnprocessableEntity);
    let input = html.find(r#"id="rules[1][value]""#).unwrap();
    let error = html[input..].find(">Choose one of the options.</p>");
    assert!(error.is_some(), "{html}");
    assert!(data.rows().is_empty());

    let rows = rows.replace(
        "rules%5B1%5D%5Bvalue%5D=book",
        "rules%5B1%5D%5Bvalue%5D=wide",
    );
    let (status, _) = post(
        &client,
        &format!("/admin/{SLUG}"),
        &format!("name=Home&{rows}"),
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    let stored: Value = serde_json::from_str(data.rows()[0]["rules"].as_str().unwrap()).unwrap();
    assert_eq!(
        stored,
        json!([{"param": "post_type", "value": "book"}, {"param": "page_template", "value": "wide"}])
    );
}
