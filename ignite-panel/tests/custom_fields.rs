//! End-to-end tests for the field types added for custom fields, on a model with
//! `#[derive(IgniteResource)]`: each one renders through its partial, refuses an invalid
//! value, and stores a valid one (the structured ones as JSON text) that the edit form shows
//! again. Runs against the test database, with an editor script and a media library
//! registered.

mod common;

use common::{reset, test_db, with_token, EMAIL, PASSWORD};
use ignite_core::{
    register_editor, register_media_library, Block, Database, Field, FieldKind, IgniteResource,
    MediaItem, MediaLibrary, RuntimeFuture,
};
use ignite_panel::Ignite;
use orbit_orm::{Model, OrbitModel};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde::Serialize;
use serde_json::{json, Value};

/// The posts a post object or relationship field may point to.
async fn post_choices(_db: &Database) -> Result<Vec<(String, String)>, orbit_orm::Error> {
    Ok(vec![
        ("1".to_string(), "Hello".to_string()),
        ("2".to_string(), "About".to_string()),
    ])
}

const SLIDE_FIELDS: &[Field] = &[
    Field {
        name: "title",
        label: "Title",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        name: "email",
        label: "Email",
        kind: FieldKind::Email,
        required: false,
    },
];

const BLOCKS: &[Block] = &[Block {
    name: "heading",
    label: "Heading",
    fields: &[Field {
        name: "text",
        label: "Text",
        kind: FieldKind::Text,
        required: true,
    }],
}];

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "landing_pages")]
#[ignite(table = landing_pages, title = "Landing pages", singular = "Landing page")]
pub struct LandingPage {
    id: i32,
    #[orbit(fillable)]
    #[ignite(searchable)]
    name: String,
    #[orbit(fillable)]
    #[ignite(form_type = "email")]
    contact: String,
    #[orbit(fillable)]
    #[ignite(form_type = "url")]
    site: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "color")]
    accent: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "radio", options = [("s", "Small"), ("l", "Large")])]
    size: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "image")]
    photo: Option<i64>,
    #[orbit(fillable)]
    #[ignite(form_type = "gallery")]
    gallery: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "link")]
    cta: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "wysiwyg")]
    body: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "post_object", options = post_choices)]
    parent: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "relationship", options = post_choices)]
    related: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "repeater", fields = SLIDE_FIELDS)]
    slides: Option<String>,
    #[orbit(fillable)]
    #[ignite(form_type = "flexible_content", blocks = BLOCKS)]
    content: Option<String>,
}

/// A media library with the items 1 to 10.
struct Library;

impl MediaLibrary for Library {
    fn find<'a>(&'a self, _: &'a Database, ids: &'a [i64]) -> RuntimeFuture<'a, Vec<MediaItem>> {
        Box::pin(async move {
            Ok(ids
                .iter()
                .filter(|id| (1..=10).contains(*id))
                .map(|&id| MediaItem {
                    id,
                    title: format!("Photo {id}"),
                    url: format!("/uploads/photo-{id}.jpg"),
                })
                .collect())
        })
    }

    fn browse_url(&self) -> Option<String> {
        Some("/admin/media".to_string())
    }
}

/// Resets the shared database and creates the `landing_pages` table.
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    register_editor("/vendor/editor/init.js");
    register_media_library(Library);
    let conn = test_db();
    conn.exec("DROP TABLE IF EXISTS landing_pages");
    conn.exec(
        "CREATE TABLE landing_pages (id INT AUTO_INCREMENT PRIMARY KEY, \
         name VARCHAR(255) NOT NULL, contact VARCHAR(255) NOT NULL, site TEXT NULL, \
         accent VARCHAR(7) NULL, size VARCHAR(10) NULL, photo BIGINT NULL, gallery TEXT NULL, \
         cta TEXT NULL, body TEXT NULL, parent VARCHAR(20) NULL, related TEXT NULL, \
         slides TEXT NULL, content TEXT NULL)",
    );
    guard
}

fn signed_in() -> Client {
    common::ensure_secret();
    let client =
        Client::tracked(rocket::build().attach(Ignite::new().resource::<LandingPage>().fairing()))
            .unwrap();
    client.get("/admin/login").dispatch();
    let status = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email={EMAIL}&password={PASSWORD}"),
        ))
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther);
    client
}

