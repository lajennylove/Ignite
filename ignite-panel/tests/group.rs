//! A group edited as a labelled block of sub-fields, as ACF's group field and Filament's
//! `Fieldset`: the form names each input after the group (`seo[title]`), the panel stores the
//! group as one JSON object, a sub-field's error shows under its input, and a group inside a
//! repeater row works the same way (`faqs[0][details][body]`). Runs against the test
//! database for the user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, FieldDef, FieldKindDef, RuntimeResource,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::{json, Value};

const SLUG: &str = "gr-pages";

fn field(name: &str, label: &str, kind: FieldKindDef, required: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind,
        required,
    }
}

/// Pages with a title, an SEO group, and a repeater of questions whose answer is a group.
fn setup(rows: Vec<Value>) -> Memory {
    let data = Memory::with(rows);
    let _ = unregister_runtime(SLUG);
    register_runtime(
        RuntimeResource::new(SLUG, "Pages", "Page", data.clone())
            .field(field("title", "Title", FieldKindDef::Text, true))
            .field(field(
                "seo",
                "SEO",
                FieldKindDef::Group(vec![
                    field("meta_title", "Meta title", FieldKindDef::Text, true),
                    field("description", "Description", FieldKindDef::Textarea, false),
                    field("indexed", "Indexed", FieldKindDef::Checkbox, false),
                ]),
                false,
            ))
            .field(field(
                "faqs",
                "Questions",
                FieldKindDef::Repeater(vec![
                    field("question", "Question", FieldKindDef::Text, true),
                    field(
                        "answer",
                        "Answer",
                        FieldKindDef::Group(vec![field(
                            "body",
                            "Body",
                            FieldKindDef::Textarea,
                            true,
                        )]),
                        false,
                    ),
                ]),
                false,
            ))
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

fn stored(data: &Memory, key: &str) -> Value {
    serde_json::from_str(data.value(1, key).as_str().expect("JSON text")).unwrap()
}

#[test]
fn a_group_renders_as_a_labelled_block_of_its_sub_fields() {
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
        html.contains(r#"data-ignite-field="seo" data-ignite-structure="group""#),
        "{html}"
    );
    assert!(html.contains(r#"<span class="ig-label">SEO</span>"#));
    assert!(html.contains(r#"<input type="hidden" name="seo[_ignite_group]" value="1">"#));
    assert!(html.contains(r#"<label class="ig-label" for="seo[meta_title]">Meta title *</label>"#));
    assert!(html.contains(
        r#"<textarea class="ig-input min-h-32" id="seo[description]" name="seo[description]">"#
    ));
    assert!(html.contains(r#"name="seo[indexed]" value="on""#));
    assert!(!html.contains(r#"value="add:seo""#), "a group adds no rows");
}

#[test]
fn a_group_is_saved_as_one_json_object() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "title=About&seo%5B_ignite_group%5D=1&seo%5Bmeta_title%5D=About+us&seo%5Bdescription%5D=&seo%5Bindexed%5D=on",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther, "{html}");
    assert_eq!(
        stored(&data, "seo"),
        json!({"meta_title": "About us", "description": null, "indexed": true})
    );
}

#[test]
fn a_group_sub_field_error_shows_under_its_input_and_the_edit_form_shows_the_values() {
    let _guard = reset();
    let data = setup(vec![json!({
        "id": 1,
        "title": "About",
        "seo": r#"{"meta_title":"About us","indexed":true}"#,
    })]);
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}/1/edit"))
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        html.contains(r#"name="seo[meta_title]" type="text"  value="About us""#),
        "{html}"
    );
    assert!(html.contains(r#"name="seo[indexed]" value="on" checked"#));

    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}/1"),
        "title=About&seo%5B_ignite_group%5D=1&seo%5Bmeta_title%5D=",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::UnprocessableEntity);
    let input = html.find(r#"id="seo[meta_title]""#).expect(&html);
    let error = html.find(">Meta title is required.</p>").unwrap();
    let next = html
        .find(r#"data-ignite-field="seo[description]""#)
        .unwrap();
    assert!(input < error && error < next, "{html}");
    assert_eq!(
        stored(&data, "seo"),
        json!({"meta_title": "About us", "indexed": true}),
        "nothing saved"
    );
}

#[test]
fn a_group_inside_a_repeater_row_is_saved_in_the_row() {
    let _guard = reset();
    let data = setup(Vec::new());
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        "title=FAQ&faqs=&_ignite_op=add%3Afaqs",
    );
    assert_eq!(status, Status::Ok);
    assert!(
        html.contains(r#"data-ignite-field="faqs[0][answer]" data-ignite-structure="group""#),
        "{html}"
    );
    assert!(html.contains(r#"name="faqs[0][answer][body]""#));

    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "title=FAQ&faqs=&faqs%5B0%5D%5Bquestion%5D=Why%3F&faqs%5B0%5D%5Banswer%5D%5B_ignite_group%5D=1&faqs%5B0%5D%5Banswer%5D%5Bbody%5D=",
    );
    assert_eq!(status, Status::UnprocessableEntity);
    let input = html.find(r#"id="faqs[0][answer][body]""#).expect(&html);
    let error = html[input..].find(">Body is required.</p>");
    assert!(error.is_some(), "{html}");

    let (status, _) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "title=FAQ&faqs=&faqs%5B0%5D%5Bquestion%5D=Why%3F&faqs%5B0%5D%5Banswer%5D%5B_ignite_group%5D=1&faqs%5B0%5D%5Banswer%5D%5Bbody%5D=Because.",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(
        stored(&data, "faqs"),
        json!([{"question": "Why?", "answer": {"body": "Because."}}])
    );
}
