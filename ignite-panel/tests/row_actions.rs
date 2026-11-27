//! Row actions, as Filament's table actions (`recordActions()`): Restore, Move to trash and
//! Delete permanently on a posts resource whose rows live in memory. Each action has a label,
//! a link or a POST on the resource, a confirmation, a visibility rule and a colour; the panel
//! renders it on the rows it is visible on and runs it with the CSRF token and the resource's
//! update or delete authorization. Runs against the test database for the users and roles.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use ignite_core::{
    register_policy, register_runtime, unregister_policy, unregister_runtime, ActingUser,
    ActionAbility, BadgeColor, ColumnDef, Database, FieldDef, FieldKindDef, IgniteResource,
    ListFilter, ListQuery, Page, Policy, Row, RowAction, RuntimeData, RuntimeFuture,
    RuntimeResource,
};
use ignite_panel::authz;
use orbit_orm::OrbitModel;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde::Serialize;
use serde_json::json;

const SLUG: &str = "ra-posts";

/// The posts, in memory: ids 1 to 3, the third in the trash, the second locked.
#[derive(Clone)]
struct Posts(Arc<Mutex<Vec<Row>>>);

fn post(id: i64, title: &str, status: &str) -> Row {
    json!({"id": id, "title": title, "status": status, "slug": format!("post-{id}")})
        .as_object()
        .unwrap()
        .clone()
}

impl Posts {
    fn new() -> Self {
        Posts(Arc::new(Mutex::new(vec![
            post(1, "Hello", "publish"),
            post(2, "Locked", "draft"),
            post(3, "Old", "trash"),
        ])))
    }

    fn status(&self, id: i64) -> Option<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|row| row["id"] == json!(id))
            .map(|row| row["status"].as_str().unwrap().to_string())
    }

    fn set_status(&self, id: i64, status: &str) {
        for row in self.0.lock().unwrap().iter_mut() {
            if row["id"] == json!(id) {
                row.insert("status".into(), json!(status));
            }
        }
    }

    fn remove(&self, id: i64) {
        self.0.lock().unwrap().retain(|row| row["id"] != json!(id));
    }
}

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

    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }
}

fn in_trash() -> ListFilter {
    ListFilter::new().where_eq("status", "trash")
}

/// The posts resource with the three actions Mission Control uses, and a preview link.
fn resource(posts: &Posts) -> RuntimeResource {
    let trash = posts.clone();
    let restore = posts.clone();
    let delete = posts.clone();
    RuntimeResource::new(SLUG, "Posts", "Post", posts.clone())
        .field(FieldDef {
            name: "title".into(),
            label: "Title".into(),
            kind: FieldKindDef::Text,
            required: true,
        })
        .column(ColumnDef {
            key: "title".into(),
            label: "Title".into(),
            sortable: false,
            searchable: false,
        })
        .row_action(
            RowAction::make("preview")
                .label("Preview")
                .url("/blog/{slug}?preview=1"),
        )
        .row_action(
            RowAction::make("trash")
                .label("Move to trash")
                .hidden(|row| row.get("status") == Some(&json!("trash")))
                .success_notification_title("Post moved to the trash.")
                .action(move |_, _, id, record| {
                    let posts = trash.clone();
                    let locked = record.get("title") == Some(&json!("Locked"));
                    Box::pin(async move {
                        if locked {
                            return Ok(Err("This post is locked.".to_string()));
                        }
                        posts.set_status(id, "trash");
                        Ok(Ok(()))
                    })
                }),
        )
        .row_action(
            RowAction::make("restore")
                .visible_when(in_trash())
                .success_notification_title("Post restored.")
                .action(move |_, _, id, _| {
                    let posts = restore.clone();
                    Box::pin(async move {
                        posts.set_status(id, "draft");
                        Ok(Ok(()))
                    })
                }),
        )
        .row_action(
            RowAction::make("delete_permanently")
                .color(BadgeColor::Danger)
                .authorize(ActionAbility::Delete)
                .modal_description("Delete this post permanently? This cannot be undone.")
                .visible_when(in_trash())
                .action(move |_, _, id, _| {
                    let posts = delete.clone();
                    Box::pin(async move {
                        posts.remove(id);
                        Ok(Ok(()))
                    })
                }),
        )
}

/// Registers the resource on fresh posts. Returns the posts, to read what the actions did.
fn setup() -> Posts {
    let posts = Posts::new();
    let _ = unregister_runtime(SLUG);
    register_runtime(resource(&posts)).expect("register");
    posts
}

