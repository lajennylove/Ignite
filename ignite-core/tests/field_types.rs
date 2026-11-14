//! The field types added for custom fields (ACF's types, with Filament's names where Filament
//! has one): each one checks its input, for a static `Field` and for a runtime `FieldDef`,
//! and the structured types store their nested values as JSON that round-trips.

use std::collections::HashMap;

use ignite_core::{
    editor_script, media_library, register_editor, register_media_library, register_runtime,
    runtime_resource, Block, BlockDef, Database, Field, FieldDef, FieldErrors, FieldKind,
    FieldKindDef, FormInput, ListQuery, MediaItem, MediaLibrary, Page, Row, RuntimeData,
    RuntimeFuture, RuntimeResource,
};
use serde_json::{json, Value};

const fn field(name: &'static str, label: &'static str, kind: FieldKind) -> Field {
    Field {
        name,
        label,
        kind,
        required: false,
    }
}

/// Runs `Field::check_value` and returns the stored text, or the error of the field.
fn check(field: &Field, text: &str) -> Result<String, String> {
    let mut errors = FieldErrors::new();
    match field.check_value(text, &mut errors) {
        Some(value) => {
            assert!(errors.is_empty(), "{errors:?}");
            Ok(value)
        }
        None => Err(errors.remove(field.name).expect("an error for the field")),
    }
}

/// Parses stored JSON text.
fn json_of(text: &str) -> Value {
    serde_json::from_str(text).expect("stored JSON")
}

#[test]
fn an_invalid_email_is_refused() {
    const EMAIL: Field = field("email", "Email", FieldKind::Email);

    assert_eq!(
        check(&EMAIL, " ada@example.com "),
        Ok("ada@example.com".into())
    );
    assert_eq!(check(&EMAIL, "ada@localhost"), Ok("ada@localhost".into()));
    for text in [
        "ada",
        "ada@",
        "@example.com",
        "ada@@example.com",
        "ada lovelace@example.com",
        "ada@-example.com",
        "ada@example..com",
    ] {
        assert_eq!(
            check(&EMAIL, text),
            Err("Email must be a valid email address.".into()),
            "{text}"
        );
    }
}

/// Values whose scheme runs code, which neither a URL nor a link accepts, whatever their
/// case or the whitespace and control characters around the scheme.
const SCRIPT_URLS: &[&str] = &[
    "javascript:alert(1)",
    "JavaScript:alert(1)",
    "  javascript:alert(1)",
    "\u{1}javascript:alert(1)",
    "java\tscript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "DATA:text/html;base64,PHNjcmlwdD4=",
    "vbscript:msgbox(1)",
];

#[test]
fn a_url_must_parse() {
    const SITE: Field = field("site", "Site", FieldKind::Url);

    for text in [
        "https://example.com/about?x=1",
        "mailto:ada@example.com",
        "ftp://files.example.com/a.zip",
        // Protocol-relative, as ACF's URL field accepts.
        "//cdn.example.com",
        "//cdn.example.com/lib.js?v=2",
    ] {
        assert_eq!(check(&SITE, text), Ok(text.into()), "{text}");
    }
    let mut refused = vec![
        "example.com",
        "/about",
        "#top",
        "?a=b",
        "http://",
        "//",
        "///about",
        "not a url",
    ];
    refused.extend_from_slice(SCRIPT_URLS);
    for text in refused {
        assert_eq!(
            check(&SITE, text),
            Err("Site must be a valid URL.".into()),
            "{text}"
        );
    }
}

