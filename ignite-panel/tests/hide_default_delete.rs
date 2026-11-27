//! A resource hides the built-in Delete button on the records that meet a filter, as
//! Filament's `DeleteAction::make()->hidden(fn ($record) => $record->trashed())`: a post in the
//! trash shows no Delete (WordPress offers only Restore and Delete Permanently there), and the
//! panel refuses its delete. Runs against the test database for the user.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, Database, IgniteResource, ListFilter,
    ListQuery, Page, Row, RuntimeData, RuntimeFuture, RuntimeResource,
};
use orbit_orm::OrbitModel;
use rocket::http::{ContentType, Status};
use serde::Serialize;
use serde_json::json;

const SLUG: &str = "hd-posts";

#[derive(Clone)]
struct Posts(Arc<Mutex<Vec<Row>>>);

impl RuntimeData for Posts {
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

    fn create<'a>(&'a self, _: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move { Ok(input) })
    }

    fn update<'a>(&'a self, _: &'a Database, _: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move { Ok(input) })
    }

    fn delete<'a>(&'a self, _: &'a Database, id: i64) -> RuntimeFuture<'a, u64> {
        let mut rows = self.0.lock().unwrap();
        let before = rows.len();
        rows.retain(|row| row["id"] != json!(id));
        let deleted = (before - rows.len()) as u64;
        Box::pin(async move { Ok(deleted) })
    }
}

fn in_trash() -> ListFilter {
    ListFilter::new().where_eq("status", "trash")
}

fn setup() -> Posts {
    let rows = [(1, "Hello", "publish"), (2, "Old", "trash")]
        .iter()
        .map(|(id, title, status)| {
            json!({"id": id, "title": title, "status": status})
                .as_object()
                .unwrap()
                .clone()
        })
        .collect();
    let posts = Posts(Arc::new(Mutex::new(rows)));
    let _ = unregister_runtime(SLUG);
    register_runtime(
        RuntimeResource::new(SLUG, "Posts", "Post", posts.clone())
            .column(ColumnDef {
                key: "title".into(),
                label: "Title".into(),
                sortable: false,
                searchable: false,
            })
            .hide_default_delete_when(in_trash()),
    )
    .expect("register");
    posts
}

#[test]
fn the_delete_button_is_absent_on_matching_rows_and_present_otherwise() {
    let _guard = reset();
    let _posts = setup();
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(SLUG).expect("unregister");
    assert!(
        html.contains(&format!(r#"action="/admin/{SLUG}/1/delete""#)),
        "the published post keeps its Delete: {html}"
    );
    assert!(
        !html.contains(&format!(r#"action="/admin/{SLUG}/2/delete""#)),
        "the trashed post has no Delete: {html}"
    );
    // Edit stays on both.
    assert!(html.contains(&format!(r#"href="/admin/{SLUG}/2/edit""#)));
}

#[test]
fn the_delete_of_a_matching_record_is_refused() {
    let _guard = reset();
    let posts = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}")).dispatch();
    let delete = |id: i64| {
        client
            .post(format!("/admin/{SLUG}/{id}/delete"))
            .header(ContentType::Form)
            .body(with_token(&client, ""))
            .dispatch()
            .status()
    };
    let trashed = delete(2);
    let published = delete(1);
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(trashed, Status::Forbidden);
    assert_eq!(published, Status::SeeOther);
    let left: Vec<i64> = posts
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect();
    assert_eq!(left, [2]);
}

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "hd_notes")]
#[ignite(table = hd_notes, title = "Notes", hide_default_delete_when = in_trash)]
pub struct Note {
    id: i32,
    #[orbit(fillable)]
    status: String,
}

#[test]
fn the_derive_sets_the_filter() {
    assert_eq!(
        <Note as IgniteResource>::hide_default_delete_when(),
        Some(in_trash())
    );
    assert_eq!(
        <Widgets as IgniteResource>::hide_default_delete_when(),
        None
    );
}