fn post_action(client: &Client, id: i64, name: &str, body: &str) -> (Status, Option<String>) {
    let response = client
        .post(format!("/admin/{SLUG}/{id}/actions/{name}"))
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    let location = response.headers().get_one("Location").map(str::to_string);
    (response.status(), location)
}

/// The cell of actions of the row of `title`.
fn actions_of(html: &str, title: &str) -> String {
    let cell = html.find(&format!("<td>{title}</td>")).expect("the row");
    let end = html[cell..].find("</tr>").unwrap() + cell;
    html[cell..end].to_string()
}

#[test]
fn each_row_shows_the_actions_visible_on_it() {
    let _guard = reset();
    let _posts = setup();
    let client = signed_in(None);
    let html = client
        .get(format!("/admin/{SLUG}"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(SLUG).expect("unregister");

    let hello = actions_of(&html, "Hello");
    assert!(
        hello.contains(
            r#"<a class="ig-btn ig-btn-ghost px-3 py-1" href="/blog/post-1?preview=1">Preview</a>"#
        ),
        "{hello}"
    );
    assert!(
        hello.contains(&format!(
            r#"<form class="inline" method="post" action="/admin/{SLUG}/1/actions/trash"><input type="hidden" name="_token" value=""#
        )),
        "{hello}"
    );
    assert!(hello.contains(r#"<input type="hidden" name="_back" value="/admin/ra-posts">"#));
    assert!(hello.contains(">Move to trash</button>"), "{hello}");
    assert!(!hello.contains("Restore"), "{hello}");
    assert!(!hello.contains("Delete permanently"), "{hello}");

    let old = actions_of(&html, "Old");
    assert!(!old.contains("Move to trash"), "{old}");
    assert!(
        old.contains(
            r#"<button type="submit" class="ig-btn ig-btn-ghost px-3 py-1">Restore</button>"#
        ),
        "{old}"
    );
    assert!(
        old.contains(&format!(
            r#"action="/admin/{SLUG}/3/actions/delete_permanently" data-confirm="Delete this post permanently? This cannot be undone.">"#
        )),
        "{old}"
    );
    assert!(
        old.contains(
            r#"class="ig-btn ig-btn-danger px-3 py-1 ig-action-danger">Delete permanently</button>"#
        ),
        "{old}"
    );
}

#[test]
fn a_post_runs_the_action_and_shows_its_message() {
    let _guard = reset();
    let posts = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}")).dispatch();
    let back = format!("/admin/{SLUG}?tab=all&page=2");
    let (status, location) = post_action(
        &client,
        1,
        "trash",
        &format!("_back={}", back.replace('&', "%26").replace('?', "%3F")),
    );
    assert_eq!(status, Status::SeeOther);
    assert_eq!(location.as_deref(), Some(back.as_str()));
    assert_eq!(posts.status(1).as_deref(), Some("trash"));
    let html = client
        .get(format!("/admin/{SLUG}"))
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        html.contains("Post moved to the trash."),
        "the success message"
    );

    let (status, _) = post_action(&client, 3, "restore", "");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(posts.status(3).as_deref(), Some("draft"));

    posts.set_status(2, "trash");
    let (status, location) = post_action(&client, 2, "delete_permanently", "");
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    // Without `_back` the list shows again at its first page.
    assert_eq!(location.as_deref(), Some("/admin/ra-posts"));
    assert_eq!(posts.status(2), None);
}

#[test]
fn a_back_address_off_the_list_is_ignored() {
    let _guard = reset();
    let posts = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}")).dispatch();
    for back in [
        "https://evil.example/",
        "/admin/ra-postsevil",
        "/admin/widgets",
    ] {
        posts.set_status(3, "trash");
        let (_, location) = post_action(&client, 3, "restore", &format!("_back={back}"));
        assert_eq!(location.as_deref(), Some("/admin/ra-posts"), "{back}");
    }
    unregister_runtime(SLUG).expect("unregister");
}

#[test]
fn an_action_may_refuse_with_a_message() {
    let _guard = reset();
    let posts = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}")).dispatch();
    let (status, _) = post_action(&client, 2, "trash", "");
    let html = client
        .get(format!("/admin/{SLUG}"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(posts.status(2).as_deref(), Some("draft"), "nothing changed");
    assert!(html.contains("This post is locked."), "the refusal shows");
    assert!(!html.contains("Post moved to the trash."));
}

#[test]
fn a_post_without_the_token_or_off_its_rule_is_refused() {
    let _guard = reset();
    let posts = setup();
    let client = signed_in(None);
    client.get(format!("/admin/{SLUG}")).dispatch();
    let without_token = client
        .post(format!("/admin/{SLUG}/1/actions/trash"))
        .header(ContentType::Form)
        .body("")
        .dispatch()
        .status();
    // Restore is visible only in the trash; post 1 is published.
    let (off_rule, _) = post_action(&client, 1, "restore", "");
    let (unknown, _) = post_action(&client, 1, "publish", "");
    let (link, _) = post_action(&client, 1, "preview", "");
    let (missing, _) = post_action(&client, 99, "trash", "");
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(without_token.code, 419);
    assert_eq!(off_rule, Status::Forbidden);
    assert_eq!(unknown, Status::NotFound);
    assert_eq!(link, Status::NotFound, "a link is not posted");
    assert_eq!(missing, Status::NotFound);
    assert_eq!(posts.status(1).as_deref(), Some("publish"));
}

/// Refuses the update of the locked post, as a resource's policy does.
struct NoLocked;

impl Policy for NoLocked {
    fn update<'a>(
        &'a self,
        _: &'a Database,
        _: &'a ActingUser,
        record: &'a Row,
    ) -> RuntimeFuture<'a, bool> {
        let allowed = record.get("title") != Some(&json!("Locked"));
        Box::pin(async move { Ok(allowed) })
    }
}