#[test]
fn a_link_accepts_what_the_acf_link_picker_stores() {
    const LINK: Field = field("cta", "Call to action", FieldKind::Link);
    let link = |url: &str| json!({"url": url}).to_string();

    for url in [
        "https://example.com/about",
        "/about",
        "/about?x=1#team",
        "/",
        "#top",
        "?a=b",
        "//cdn.example.com/page",
        "mailto:ada@example.com",
    ] {
        let stored = check(&LINK, &link(url)).unwrap_or_else(|e| panic!("{url}: {e}"));
        assert_eq!(json_of(&stored)["url"], json!(url), "{url}");
    }
    let mut refused = vec![
        "about",
        "//",
        "///about",
        "/\\evil.example.com",
        "http://",
        "not a url",
    ];
    refused.extend_from_slice(SCRIPT_URLS);
    for url in refused {
        assert_eq!(
            check(&LINK, &link(url)),
            Err("Call to action must be a link with a valid URL.".into()),
            "{url}"
        );
    }
}

#[test]
fn a_color_must_be_rrggbb() {
    const ACCENT: Field = field("accent", "Accent", FieldKind::Color);

    assert_eq!(check(&ACCENT, "#1a2B3c"), Ok("#1a2b3c".into()));
    for text in [
        "1a2b3c",
        "#abc",
        "#1a2b3g",
        "#1a2b3c4d",
        "red",
        "rgb(1,2,3)",
    ] {
        assert_eq!(
            check(&ACCENT, text),
            Err("Accent must be a color, as #rrggbb.".into()),
            "{text}"
        );
    }
}

#[test]
fn a_radio_must_be_one_of_its_choices() {
    const SIZE: Field = field(
        "size",
        "Size",
        FieldKind::Radio(&[("s", "Small"), ("l", "Large")]),
    );

    assert_eq!(check(&SIZE, " l "), Ok("l".into()));
    assert_eq!(check(&SIZE, "m"), Err("Choose one of the options.".into()));
}

#[test]
fn an_image_or_a_file_is_a_media_id() {
    const IMAGE: Field = field("photo", "Photo", FieldKind::Image);
    const FILE: Field = field("manual", "Manual", FieldKind::File);

    assert_eq!(check(&IMAGE, " 42 "), Ok("42".into()));
    assert_eq!(check(&FILE, "7"), Ok("7".into()));
    for text in ["0", "-3", "4.5", "photo.jpg"] {
        assert_eq!(
            check(&IMAGE, text),
            Err("Photo must be the id of a media item.".into()),
            "{text}"
        );
    }
}

