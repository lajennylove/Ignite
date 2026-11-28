//! Flexible content edited as rows that each take one block, as Filament's `Builder` (an ACF
//! flexible content field): the add button opens a picker of the blocks, each row keeps its
//! block in `content[0][type]` and its values in `content[0][data][...]`, the panel stores the
//! rows as Filament stores a builder (`{"type": ..., "data": {...}}`), a block's sub-field
//! error shows under its input, and a repeater inside a block has its own buttons. It runs on
//! the same rows, buttons and settings as the repeater. Runs against the test database for the
//! user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, BlockDef, ColumnDef, FieldDef, FieldKindDef,
    RepeaterSettings, RuntimeResource,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::{json, Value};

const SLUG: &str = "fx-pages";

fn field(name: &str, label: &str, kind: FieldKindDef, required: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind,
        required,
    }
}

/// Pages whose content takes a heading block or a gallery block with a repeater of items.
fn setup() -> Memory {
    let data = Memory::default();
    let _ = unregister_runtime(SLUG);
    let level = FieldKindDef::Select(vec![("h2".into(), "H2".into()), ("h3".into(), "H3".into())]);
    register_runtime(
        RuntimeResource::new(SLUG, "Pages", "Page", data.clone())
            .field(field("title", "Title", FieldKindDef::Text, true))
            .field(field(
                "content",
                "Content",
                FieldKindDef::FlexibleContent(vec![
                    BlockDef {
                        name: "heading".into(),
                        label: "Heading".into(),
                        fields: vec![
                            field("text", "Text", FieldKindDef::Text, true),
                            field("level", "Level", level, false),
                        ],
                    },
                    BlockDef {
                        name: "gallery".into(),
                        label: "Gallery".into(),
                        fields: vec![
                            field("caption", "Caption", FieldKindDef::Text, false),
                            field(
                                "items",
                                "Items",
                                FieldKindDef::Repeater(vec![field(
                                    "url",
                                    "URL",
                                    FieldKindDef::Url,
                                    true,
                                )]),
                                false,
                            ),
                        ],
                    },
                ]),
                false,
            ))
            .repeater("content", RepeaterSettings::new().max_items(2))
            .repeater(
                "content.gallery.items",
                RepeaterSettings::new().add_action_label("Add image"),
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

#[test]
fn the_add_button_opens_a_picker_of_the_blocks() {
    let _guard = reset();
    setup();
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}/new"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(SLUG).expect("unregister");
    assert!(
        html.contains(r#"data-ignite-field="content" data-ignite-structure="flexible_content""#),
        "{html}"
    );
    assert!(html.contains(r#"<summary class="ig-btn ig-btn-outline">Add to Content</summary>"#));
    assert!(html.contains(r#"value="add:content:heading" data-ignite-op>Heading</button>"#));
    assert!(html.contains(r#"value="add:content:gallery" data-ignite-op>Gallery</button>"#));
}

#[test]
fn a_picked_block_shows_its_own_sub_fields_and_a_nested_repeater_adds_rows() {
    let _guard = reset();
    let data = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        "title=Home&content=&_ignite_op=add%3Acontent%3Agallery",
    );
    assert_eq!(status, Status::Ok);
    assert!(
        html.contains(r#"<input type="hidden" name="content[0][type]" value="gallery">"#),
        "{html}"
    );
    assert!(html.contains(r#"<span class="font-semibold text-heading text-sm">Gallery</span>"#));
    assert!(html.contains(r#"name="content[0][data][caption]""#));
    assert!(html.contains(r#"value="add:content.0.data.items" data-ignite-op>Add image</button>"#));
    assert!(
        !html.contains(r#"name="content[0][data][text]""#),
        "a heading's field"
    );

    let (_, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        "title=Home&content=&content%5B0%5D%5Btype%5D=gallery&content%5B0%5D%5Bdata%5D%5Bcaption%5D=Trip\
         &content%5B0%5D%5Bdata%5D%5Bitems%5D=&_ignite_op=add%3Acontent.0.data.items",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert!(
        html.contains(r#"name="content[0][data][items][0][url]""#),
        "{html}"
    );
    assert!(html.contains(r#"name="content[0][data][caption]" type="text"  value="Trip""#));
    assert!(data.rows().is_empty(), "nothing saved");
}

#[test]
fn blocks_are_saved_as_type_and_data_and_errors_show_under_the_block_inputs() {
    let _guard = reset();
    let data = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "title=Home&content=&content%5B0%5D%5Btype%5D=heading&content%5B0%5D%5Bdata%5D%5Btext%5D=\
         &content%5B1%5D%5Btype%5D=gallery&content%5B1%5D%5Bdata%5D%5Bitems%5D=\
         &content%5B1%5D%5Bdata%5D%5Bitems%5D%5B0%5D%5Burl%5D=not+a+url",
    );
    assert_eq!(status, Status::UnprocessableEntity);
    let text = html.find(r#"id="content[0][data][text]""#).expect(&html);
    let required = html.find(">Text is required.</p>").unwrap();
    let url = html
        .find(r#"id="content[1][data][items][0][url]""#)
        .unwrap();
    let invalid = html.find(">URL must be a valid URL.</p>").unwrap();
    assert!(text < required && required < url && url < invalid, "{html}");
    assert!(html.contains(">Block 1: Text is required.</p>"));

    let (status, _) = post(
        &client,
        &format!("/admin/{SLUG}"),
        "title=Home&content=&content%5B0%5D%5Btype%5D=heading&content%5B0%5D%5Bdata%5D%5Btext%5D=Hello\
         &content%5B0%5D%5Bdata%5D%5Blevel%5D=h2\
         &content%5B1%5D%5Btype%5D=gallery&content%5B1%5D%5Bdata%5D%5Bcaption%5D=\
         &content%5B1%5D%5Bdata%5D%5Bitems%5D=\
         &content%5B1%5D%5Bdata%5D%5Bitems%5D%5B0%5D%5Burl%5D=https%3A%2F%2Fexample.com%2Fa.jpg",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    let stored: Value =
        serde_json::from_str(data.value(1, "content").as_str().expect("JSON text")).unwrap();
    assert_eq!(
        stored,
        json!([
            {"type": "heading", "data": {"text": "Hello", "level": "h2"}},
            {"type": "gallery", "data": {"caption": null, "items": [{"url": "https://example.com/a.jpg"}]}}
        ])
    );
}

#[test]
fn the_picker_hides_at_the_most_blocks() {
    let _guard = reset();
    setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (_, html) = post(
        &client,
        &format!("/admin/{SLUG}/new/live"),
        "title=Home&content=&content%5B0%5D%5Btype%5D=heading&content%5B1%5D%5Btype%5D=heading\
         &_ignite_op=add%3Acontent%3Aheading",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert!(!html.contains(r#"name="content[2][type]""#), "{html}");
    assert!(!html.contains("Add to Content</summary>"));
    assert!(html.contains(r#"value="up:content.1""#));
}
