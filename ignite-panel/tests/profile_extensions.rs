//! The profile's extension points, as the fields a Filament app adds to its `EditProfile`
//! form: fields whose values the app loads and saves itself (`Ignite::profile_fields`), shown
//! after the name and the email with the same markup and saved by "Save details", and the
//! app's own picture field in place of the upload (`Ignite::profile_picture`), with the URL of
//! the user's picture as Filament's `getFilamentAvatarUrl()`.

mod common;

use std::collections::HashMap;
use std::sync::Mutex;

use common::{reset, signed_in_with, with_token};
use ignite_core::{FieldDef, FieldKindDef, Row, ValidationError};
use ignite_panel::{Ignite, ProfileFields, ProfilePicture};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::{Client, LocalResponse};
use serde_json::Value;

/// The app's user meta: values by user id and key, as Mission Control keeps them in its
/// `usermeta` table.
static META: Mutex<Option<HashMap<(i64, String), Value>>> = Mutex::new(None);

fn meta(user: i64, key: &str) -> Option<Value> {
    META.lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&(user, key.to_string())).cloned())
}

fn set_meta(user: i64, key: &str, value: Value) {
    META.lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert((user, key.to_string()), value);
}

fn clear_meta() {
    *META.lock().unwrap() = Some(HashMap::new());
}

fn field(name: &str, label: &str, kind: FieldKindDef, required: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind,
        required,
    }
}

/// WordPress's profile fields, kept as user meta.
fn wordpress_fields() -> ProfileFields {
    ProfileFields::make()
        .field(field("first_name", "First Name", FieldKindDef::Text, true))
        .field(field("nickname", "Nickname", FieldKindDef::Text, false))
        .field(field(
            "display_name",
            "Display name publicly as",
            FieldKindDef::Lookup,
            false,
        ))
        .field(field("website", "Website", FieldKindDef::Url, false))
        .field(field(
            "description",
            "Biographical Info",
            FieldKindDef::Textarea,
            false,
        ))
        .field(field(
            "rich_editing",
            "Disable the visual editor when writing",
            FieldKindDef::Checkbox,
            false,
        ))
        .load(|_, user| {
            let id = user.id();
            Box::pin(async move {
                let mut row = Row::new();
                for key in [
                    "first_name",
                    "nickname",
                    "display_name",
                    "website",
                    "description",
                    "rich_editing",
                ] {
                    if let Some(value) = meta(id, key) {
                        row.insert(key.to_string(), value);
                    }
                }
                Ok(row)
            })
        })
        .save(|_, user, values| {
            let id = user.id();
            Box::pin(async move {
                if values.get("nickname").and_then(Value::as_str) == Some("taken") {
                    return Ok(Err(ValidationError::with_messages([(
                        "nickname",
                        "That nickname is in use.",
                    )])));
                }
                for (key, value) in values {
                    set_meta(id, &key, value);
                }
                Ok(Ok(()))
            })
        })
        .options(|_, user, field| {
            let name = user.name().to_string();
            let field = field.to_string();
            Box::pin(async move {
                Ok(match field.as_str() {
                    "display_name" => vec![(name.clone(), name), ("admin".into(), "admin".into())],
                    _ => Vec::new(),
                })
            })
        })
}

fn signed_in(picture: bool) -> Client {
    clear_meta();
    signed_in_with(|ignite: Ignite| {
        let ignite = ignite.profile_fields(wordpress_fields());
        if !picture {
            return ignite;
        }
        ignite.profile_picture(
            ProfilePicture::make(field(
                "avatar_url",
                "Profile Picture URL",
                FieldKindDef::Url,
                false,
            ))
            .load(|_, user| {
                let id = user.id();
                Box::pin(async move {
                    let mut row = Row::new();
                    if let Some(value) = meta(id, "avatar_url") {
                        row.insert("avatar_url".into(), value);
                    }
                    Ok(row)
                })
            })
            .save(|_, user, values| {
                let id = user.id();
                Box::pin(async move {
                    for (key, value) in values {
                        set_meta(id, &key, value);
                    }
                    Ok(Ok(()))
                })
            })
            .avatar_url(|_, user| {
                let id = user.id();
                Box::pin(async move {
                    Ok(meta(id, "avatar_url").and_then(|v| v.as_str().map(str::to_string)))
                })
            }),
        )
    })
}

