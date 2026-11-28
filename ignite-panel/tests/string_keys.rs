//! String record keys, as Filament's `$recordRouteKeyName`: a runtime resource whose records
//! are addressed by a key other than the numeric `id` (a post type's key) uses that key in
//! its URLs and routes, and its data finds, updates and deletes rows by it. The key field is
//! set on create and locked after the first save with `disabled_on(Edit)`, as Filament's
//! `disabledOn('edit')`: shown disabled, never saved, whatever a request sends. Runs against
//! the test database for the user.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ActingUser, ColumnDef, Database, FieldDef, FieldKindDef,
    ListQuery, Operation, Page, Row, RuntimeData, RuntimeFuture, RuntimeResource, ValidationError,
};
use orbit_orm::Error;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::json;

const SLUG: &str = "sk-types";

/// Post types kept in memory, keyed by `key`: they have no numeric id.
#[derive(Clone, Default)]
struct Types(Arc<Mutex<Vec<Row>>>);

impl Types {
    fn get(&self, key: &str) -> Option<Row> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|row| row["key"] == json!(key))
            .cloned()
    }
}

impl RuntimeData for Types {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        let rows = self.0.lock().unwrap().clone();
        Box::pin(async move {
            Ok(Page {
                total: rows.len() as i64,
                rows,
            })
        })
    }
    fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async { Ok(None) })
    }
    fn create<'a>(&'a self, _: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        self.0.lock().unwrap().push(input.clone());
        Box::pin(async move { Ok(input) })
    }
    fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Err(Error::NotFound { table: "types" }) })
    }
    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }
    fn find_by_key_for<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        key: &'a str,
    ) -> RuntimeFuture<'a, Option<Row>> {
        let row = self.get(key);
        Box::pin(async move { Ok(row) })
    }
    fn try_update_by_key_for<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        key: &'a str,
        input: Row,
    ) -> RuntimeFuture<'a, Result<Row, ValidationError>> {
        let mut rows = self.0.lock().unwrap();
        let row = rows
            .iter_mut()
            .find(|row| row["key"] == json!(key))
            .cloned();
        let result = match row {
            Some(mut row) => {
                assert!(!input.contains_key("key"), "a disabled key is not saved");
                row.extend(input);
                let index = rows.iter().position(|r| r["key"] == json!(key)).unwrap();
                rows[index] = row.clone();
                Ok(Ok(row))
            }
            None => Err(Error::NotFound { table: "types" }),
        };
        Box::pin(async move { result })
    }
    fn delete_by_key_for<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        key: &'a str,
    ) -> RuntimeFuture<'a, u64> {
        let mut rows = self.0.lock().unwrap();
        let before = rows.len();
        rows.retain(|row| row["key"] != json!(key));
        let deleted = (before - rows.len()) as u64;
        Box::pin(async move { Ok(deleted) })
    }
}

fn field(name: &str, label: &str) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: label.into(),
        kind: FieldKindDef::Text,
        required: true,
    }
}

fn setup() -> Types {
    let data = Types::default();
    for (key, label) in [("book", "Books"), ("new type", "New types")] {
        data.0.lock().unwrap().push(
            json!({"key": key, "label": label})
                .as_object()
                .unwrap()
                .clone(),
        );
    }
    let _ = unregister_runtime(SLUG);
    register_runtime(
        RuntimeResource::new(SLUG, "Post types", "Post type", data.clone())
            .field(field("key", "Post type key"))
            .field(field("label", "Plural label"))
            .record_route_key_name("key")
            .disabled_on("key", &[Operation::Edit])
            .column(ColumnDef {
                key: "label".into(),
                label: "Plural label".into(),
                sortable: false,
                searchable: false,
            }),
    )
    .expect("register");
    data
}

fn get(client: &Client, path: &str) -> (Status, String) {
    let response = client.get(path).dispatch();
    let status = response.status();
    (status, response.into_string().unwrap_or_default())
}

fn post(client: &Client, path: &str, body: &str) -> Status {
    client
        .post(path)
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch()
        .status()
}

#[test]
fn the_list_links_each_record_by_its_key() {
    let _guard = reset();
    setup();
    let client = signed_in(None);
    let (status, html) = get(&client, &format!("/admin/{SLUG}"));
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::Ok, "{html}");
    assert!(
        html.contains(r#"href="/admin/sk-types/book/edit">Edit</a>"#),
        "{html}"
    );
    assert!(html.contains(r#"action="/admin/sk-types/book/delete""#));
    assert!(html.contains(r#"href="/admin/sk-types/new%20type/edit">Edit</a>"#));
}

#[test]
fn the_edit_form_is_addressed_by_the_key_and_locks_it() {
    let _guard = reset();
    let data = setup();
    let client = signed_in(None);
    let (status, html) = get(&client, &format!("/admin/{SLUG}/book/edit"));
    assert_eq!(status, Status::Ok);
    assert!(
        html.contains(r#"<h1 class="mb-6">Edit Post type #book</h1>"#),
        "{html}"
    );
    assert!(html.contains(r#"action="/admin/sk-types/book""#));
    assert!(html.contains(
        r#"<div data-ignite-field="key"><fieldset disabled style="border:0;margin:0;padding:0;min-width:0">"#
    ));
    assert!(html.contains(r#"name="key" type="text"  value="book""#));

    // The key sent anyway is not saved; the other fields are.
    assert_eq!(
        post(
            &client,
            &format!("/admin/{SLUG}/book"),
            "key=hacked&label=Novels"
        ),
        Status::SeeOther
    );
    assert_eq!(data.get("book").unwrap()["label"], json!("Novels"));
    assert!(data.get("hacked").is_none());

    // A key with a space is one path segment.
    let (status, html) = get(&client, &format!("/admin/{SLUG}/new%20type/edit"));
    assert_eq!(status, Status::Ok, "{html}");
    assert!(html.contains(r#"action="/admin/sk-types/new%20type""#));
    unregister_runtime(SLUG).expect("unregister");
}

#[test]
fn the_key_is_set_on_create_and_unknown_keys_are_404() {
    let _guard = reset();
    let data = setup();
    let client = signed_in(None);
    let (_, html) = get(&client, &format!("/admin/{SLUG}/new"));
    assert!(
        !html.contains("<fieldset disabled"),
        "the key is editable on create"
    );
    assert_eq!(
        post(&client, &format!("/admin/{SLUG}"), "key=movie&label=Movies"),
        Status::SeeOther
    );
    assert_eq!(data.get("movie").unwrap()["label"], json!("Movies"));

    let (status, _) = get(&client, &format!("/admin/{SLUG}/nothing/edit"));
    assert_eq!(status, Status::NotFound);
    assert_eq!(
        post(&client, &format!("/admin/{SLUG}/nothing"), "label=X"),
        Status::NotFound
    );
    assert_eq!(
        post(&client, &format!("/admin/{SLUG}/movie/delete"), ""),
        Status::SeeOther
    );
    unregister_runtime(SLUG).expect("unregister");
    assert!(data.get("movie").is_none());
}

#[test]
fn a_resource_with_numeric_ids_refuses_other_keys() {
    let _guard = reset();
    let client = signed_in(None);
    let (status, _) = get(&client, "/admin/widgets/abc/edit");
    assert_eq!(status, Status::NotFound);
    let (status, html) = get(&client, "/admin/widgets/1/edit");
    assert_eq!(status, Status::Ok);
    assert!(html.contains(r#"action="/admin/widgets/1""#));
}
