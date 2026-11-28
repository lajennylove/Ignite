//! Runtime abilities for the Roles screen, as a Filament resource policy checking the
//! permission it chooses (`viewAny`, `create`, `update`, `delete`) and `canAccess()`: a runtime
//! resource names the ability each of its actions needs (`RuntimeResource::ability`, such as a
//! post type's `edit_books` and `delete_books`) and an ability every action needs
//! (`access_ability`, such as `manage_options`). The abilities gate the resource as a whole:
//! its navigation entry, list, forms, saves and deletes. A resource registered while the app
//! runs gets its abilities as permissions on the next request, so the Roles screen lists
//! them without a restart, and `register_abilities` adds others at run time. Runs against the
//! test database for the user.

mod common;

use common::memory::Memory;
use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, FieldDef, FieldKindDef, ResourceAction,
    RuntimeResource,
};
use ignite_panel::{authz, users};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::json;

const BOOKS: &str = "ra-books";
const SETTINGS: &str = "ra-settings";

fn resource(slug: &str, data: Memory) -> RuntimeResource {
    RuntimeResource::new(slug, "Books", "Book", data)
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
}

/// Books, a post type whose capability type is `book`, as WordPress maps its capabilities.
fn books() -> Memory {
    let data = Memory::with(vec![json!({"id": 1, "title": "Dune"})]);
    let _ = unregister_runtime(BOOKS);
    register_runtime(
        resource(BOOKS, data.clone())
            .ability(ResourceAction::ViewAny, "edit_books")
            .ability(ResourceAction::View, "edit_books")
            .ability(ResourceAction::Create, "edit_books")
            .ability(ResourceAction::Update, "edit_books")
            .ability(ResourceAction::Delete, "delete_books"),
    )
    .expect("register");
    data
}

/// A user with the role `role` holding `abilities`, signed in.
fn user_with(role: &str, abilities: &[&str]) -> Client {
    let t = test_db();
    let email = format!("{role}@example.test");
    t.run(async {
        authz::create_role(&t.db, role).await.unwrap();
        users::create_user(&t.db, role, &email, PASSWORD)
            .await
            .unwrap();
        authz::assign_role(&t.db, &email, role).await.unwrap();
    });
    // The first signed-in request registers the runtime resources' abilities.
    let client = signed_in_as(None, &email, PASSWORD);
    client.get("/admin").dispatch();
    t.run(async {
        for ability in abilities {
            authz::grant_permission(&t.db, role, ability).await.unwrap();
        }
    });
    client
}

fn status(client: &Client, path: &str) -> Status {
    client.get(path).dispatch().status()
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
fn a_resource_registered_at_run_time_has_its_abilities_on_the_roles_screen() {
    let _guard = reset();
    let admin = signed_in_with(|ignite| ignite.user_management());
    books();
    let html = admin
        .get("/admin/roles/new")
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(BOOKS).expect("unregister");
    assert!(
        html.contains(r#"<option value="edit_books" >edit_books</option>"#),
        "{html}"
    );
    assert!(html.contains(r#"<option value="delete_books" >delete_books</option>"#));
    assert!(
        !html.contains(r#"value="ra-books.update""#),
        "a named ability replaces the default one"
    );

    // An ability the app registers while it runs is listed too.
    let t = test_db();
    t.run(async {
        ignite_panel::register_abilities(&t.db, ["manage_network_options"])
            .await
            .unwrap()
    });
    let html = admin
        .get("/admin/roles/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(html.contains(r#"value="manage_network_options""#), "{html}");
}

#[test]
fn the_named_abilities_gate_the_resource_as_a_whole() {
    let _guard = reset();
    let data = books();
    let editor = user_with("editor", &["edit_books"]);
    assert_eq!(status(&editor, &format!("/admin/{BOOKS}")), Status::Ok);
    let dashboard = editor.get("/admin").dispatch().into_string().unwrap();
    assert!(
        dashboard.contains(r#"href="/admin/ra-books""#),
        "{dashboard}"
    );
    assert_eq!(status(&editor, &format!("/admin/{BOOKS}/new")), Status::Ok);
    assert_eq!(
        status(&editor, &format!("/admin/{BOOKS}/1/edit")),
        Status::Ok
    );
    assert_eq!(
        post(&editor, &format!("/admin/{BOOKS}/1"), "title=Dune+Messiah"),
        Status::SeeOther
    );
    // Without `delete_books`, no delete.
    assert_eq!(
        post(&editor, &format!("/admin/{BOOKS}/1/delete"), ""),
        Status::Forbidden
    );
    assert_eq!(data.rows().len(), 1);

    // The default abilities of the resource are not registered for it, and are not enough:
    // without the named one nothing shows.
    let t = test_db();
    t.run(async {
        ignite_panel::register_abilities(&t.db, ["ra-books.viewAny", "ra-books.update"])
            .await
            .unwrap()
    });
    let reader = user_with("reader", &["ra-books.viewAny", "ra-books.update"]);
    assert_eq!(
        status(&reader, &format!("/admin/{BOOKS}")),
        Status::Forbidden
    );
    assert_eq!(
        status(&reader, &format!("/admin/{BOOKS}/1/edit")),
        Status::Forbidden
    );
    let dashboard = reader.get("/admin").dispatch().into_string().unwrap();
    unregister_runtime(BOOKS).expect("unregister");
    assert!(!dashboard.contains(r#"href="/admin/ra-books""#));
}

#[test]
fn the_access_ability_is_needed_beside_each_actions_own() {
    let _guard = reset();
    let _ = unregister_runtime(SETTINGS);
    register_runtime(
        resource(
            SETTINGS,
            Memory::with(vec![json!({"id": 1, "title": "Site"})]),
        )
        .access_ability("manage_options"),
    )
    .expect("register");
    let author = user_with("author", &["ra-settings.viewAny", "ra-settings.update"]);
    assert_eq!(
        status(&author, &format!("/admin/{SETTINGS}")),
        Status::Forbidden
    );
    assert_eq!(
        status(&author, &format!("/admin/{SETTINGS}/1/edit")),
        Status::Forbidden
    );

    let admin = user_with(
        "administrator",
        &[
            "manage_options",
            "ra-settings.viewAny",
            "ra-settings.update",
        ],
    );
    assert_eq!(status(&admin, &format!("/admin/{SETTINGS}")), Status::Ok);
    assert_eq!(
        status(&admin, &format!("/admin/{SETTINGS}/1/edit")),
        Status::Ok
    );
    assert_eq!(
        status(&admin, &format!("/admin/{SETTINGS}/new")),
        Status::Forbidden,
        "the access ability does not replace the action's own"
    );
    unregister_runtime(SETTINGS).expect("unregister");
}