fn post<'c>(client: &'c Client, path: &'static str, body: &str) -> LocalResponse<'c> {
    let body = with_token(client, body);
    client
        .post(path)
        .header(ContentType::Form)
        .body(body)
        .dispatch()
}

fn profile(client: &Client) -> String {
    let response = client.get("/admin/profile").dispatch();
    assert_eq!(response.status(), Status::Ok);
    response.into_string().unwrap()
}

fn user_id() -> i64 {
    let conn = common::test_db();
    conn.run(ignite_panel::users::find_by_email(&conn.db, common::EMAIL))
        .unwrap()
        .expect("the test user")
        .id
}

const DETAILS: &str = "name=Admin&email=admin%40example.test";

#[test]
fn the_extra_fields_render_after_name_and_email_with_the_same_markup() {
    let _guard = reset();
    let client = signed_in(false);
    let page = profile(&client);
    let at = |needle: &str| {
        page.find(needle)
            .unwrap_or_else(|| panic!("{needle} in {page}"))
    };
    let name = at(r#"data-ignite-field="name""#);
    let email = at(r#"data-ignite-field="email""#);
    let job = at(r#"data-ignite-field="job_title""#);
    let first = at(r#"data-ignite-field="first_name""#);
    let bio = at(r#"data-ignite-field="description""#);
    let save = at("Save details");
    assert!(name < email && email < job && job < first && first < bio && bio < save);
    // The same markup as the name: the label, then the input.
    assert!(page.contains(r#"<label class="ig-label" for="first_name">First Name *</label>"#));
    assert!(page.contains(r#"<label class="ig-label" for="name">Name *</label>"#));
    // The lookup's choices come from the app.
    assert!(
        page.contains(r#"<option value="admin" >admin</option>"#)
            || page.contains(r#"<option value="admin""#)
    );
    // The upload stays the default picture field.
    assert!(page.contains(r#"name="avatar""#));
    assert!(!page.contains("Save picture"));
}

#[test]
fn the_extra_fields_load_and_save_through_the_app() {
    let _guard = reset();
    let client = signed_in(false);
    let id = user_id();
    set_meta(id, "nickname", Value::String("ada".into()));
    assert!(
        profile(&client).contains(r#"value="ada""#),
        "the stored value shows"
    );

    let response = post(
        &client,
        "/admin/profile",
        &format!(
            "{DETAILS}&first_name=Ada&nickname=lovelace&display_name=admin&website=https%3A%2F%2Fexample.test&description=Hi&rich_editing=on"
        ),
    );
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(meta(id, "first_name"), Some(Value::String("Ada".into())));
    assert_eq!(meta(id, "nickname"), Some(Value::String("lovelace".into())));
    assert_eq!(
        meta(id, "display_name"),
        Some(Value::String("admin".into()))
    );
    assert_eq!(meta(id, "rich_editing"), Some(Value::Bool(true)));
    let page = profile(&client);
    assert!(
        page.contains("Profile saved."),
        "the same message as the details"
    );
    assert!(page.contains(r#"value="lovelace""#));

    // An unticked checkbox saves false, and a blank optional field null.
    post(
        &client,
        "/admin/profile",
        &format!("{DETAILS}&first_name=Ada&nickname="),
    );
    assert_eq!(meta(id, "rich_editing"), Some(Value::Bool(false)));
    assert_eq!(meta(id, "nickname"), Some(Value::Null));
}

#[test]
fn the_extra_fields_show_their_errors_on_the_field() {
    let _guard = reset();
    let client = signed_in(false);
    let id = user_id();
    let response = post(
        &client,
        "/admin/profile",
        "name=Renamed&email=admin%40example.test&first_name=&website=not+a+url&display_name=nobody&nickname=kept",
    );
    assert_eq!(response.status(), Status::UnprocessableEntity);
    let page = response.into_string().unwrap();
    let after = |field: &str, message: &str| {
        let start = page
            .find(&format!(r#"data-ignite-field="{field}""#))
            .unwrap();
        let end = page[start + 1..]
            .find("data-ignite-field=")
            .map_or(page.len(), |i| start + 1 + i);
        assert!(
            page[start..end].contains(message),
            "{message} under {field}"
        );
    };
    after("first_name", "First Name is required.");
    after("website", "Website must be a valid URL.");
    after("display_name", "Choose one of the options.");
    assert!(
        page.contains(r#"value="kept""#),
        "the typed values are kept"
    );
    assert!(page.contains(r#"value="Renamed""#));
    assert_eq!(meta(id, "nickname"), None, "nothing is saved");
    let conn = common::test_db();
    let user = conn
        .run(ignite_panel::users::find_by_email(&conn.db, common::EMAIL))
        .unwrap()
        .unwrap();
    assert_eq!(user.name, "Admin", "the name is not saved either");
}

#[test]
fn the_apps_refusal_shows_on_the_field_and_saves_nothing() {
    let _guard = reset();
    let client = signed_in(false);
    let response = post(
        &client,
        "/admin/profile",
        "name=Renamed&email=admin%40example.test&first_name=Ada&nickname=taken",
    );
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("That nickname is in use."));
    let conn = common::test_db();
    let user = conn
        .run(ignite_panel::users::find_by_email(&conn.db, common::EMAIL))
        .unwrap()
        .unwrap();
    assert_eq!(user.name, "Admin");
}

#[test]
fn the_password_form_keeps_the_extra_values() {
    let _guard = reset();
    let client = signed_in(false);
    set_meta(user_id(), "nickname", Value::String("ada".into()));
    let response = post(
        &client,
        "/admin/profile/password",
        "current_password=wrong&new_password=short&confirm_password=other",
    );
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response.into_string().unwrap().contains(r#"value="ada""#));
}

#[test]
fn the_app_replaces_the_picture_field_and_gives_the_avatar() {
    let _guard = reset();
    let client = signed_in(true);
    let id = user_id();
    let page = profile(&client);
    assert!(!page.contains(r#"name="avatar""#), "the upload is replaced");
    assert!(page.contains(r#"action="/admin/profile/picture""#));
    assert!(page.contains(r#"data-ignite-field="avatar_url""#));
    assert!(page.contains("Save picture"));
    // The details form keeps its fields; the picture field is not one of them.
    let details = &page[..page.find("Save details").unwrap()];
    assert!(!details.contains(r#"data-ignite-field="avatar_url""#));

    let response = post(
        &client,
        "/admin/profile/picture",
        "avatar_url=https%3A%2F%2Fcdn.example.test%2Fada.webp",
    );
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(
        meta(id, "avatar_url"),
        Some(Value::String("https://cdn.example.test/ada.webp".into()))
    );
    let page = profile(&client);
    assert!(page.contains("Picture saved."));
    // The top bar and the card show the app's picture.
    assert!(page.contains(r#"<img src="https://cdn.example.test/ada.webp" alt="" class="h-9 w-9 rounded-full object-cover">"#));
    assert!(page.contains(r#"<img src="https://cdn.example.test/ada.webp" alt="" class="h-20 w-20 shrink-0 rounded-full object-cover">"#));
    // And so does every other page.
    let dashboard = client.get("/admin").dispatch().into_string().unwrap();
    assert!(dashboard.contains("https://cdn.example.test/ada.webp"));
}

#[test]
fn a_refused_picture_shows_its_error_on_the_field() {
    let _guard = reset();
    let client = signed_in(true);
    let response = post(
        &client,
        "/admin/profile/picture",
        "avatar_url=javascript%3Aalert(1)",
    );
    assert_eq!(response.status(), Status::UnprocessableEntity);
    let page = response.into_string().unwrap();
    let start = page.find(r#"data-ignite-field="avatar_url""#).unwrap();
    assert!(page[start..].contains("Profile Picture URL must be a valid URL."));
    assert!(
        page.contains(r#"data-ignite-field="first_name""#),
        "the details form shows"
    );
    assert_eq!(meta(user_id(), "avatar_url"), None);
}

#[test]
fn without_a_picture_field_its_path_is_not_found() {
    let _guard = reset();
    let client = signed_in(false);
    let response = post(&client, "/admin/profile/picture", "avatar_url=x");
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
#[should_panic(expected = "the profile already has a field named `job_title`")]
fn a_field_name_used_twice_stops_the_panel() {
    let _guard = reset();
    common::ensure_secret();
    let _ = Ignite::new()
        .profile_field(ignite_core::Field {
            name: "job_title",
            label: "Job title",
            kind: ignite_core::FieldKind::Text,
            required: false,
        })
        .profile_fields(ProfileFields::make().field(field(
            "job_title",
            "Job",
            FieldKindDef::Text,
            false,
        )))
        .fairing();
}