#[test]
fn the_action_needs_the_resources_update_or_delete_authorization() {
    let _guard = reset();
    let posts = setup();
    register_policy(SLUG, NoLocked);
    let t = test_db();
    t.run(async {
        ignite_panel::register_runtime_permissions(&t.db, SLUG)
            .await
            .expect("permissions");
        authz::create_role(&t.db, "ra-editors").await.expect("role");
        for action in ["viewAny", "update"] {
            authz::grant_permission(&t.db, "ra-editors", &format!("{SLUG}.{action}"))
                .await
                .expect("grant");
        }
        ignite_panel::users::create_user(&t.db, "Ed", "ed@example.test", PASSWORD)
            .await
            .expect("user");
        authz::assign_role(&t.db, "ed@example.test", "ra-editors")
            .await
            .expect("assign");
    });
    let client = signed_in_as(None, "ed@example.test", PASSWORD);
    let html = client
        .get(format!("/admin/{SLUG}"))
        .dispatch()
        .into_string()
        .unwrap();
    // Without the delete ability, Delete permanently is not shown and not run.
    let old = actions_of(&html, "Old");
    assert!(old.contains(">Restore</button>"), "{old}");
    assert!(!old.contains("Delete permanently"), "{old}");
    let (delete, _) = post_action(&client, 3, "delete_permanently", "");
    // The policy refuses the update of the locked post: its update actions are not shown.
    let locked = actions_of(&html, "Locked");
    assert!(!locked.contains("Move to trash"), "{locked}");
    assert!(
        locked.contains("Preview"),
        "a link needs no authorization: {locked}"
    );
    let (trash, _) = post_action(&client, 2, "trash", "");
    unregister_policy(SLUG);
    unregister_runtime(SLUG).expect("unregister");
    assert_eq!(delete, Status::Forbidden);
    assert_eq!(trash, Status::Forbidden);
    assert_eq!(posts.status(3).as_deref(), Some("trash"));
    assert_eq!(posts.status(2).as_deref(), Some("draft"));
}

#[derive(Debug, Clone, OrbitModel, Serialize, ignite_core::IgniteResource)]
#[orbit(table = "ra_notes")]
#[ignite(table = ra_notes, title = "Notes", row_actions = note_actions)]
pub struct Note {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

fn note_actions() -> Vec<RowAction> {
    vec![RowAction::make("archive").requires_confirmation()]
}

#[test]
fn the_derive_sets_the_row_actions() {
    let actions = <Note as IgniteResource>::row_actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].get_label(), "Archive");
    assert_eq!(
        actions[0].confirmation(),
        Some("Are you sure you would like to do this?")
    );
}

#[test]
fn a_list_without_row_actions_renders_as_before() {
    let _guard = reset();
    let client = signed_in(None);
    let html = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(!html.contains("/actions/"));
    assert!(!html.contains(r#"name="_back""#));
}
