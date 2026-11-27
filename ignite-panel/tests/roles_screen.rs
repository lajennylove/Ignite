//! The Roles screen, as Filament Shield's: the list of roles, creating a role with its
//! abilities, editing them, renaming and deleting, the app's declared roles (created once with
//! their default abilities) and the app's own abilities. Runs against the `ignite_test`
//! database.

mod common;

use common::*;
use ignite_panel::authz;
use ignite_panel::{Ignite, Role};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

fn panel(ignite: Ignite) -> Ignite {
    ignite
        .role(
            Role::make("author")
                .label("Author")
                .abilities(["widgets.viewAny", "widgets.create"]),
        )
        .abilities(["settings.manage"])
        .user_management()
}

fn post(client: &Client, path: &str, body: &str) -> (Status, String) {
    let response = client
        .post(path)
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    (
        response.status(),
        response.into_string().unwrap_or_default(),
    )
}

fn get(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    response.into_string().unwrap()
}

fn abilities(role: &str) -> Vec<String> {
    let t = test_db();
    t.run(async { authz::role_permissions(&t.db, role).await.unwrap() })
}

fn role_id(name: &str) -> i64 {
    test_db().count("SELECT id FROM roles WHERE name = ?", &[name.into()])
}

#[test]
fn the_roles_are_listed_with_their_labels_and_counts() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let list = get(&client, "/admin/roles");
    assert!(list.contains("super-admin"));
    assert!(list.contains("Super Admin"));
    assert!(list.contains("author"));
    assert!(list.contains("Author"));
    // The declared role starts with its abilities.
    assert_eq!(
        abilities("author"),
        vec!["widgets.create".to_string(), "widgets.viewAny".to_string()]
    );
}

#[test]
fn the_form_offers_every_ability_the_panel_knows() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let form = get(&client, "/admin/roles/new");
    for ability in [
        "widgets.viewAny",
        "widgets.create",
        "widgets.update",
        "widgets.delete",
        "users.viewAny",
        "roles.update",
        "settings.manage",
    ] {
        assert!(form.contains(&format!("value=\"{ability}\"")), "{ability}");
    }
}

#[test]
fn a_role_is_created_with_its_abilities() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let (status, body) = post(
        &client,
        "/admin/roles",
        "name=editor&abilities[]=&abilities[]=widgets.viewAny&abilities[]=widgets.update",
    );
    assert_eq!(status, Status::SeeOther, "{body}");
    assert_eq!(
        abilities("editor"),
        vec!["widgets.update".to_string(), "widgets.viewAny".to_string()]
    );

    // A name that is not valid, a taken name, an unknown ability.
    let (status, body) = post(&client, "/admin/roles", "name=Not+Valid");
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(body.contains("Use lowercase letters"));
    let (status, body) = post(&client, "/admin/roles", "name=editor");
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(body.contains("A role named editor already exists."));
    let (status, _) = post(
        &client,
        "/admin/roles",
        "name=other&abilities[]=no.such.ability",
    );
    assert_eq!(status, Status::UnprocessableEntity);
}

#[test]
fn a_roles_abilities_are_edited_and_the_role_renamed() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let id = role_id("author");
    let form = get(&client, &format!("/admin/roles/{id}/edit"));
    assert!(form.contains("value=\"author\""));

    let (status, body) = post(
        &client,
        &format!("/admin/roles/{id}"),
        "name=writer&abilities[]=&abilities[]=settings.manage",
    );
    assert_eq!(status, Status::SeeOther, "{body}");
    assert_eq!(abilities("writer"), vec!["settings.manage".to_string()]);

    let super_admin = role_id("super-admin");
    let (status, body) = post(
        &client,
        &format!("/admin/roles/{super_admin}"),
        "name=root&abilities[]=",
    );
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(body.contains("The super-admin role cannot be renamed or deleted."));
}

#[test]
fn a_role_is_deleted_but_not_super_admin() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let id = role_id("author");
    post(&client, &format!("/admin/roles/{id}/actions/delete"), "");
    assert_eq!(
        test_db().count("SELECT COUNT(*) FROM roles WHERE name = 'author'", &[]),
        0
    );
    assert!(get(&client, "/admin/roles").contains("Role deleted."));

    let super_admin = role_id("super-admin");
    post(
        &client,
        &format!("/admin/roles/{super_admin}/actions/delete"),
        "",
    );
    assert!(
        get(&client, "/admin/roles").contains("The super-admin role cannot be renamed or deleted.")
    );
    assert!(role_id("super-admin") > 0);
}

#[test]
fn a_declared_role_keeps_the_abilities_changed_on_the_screen() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let id = role_id("author");
    post(
        &client,
        &format!("/admin/roles/{id}"),
        "name=author&abilities[]=&abilities[]=widgets.viewAny",
    );
    // The app starts again: the role exists, so its abilities stay as the screen left them.
    let _again = client_with(panel);
    assert_eq!(abilities("author"), vec!["widgets.viewAny".to_string()]);
}