#[test]
fn a_link_stores_its_url_title_and_target_as_json() {
    const LINK: Field = field("cta", "Call to action", FieldKind::Link);

    let stored = check(
        &LINK,
        r#"{"url": "https://example.com", "title": "Read more", "target": "_blank"}"#,
    )
    .expect("valid");
    assert_eq!(
        json_of(&stored),
        json!({"url": "https://example.com", "title": "Read more", "target": "_blank"})
    );
    // Reading the stored text back gives the same link.
    assert_eq!(check(&LINK, &stored), Ok(stored.clone()));
    // The title and the target may be left out.
    assert_eq!(
        json_of(&check(&LINK, r#"{"url": "https://example.com"}"#).unwrap()),
        json!({"url": "https://example.com", "title": "", "target": ""})
    );

    for text in [
        r#"{"url": "example.com"}"#,
        r#"{"title": "No URL"}"#,
        r#"{"url": "https://example.com", "target": "_top"}"#,
        r#"["https://example.com"]"#,
        "https://example.com",
    ] {
        assert_eq!(
            check(&LINK, text),
            Err("Call to action must be a link with a valid URL.".into()),
            "{text}"
        );
    }
}

#[test]
fn a_wysiwyg_keeps_the_submitted_html() {
    const BODY: Field = field("body", "Body", FieldKind::Wysiwyg);

    assert_eq!(
        check(&BODY, "<p>Hello <strong>world</strong></p>\n"),
        Ok("<p>Hello <strong>world</strong></p>\n".into())
    );
}

#[test]
fn post_object_taxonomy_and_user_load_their_options_and_store_one_value() {
    for kind in [FieldKind::PostObject, FieldKind::Taxonomy, FieldKind::User] {
        let one = field("ref", "Ref", kind);
        assert!(kind.loads_options(), "{kind:?}");
        assert_eq!(check(&one, " 12 "), Ok("12".into()));
    }
    assert!(FieldKind::Lookup.loads_options());
    assert!(FieldKind::Relationship.loads_options());
    assert!(!FieldKind::Text.loads_options());
    assert!(FieldKindDef::Relationship.loads_options());
    assert!(!FieldKindDef::Gallery.loads_options());
}

#[test]
fn a_relationship_stores_its_choices_as_a_json_array() {
    const RELATED: Field = field("related", "Related", FieldKind::Relationship);

    let stored = check(&RELATED, r#"["12", 40]"#).expect("valid");
    assert_eq!(json_of(&stored), json!(["12", "40"]));
    assert_eq!(check(&RELATED, &stored), Ok(stored.clone()));
    assert_eq!(
        check(&RELATED, r#"{"a": 1}"#),
        Err("Related must be a list of choices.".into())
    );
}

#[test]
fn a_gallery_stores_media_ids_as_a_json_array() {
    const GALLERY: Field = field("gallery", "Gallery", FieldKind::Gallery);

    let stored = check(&GALLERY, r#"[3, "8"]"#).expect("valid");
    assert_eq!(json_of(&stored), json!([3, 8]));
    assert_eq!(check(&GALLERY, &stored), Ok(stored.clone()));
    for text in [r#"[3, "x"]"#, "[0]", "3"] {
        assert_eq!(
            check(&GALLERY, text),
            Err("Gallery must be a list of media ids.".into()),
            "{text}"
        );
    }
}

const SLIDE_FIELDS: &[Field] = &[
    Field {
        name: "title",
        label: "Title",
        kind: FieldKind::Text,
        required: true,
    },
    field("email", "Email", FieldKind::Email),
    field("count", "Count", FieldKind::Number),
    field("shown", "Shown", FieldKind::Checkbox),
    field("photo", "Photo", FieldKind::Image),
    field("cta", "Link", FieldKind::Link),
];

#[test]
fn a_repeater_stores_its_rows_as_json_and_round_trips() {
    const SLIDES: Field = field("slides", "Slides", FieldKind::Repeater(SLIDE_FIELDS));

    let rows = json!([
        {
            "title": "One",
            "email": "ada@example.com",
            "count": 3,
            "shown": true,
            "photo": 12,
            "cta": {"url": "https://example.com", "title": "Go", "target": ""}
        },
        {"title": "Two", "count": "4", "shown": false}
    ]);
    let stored = check(&SLIDES, &rows.to_string()).expect("valid");
    let expected = json!([
        {
            "title": "One",
            "email": "ada@example.com",
            "count": 3,
            "shown": true,
            "photo": 12,
            "cta": {"url": "https://example.com", "title": "Go", "target": ""}
        },
        {"title": "Two", "count": 4, "shown": false}
    ]);
    assert_eq!(json_of(&stored), expected);
    // Writing the stored value again gives the same value.
    assert_eq!(check(&SLIDES, &stored), Ok(stored.clone()));
    // A key that is not a sub-field is dropped.
    assert_eq!(
        json_of(&check(&SLIDES, r#"[{"title": "One", "extra": 1}]"#).unwrap()),
        json!([{"title": "One"}])
    );
}

#[test]
fn a_repeater_checks_each_row_against_its_sub_fields() {
    const SLIDES: Field = field("slides", "Slides", FieldKind::Repeater(SLIDE_FIELDS));

    let cases = [
        (
            r#"[{"title": "One"}, {"title": "Two", "email": "nope"}]"#,
            "Row 2: Email must be a valid email address.",
        ),
        (
            r#"[{"email": "ada@example.com"}]"#,
            "Row 1: Title is required.",
        ),
        (
            r#"[{"title": "One", "count": "many"}]"#,
            "Row 1: Count must be a whole number.",
        ),
        (
            r#"[{"title": "One", "photo": -1}]"#,
            "Row 1: Photo must be the id of a media item.",
        ),
        (r#"{"title": "One"}"#, "Slides must be a list of rows."),
        (r#"[["One"]]"#, "Slides must be a list of rows."),
        ("not json", "Slides must be a list of rows."),
    ];
    for (text, message) in cases {
        assert_eq!(check(&SLIDES, text), Err(message.to_string()), "{text}");
    }
}

#[test]
fn a_required_repeater_with_no_rows_is_required() {
    const SLIDES: Field = Field {
        name: "slides",
        label: "Slides",
        kind: FieldKind::Repeater(SLIDE_FIELDS),
        required: true,
    };

    assert_eq!(check(&SLIDES, "[]"), Err("Slides is required.".into()));
    let optional = field("slides", "Slides", FieldKind::Repeater(SLIDE_FIELDS));
    assert_eq!(check(&optional, "[]"), Ok("[]".into()));
}

#[test]
fn a_repeater_can_nest_a_repeater() {
    const ITEMS: &[Field] = &[field("label", "Label", FieldKind::Text)];
    const SECTIONS: &[Field] = &[
        field("heading", "Heading", FieldKind::Text),
        field("items", "Items", FieldKind::Repeater(ITEMS)),
    ];
    const PAGE: Field = field("sections", "Sections", FieldKind::Repeater(SECTIONS));

    let value = json!([{"heading": "A", "items": [{"label": "x"}, {"label": "y"}]}]);
    let stored = check(&PAGE, &value.to_string()).expect("valid");
    assert_eq!(json_of(&stored), value);
    assert_eq!(
        check(&PAGE, r#"[{"items": {"label": "x"}}]"#),
        Err("Row 1: Items must be a list of rows.".into())
    );
}

#[test]
fn a_group_and_a_clone_store_one_json_object() {
    const ADDRESS: &[Field] = &[
        field("street", "Street", FieldKind::Text),
        field("color", "Color", FieldKind::Color),
    ];
    for kind in [FieldKind::Group(ADDRESS), FieldKind::Clone(ADDRESS)] {
        let group = field("address", "Address", kind);
        let stored =
            check(&group, r##"{"street": "Main St", "color": "#AABBCC"}"##).expect("valid");
        assert_eq!(
            json_of(&stored),
            json!({"street": "Main St", "color": "#aabbcc"})
        );
        assert_eq!(check(&group, &stored), Ok(stored.clone()));
        assert_eq!(
            check(&group, r#"{"color": "blue"}"#),
            Err("Color must be a color, as #rrggbb.".into())
        );
        assert_eq!(
            check(&group, "[]"),
            Err("Address must be a group of values.".into())
        );
    }
}

const BLOCKS: &[Block] = &[
    Block {
        name: "heading",
        label: "Heading",
        fields: &[Field {
            name: "text",
            label: "Text",
            kind: FieldKind::Text,
            required: true,
        }],
    },
    Block {
        name: "image",
        label: "Image",
        fields: &[
            field("photo", "Photo", FieldKind::Image),
            field("caption", "Caption", FieldKind::Textarea),
        ],
    },
];

#[test]
fn flexible_content_stores_typed_blocks_as_json_and_round_trips() {
    const CONTENT: Field = field("content", "Content", FieldKind::FlexibleContent(BLOCKS));

    let value = json!([
        {"type": "heading", "data": {"text": "Welcome"}},
        {"type": "image", "data": {"photo": 4, "caption": "A lantern"}}
    ]);
    let stored = check(&CONTENT, &value.to_string()).expect("valid");
    assert_eq!(json_of(&stored), value);
    assert_eq!(check(&CONTENT, &stored), Ok(stored.clone()));

    let cases = [
        (
            r#"[{"type": "video", "data": {}}]"#,
            "Block 1: \"video\" is not a block of Content.",
        ),
        (
            r#"[{"type": "image", "data": {}}, {"type": "heading", "data": {}}]"#,
            "Block 2: Text is required.",
        ),
        (r#"[{"data": {}}]"#, "Content must be a list of blocks."),
        (
            r#"[{"type": "heading", "data": []}]"#,
            "Content must be a list of blocks.",
        ),
        (
            r#"{"type": "heading"}"#,
            "Content must be a list of blocks.",
        ),
    ];
    for (text, message) in cases {
        assert_eq!(check(&CONTENT, text), Err(message.to_string()), "{text}");
    }
}

#[test]
fn every_new_kind_has_an_owned_twin() {
    const SUB: &[Field] = &[field("title", "Title", FieldKind::Text)];
    let sub = vec![FieldDef {
        name: "title".into(),
        label: "Title".into(),
        kind: FieldKindDef::Text,
        required: false,
    }];
    let pairs = [
        (FieldKind::Email, FieldKindDef::Email),
        (FieldKind::Url, FieldKindDef::Url),
        (
            FieldKind::Radio(&[("a", "A")]),
            FieldKindDef::Radio(vec![("a".into(), "A".into())]),
        ),
        (FieldKind::Color, FieldKindDef::Color),
        (FieldKind::Image, FieldKindDef::Image),
        (FieldKind::File, FieldKindDef::File),
        (FieldKind::Link, FieldKindDef::Link),
        (FieldKind::Wysiwyg, FieldKindDef::Wysiwyg),
        (FieldKind::PostObject, FieldKindDef::PostObject),
        (FieldKind::Relationship, FieldKindDef::Relationship),
        (FieldKind::Taxonomy, FieldKindDef::Taxonomy),
        (FieldKind::User, FieldKindDef::User),
        (
            FieldKind::Repeater(SUB),
            FieldKindDef::Repeater(sub.clone()),
        ),
        (FieldKind::Group(SUB), FieldKindDef::Group(sub.clone())),
        (FieldKind::Gallery, FieldKindDef::Gallery),
        (FieldKind::Clone(SUB), FieldKindDef::Clone(sub.clone())),
        (
            FieldKind::FlexibleContent(&[Block {
                name: "hero",
                label: "Hero",
                fields: SUB,
            }]),
            FieldKindDef::FlexibleContent(vec![BlockDef {
                name: "hero".into(),
                label: "Hero".into(),
                fields: sub.clone(),
            }]),
        ),
    ];
    for (kind, def) in pairs {
        assert_eq!(FieldKindDef::from(kind), def, "{kind:?}");
    }
}

/// A data source that is never called: these tests only validate.
struct NoData;

impl RuntimeData for NoData {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        unreachable!()
    }
    fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
        unreachable!()
    }
    fn create<'a>(&'a self, _: &'a Database, _: Row) -> RuntimeFuture<'a, Row> {
        unreachable!()
    }
    fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
        unreachable!()
    }
    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        unreachable!()
    }
}

fn def(name: &str, label: &str, kind: FieldKindDef, required: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind,
        required,
    }
}

fn form(pairs: &[(&str, &str)]) -> FormInput {
    FormInput::new(
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<HashMap<_, _>>(),
    )
}

/// A runtime resource with one field of each new text kind and a repeater.
fn custom_fields(slug: &str) -> std::sync::Arc<ignite_core::RegisteredResource> {
    let rows = vec![
        def("title", "Title", FieldKindDef::Text, true),
        def("email", "Email", FieldKindDef::Email, false),
    ];
    let resource = RuntimeResource::new(slug, "Pages", "Page", NoData)
        .field(def("contact", "Contact", FieldKindDef::Email, true))
        .field(def("site", "Site", FieldKindDef::Url, false))
        .field(def("accent", "Accent", FieldKindDef::Color, false))
        .field(def(
            "size",
            "Size",
            FieldKindDef::Radio(vec![("s".into(), "Small".into())]),
            false,
        ))
        .field(def("photo", "Photo", FieldKindDef::Image, false))
        .field(def("cta", "Link", FieldKindDef::Link, false))
        .field(def("body", "Body", FieldKindDef::Wysiwyg, false))
        .field(def("author", "Author", FieldKindDef::User, false))
        .field(def("slides", "Slides", FieldKindDef::Repeater(rows), false));
    register_runtime(resource).expect("register");
    runtime_resource(slug).expect("registered")
}

#[test]
fn a_runtime_resource_validates_the_new_kinds_and_stores_text() {
    let resource = custom_fields("ft-valid");
    let slides = r#"[{"title": "One", "email": "ada@example.com"}]"#;
    let row = resource
        .validate(&form(&[
            ("contact", "ada@example.com"),
            ("site", "https://example.com"),
            ("accent", "#FFAA00"),
            ("size", "s"),
            ("photo", "9"),
            ("cta", r#"{"url": "https://example.com/a"}"#),
            ("body", "<p>Hi</p>"),
            ("author", " 3 "),
            ("slides", slides),
        ]))
        .expect("valid");

    assert_eq!(row["contact"], json!("ada@example.com"));
    assert_eq!(row["site"], json!("https://example.com"));
    assert_eq!(row["accent"], json!("#ffaa00"));
    assert_eq!(row["size"], json!("s"));
    assert_eq!(row["photo"], json!("9"));
    assert_eq!(row["body"], json!("<p>Hi</p>"));
    assert_eq!(row["author"], json!("3"));
    let link = row["cta"].as_str().expect("JSON text");
    assert_eq!(
        json_of(link),
        json!({"url": "https://example.com/a", "title": "", "target": ""})
    );
    let stored = row["slides"].as_str().expect("JSON text");
    assert_eq!(json_of(stored), json_of(slides));

    // The stored value, sent back on an edit, validates to the same value.
    let again = resource
        .validate_update(&form(&[("contact", "ada@example.com"), ("slides", stored)]))
        .expect("valid");
    assert_eq!(again["slides"], json!(stored));
}

#[test]
fn a_runtime_resource_refuses_invalid_values_of_the_new_kinds() {
    let errors = custom_fields("ft-invalid")
        .validate(&form(&[
            ("contact", "not-an-email"),
            ("site", "example.com"),
            ("accent", "orange"),
            ("size", "xl"),
            ("photo", "first"),
            ("cta", r#"{"url": "nope"}"#),
            ("slides", r#"[{"title": "One", "email": "bad"}]"#),
        ]))
        .unwrap_err();

    let message = |name: &str| errors.get(name).map(String::as_str);
    assert_eq!(
        message("contact"),
        Some("Contact must be a valid email address.")
    );
    assert_eq!(message("site"), Some("Site must be a valid URL."));
    assert_eq!(
        message("accent"),
        Some("Accent must be a color, as #rrggbb.")
    );
    assert_eq!(message("size"), Some("Choose one of the options."));
    assert_eq!(
        message("photo"),
        Some("Photo must be the id of a media item.")
    );
    assert_eq!(
        message("cta"),
        Some("Link must be a link with a valid URL.")
    );
    assert_eq!(
        message("slides"),
        Some("Row 1: Email must be a valid email address.")
    );
}

#[test]
fn the_app_registers_the_editor_script() {
    register_editor("/vendor/editor/editor.min.js");
    assert_eq!(
        editor_script().as_deref(),
        Some("/vendor/editor/editor.min.js")
    );
}

struct Library;

impl MediaLibrary for Library {
    fn find<'a>(&'a self, _: &'a Database, ids: &'a [i64]) -> RuntimeFuture<'a, Vec<MediaItem>> {
        Box::pin(async move {
            Ok(ids
                .iter()
                .map(|&id| MediaItem {
                    id,
                    title: format!("Item {id}"),
                    url: format!("/uploads/{id}.jpg"),
                })
                .collect())
        })
    }
}

#[test]
fn the_app_registers_a_media_library() {
    register_media_library(Library);
    assert!(media_library().is_some());
}
