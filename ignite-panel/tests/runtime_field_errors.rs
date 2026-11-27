//! A runtime resource's data layer refusing a save with field messages, as Laravel's
//! `ValidationException::withMessages()`: `RuntimeData::try_create_for` and `try_update_for`
//! answer `ValidationError`, and the panel shows the form again with 422, each message under
//! its field and the typed values kept. Runs against the test database for the user.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ActingUser, ColumnDef, Database, FieldDef, FieldKindDef,
    ListQuery, Page, Row, RuntimeData, RuntimeFuture, RuntimeResource, ValidationError,
};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::json;

const SLUG: &str = "fe-settings";

/// Settings that refuse a site title of "WordPress" and an email without a domain, and save
/// anything else.
#[derive(Clone, Default)]
struct Settings(Arc<Mutex<Vec<Row>>>);

fn refusal(input: &Row) -> Option<ValidationError> {
    let mut messages = Vec::new();
    if input.get("title") == Some(&json!("WordPress")) {
        messages.push(("title", "That title is reserved."));
    }
    if input
        .get("email")
        .and_then(|v| v.as_str())
        .is_some_and(|email| !email.contains('.'))
    {
        messages.push(("email", "The email address isn't correct."));
    }
    (!messages.is_empty()).then(|| ValidationError::with_messages(messages))
}

impl RuntimeData for Settings {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        let rows = self.0.lock().unwrap().clone();
        Box::pin(async move {
            Ok(Page {
                total: rows.len() as i64,
                rows,
            })
        })
    }

    fn find<'a>(&'a self, _: &'a Database, id: i64) -> RuntimeFuture<'a, Option<Row>> {
        let row = self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|row| row["id"] == json!(id))
            .cloned();
        Box::pin(async move { Ok(row) })
    }

    fn create<'a>(&'a self, _: &'a Database, mut input: Row) -> RuntimeFuture<'a, Row> {
        let mut rows = self.0.lock().unwrap();
        input.insert("id".into(), json!(rows.len() as i64 + 1));
        rows.push(input.clone());
        Box::pin(async move { Ok(input) })
    }

    fn update<'a>(&'a self, _: &'a Database, id: i64, input: Row) -> RuntimeFuture<'a, Row> {
        let mut rows = self.0.lock().unwrap();
        let row = rows.iter_mut().find(|row| row["id"] == json!(id)).unwrap();
        row.extend(input);
        let row = row.clone();
        Box::pin(async move { Ok(row) })
    }

    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }

    fn try_create_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        input: Row,
    ) -> RuntimeFuture<'a, Result<Row, ValidationError>> {
        Box::pin(async move {
            if let Some(refused) = refusal(&input) {
                return Ok(Err(refused));
            }
            self.create_for(db, user, input).await.map(Ok)
        })
    }

    fn try_update_for<'a>(
        &'a self,
        db: &'a Database,
        user: &'a ActingUser,
        id: i64,
        input: Row,
    ) -> RuntimeFuture<'a, Result<Row, ValidationError>> {
        Box::pin(async move {
            if let Some(refused) = refusal(&input) {
                return Ok(Err(refused));
            }
            self.update_for(db, user, id, input).await.map(Ok)
        })
    }
}

fn text(name: &str, label: &str) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind: FieldKindDef::Text,
        required: true,
    }
}

fn setup() -> Settings {
    let settings = Settings::default();
    let _ = unregister_runtime(SLUG);
    register_runtime(
        RuntimeResource::new(SLUG, "Settings", "Setting", settings.clone())
            .field(text("title", "Site title"))
            .field(text("email", "Administration email"))
            .column(ColumnDef {
                key: "title".into(),
                label: "Site title".into(),
                sortable: false,
                searchable: false,
            }),
    )
    .expect("register");
    settings
}

fn submit(client: &Client, path: &str, body: &str) -> (Status, String) {
    let response = client
        .post(path)
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    let status = response.status();
    (status, response.into_string().unwrap_or_default())
}

fn error_slot(message: &str) -> String {
    format!(r#"<p class="mt-1.5 mb-0 text-sm text-error-emphasis">{message}</p>"#)
}

#[test]
fn a_refused_create_shows_each_message_under_its_field_with_the_values_kept() {
    let _guard = reset();
    let settings = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/new")).dispatch();
    let (status, html) = submit(
        &client,
        &format!("/admin/{SLUG}"),
        "title=WordPress&email=admin%40localhost",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::UnprocessableEntity);
    let title = html.find(r#"data-ignite-field="title""#).unwrap();
    let email = html.find(r#"data-ignite-field="email""#).unwrap();
    let title_error = html.find(&error_slot("That title is reserved.")).unwrap();
    let email_error = html
        .find(&error_slot("The email address isn&#x27;t correct."))
        .or_else(|| html.find(&error_slot("The email address isn&#39;t correct.")))
        .expect(&html);
    assert!(title < title_error && title_error < email, "{html}");
    assert!(email < email_error, "{html}");
    assert!(html.contains(r#"value="WordPress""#), "{html}");
    assert!(html.contains(r#"value="admin@localhost""#), "{html}");
    assert!(settings.0.lock().unwrap().is_empty(), "nothing was saved");
}

#[test]
fn a_refused_update_shows_its_message_and_an_accepted_one_saves() {
    let _guard = reset();
    let settings = setup();
    settings.0.lock().unwrap().push(
        json!({"id": 1, "title": "My site", "email": "a@example.test"})
            .as_object()
            .unwrap()
            .clone(),
    );
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}/1/edit")).dispatch();
    let (status, html) = submit(
        &client,
        &format!("/admin/{SLUG}/1"),
        "title=WordPress&email=a%40example.test",
    );
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(
        html.contains(&error_slot("That title is reserved.")),
        "{html}"
    );
    assert!(html.contains(r#"value="WordPress""#), "{html}");
    assert_eq!(settings.0.lock().unwrap()[0]["title"], json!("My site"));

    let (status, _) = submit(
        &client,
        &format!("/admin/{SLUG}/1"),
        "title=Blog&email=a%40example.test",
    );
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(settings.0.lock().unwrap()[0]["title"], json!("Blog"));
}

#[test]
fn validation_error_keeps_the_last_message_of_a_field() {
    let refused = ValidationError::with_messages([("title", "One."), ("title", "Two.")]);
    assert_eq!(refused.errors().len(), 1);
    assert_eq!(refused.errors()["title"], "Two.");
    assert_eq!(refused.to_string(), "the save was refused on: title");
}