/// The urlencoded form of `pairs`.
fn encode(pairs: &[(&str, &str)]) -> String {
    form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

/// A valid page, with the given overrides applied to the defaults.
fn page_form(overrides: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = [
        ("name", "Launch"),
        ("contact", "ada@example.com"),
        ("site", "https://example.com"),
        ("accent", "#FFAA00"),
        ("size", ""),
        ("size", "l"),
        ("photo", "4"),
        ("gallery[]", "2"),
        ("gallery[]", "3"),
        ("gallery[]", ""),
        ("cta[url]", "https://example.com/start"),
        ("cta[title]", "Start"),
        ("cta[target]", "_blank"),
        ("body", "<p>Hello <em>world</em></p>"),
        ("parent", "1"),
        ("related[]", ""),
        ("related[]", "1"),
        ("related[]", "2"),
        (
            "slides",
            r#"[{"title": "One", "email": "ada@example.com"}]"#,
        ),
        (
            "content",
            r#"[{"type": "heading", "data": {"text": "Welcome"}}]"#,
        ),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    for (key, value) in overrides {
        fields.retain(|(k, _)| k != key);
        fields.push((key.to_string(), value.to_string()));
    }
    fields
}

fn post(client: &Client, fields: &[(String, String)]) -> (Status, String) {
    let pairs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let response = client
        .post("/admin/landing_pages")
        .header(ContentType::Form)
        .body(with_token(client, &encode(&pairs)))
        .dispatch();
    let status = response.status();
    (status, response.into_string().unwrap_or_default())
}

fn stored_page() -> LandingPage {
    let t = test_db();
    t.run(async { LandingPage::query().find(&t.db, 1).await })
        .expect("query")
        .expect("a stored page")
}

#[test]
fn each_new_type_renders_through_its_partial() {
    let _guard = setup();
    let client = signed_in();

    let form = client
        .get("/admin/landing_pages/new")
        .dispatch()
        .into_string()
        .unwrap();
    for expected in [
        r#"name="contact" type="email""#,
        r#"name="site" type="text" inputmode="url""#,
        r##"name="accent" type="text" value="" pattern="#[0-9a-fA-F]{6}""##,
        r#"type="radio" class="h-4 w-4 border-bordergray text-primary focus:ring-primary" name="size" value="l""#,
        r#"name="photo" type="number" min="1" step="1" value="" data-ignite-media="image""#,
        r#"name="gallery[]" type="number""#,
        r#"href="/admin/media""#,
        r#"name="cta[url]" type="text" inputmode="url""#,
        r#"name="cta[target]" value="_blank""#,
        r#"name="body" data-ignite-editor="wysiwyg""#,
        r#"<script src="/vendor/editor/init.js" defer></script>"#,
        r#"<option value="2" >About</option>"#,
        r#"name="related[]" multiple"#,
        // A repeater renders as rows of its sub-fields (see tests/repeater.rs).
        r#"data-ignite-field="slides" data-ignite-structure="repeater""#,
        r#"data-ignite-field="content" data-ignite-structure="flexible_content""#,
    ] {
        assert!(form.contains(expected), "missing {expected}\n{form}");
    }
}

#[test]
fn invalid_values_of_the_new_types_are_refused() {
    let _guard = setup();
    let client = signed_in();

    let (status, body) = post(
        &client,
        &page_form(&[
            ("contact", "not-an-email"),
            ("site", "example.com"),
            ("accent", "orange"),
            ("photo", "99"),
            ("parent", "7"),
            ("related[]", "9"),
            ("slides", r#"[{"title": "One", "email": "bad"}]"#),
            ("content", r#"[{"type": "video", "data": {}}]"#),
        ]),
    );
    assert_eq!(status, Status::UnprocessableEntity);
    for message in [
        "Contact must be a valid email address.",
        "Site must be a valid URL.",
        "Accent must be a color, as #rrggbb.",
        "Photo must be the id of a media item.",
        "Row 1: Email must be a valid email address.",
        "Block 1: &quot;video&quot; is not a block of Content.",
        "Choose one of the options.",
    ] {
        assert!(body.contains(message), "missing {message}\n{body}");
    }
    assert_eq!(
        body.matches("Choose one of the options.").count(),
        2,
        "the post object and the relationship"
    );
    assert_eq!(
        test_db().count("SELECT COUNT(*) FROM landing_pages", &[]),
        0
    );
}

#[test]
fn valid_values_are_stored_and_the_edit_form_shows_them_again() {
    let _guard = setup();
    let client = signed_in();

    let (status, body) = post(&client, &page_form(&[]));
    assert_eq!(status, Status::SeeOther, "{body}");

    let page = stored_page();
    assert_eq!(page.contact, "ada@example.com");
    assert_eq!(page.accent.as_deref(), Some("#ffaa00"));
    assert_eq!(page.size.as_deref(), Some("l"));
    assert_eq!(page.photo, Some(4));
    assert_eq!(page.body.as_deref(), Some("<p>Hello <em>world</em></p>"));
    assert_eq!(page.parent.as_deref(), Some("1"));
    let json_of = |text: &Option<String>| -> Value {
        serde_json::from_str(text.as_deref().expect("stored")).expect("JSON")
    };
    assert_eq!(json_of(&page.gallery), json!([2, 3]));
    assert_eq!(
        json_of(&page.cta),
        json!({"url": "https://example.com/start", "title": "Start", "target": "_blank"})
    );
    assert_eq!(json_of(&page.related), json!(["1", "2"]));
    assert_eq!(
        json_of(&page.slides),
        json!([{"title": "One", "email": "ada@example.com"}])
    );
    assert_eq!(
        json_of(&page.content),
        json!([{"type": "heading", "data": {"text": "Welcome"}}])
    );

    let edit = client
        .get("/admin/landing_pages/1/edit")
        .dispatch()
        .into_string()
        .unwrap();
    for expected in [
        r#"name="size" value="l" checked"#,
        r#"name="cta[url]" type="text" inputmode="url" value="https://example.com/start""#,
        r#"name="cta[target]" value="_blank" checked"#,
        r#"name="gallery[]" type="number" min="1" step="1" value="3""#,
        r#"src="/uploads/photo-4.jpg""#,
        "Photo 4",
        r#"<option value="1" selected>Hello</option>"#,
        r#"<option value="2" selected>About</option>"#,
    ] {
        assert!(edit.contains(expected), "missing {expected}\n{edit}");
    }

    // Saving the edit form as it is shown keeps every value.
    let unchanged = page_form(&[]);
    let pairs: Vec<(&str, &str)> = unchanged
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let response = client
        .post("/admin/landing_pages/1")
        .header(ContentType::Form)
        .body(with_token(&client, &encode(&pairs)))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    let again = stored_page();
    assert_eq!(again.slides, page.slides);
    assert_eq!(again.content, page.content);
    assert_eq!(again.related, page.related);
}

#[test]
fn clearing_a_relationship_and_a_link_stores_null() {
    let _guard = setup();
    let client = signed_in();
    assert_eq!(post(&client, &page_form(&[])).0, Status::SeeOther);

    // A relationship with nothing chosen sends only its hidden blank value; a link with
    // every input blank sends no value either.
    let mut cleared = page_form(&[]);
    cleared.retain(|(k, v)| !(k == "related[]" && !v.is_empty()));
    cleared.retain(|(k, _)| !k.starts_with("cta["));
    cleared.push(("cta[url]".into(), String::new()));
    cleared.push(("cta[title]".into(), String::new()));
    let pairs: Vec<(&str, &str)> = cleared
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let response = client
        .post("/admin/landing_pages/1")
        .header(ContentType::Form)
        .body(with_token(&client, &encode(&pairs)))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    let page = stored_page();
    assert_eq!(page.related, None);
    assert_eq!(page.cta, None);
}

#[test]
fn a_link_takes_a_relative_url_and_a_url_a_protocol_relative_one() {
    let _guard = setup();
    let client = signed_in();

    let (status, body) = post(
        &client,
        &page_form(&[
            ("site", "//cdn.example.com/start"),
            ("cta[url]", "/about#team"),
        ]),
    );
    assert_eq!(status, Status::SeeOther, "{body}");
    let page = stored_page();
    assert_eq!(page.site.as_deref(), Some("//cdn.example.com/start"));
    let cta: Value = serde_json::from_str(page.cta.as_deref().expect("stored")).unwrap();
    assert_eq!(cta["url"], json!("/about#team"));

    let (status, body) = post(&client, &page_form(&[("cta[url]", " JavaScript:alert(1)")]));
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(
        body.contains("Cta must be a link with a valid URL."),
        "{body}"
    );
}
