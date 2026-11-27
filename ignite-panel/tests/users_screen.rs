//! The Users screen, as WordPress's Users screens and a Filament `UserResource`: the list with
//! its search and a view per role, adding a user with roles, editing one, disabling, enabling
//! and deleting, and an app extending the screen. Runs against the `ignite_test` database.

mod common;

use common::*;
use ignite_core::{ColumnDef, RowAction};
use ignite_panel::authz::{self, SUPER_ADMIN};
use ignite_panel::screens::users_resource;
use ignite_panel::{users, Ignite, Role};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

fn panel(ignite: Ignite) -> Ignite {
    ignite
        .role(
            Role::make("author")
                .label("Author")
                .ability("widgets.viewAny"),
        )
        .user_management()
}

fn post(client: &Client, path: &str, body: &str) -> (Status, Option<String>, String) {
    let response = client
        .post(path)
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    let status = response.status();
    let location = response.headers().get_one("Location").map(str::to_string);
    (status, location, response.into_string().unwrap_or_default())
}

fn get(client: &Client, path: &str) -> (Status, String) {
    let response = client.get(path).dispatch();
    (
        response.status(),
        response.into_string().unwrap_or_default(),
    )
}

fn user_id(email: &str) -> i64 {
    let t = test_db();
    t.run(async {
        users::find_by_email(&t.db, email)
            .await
            .unwrap()
            .unwrap()
            .id
    })
}

fn roles_of(email: &str) -> Vec<String> {
    let t = test_db();
    t.run(async { authz::user_roles(&t.db, email).await.unwrap() })
}

fn add_ada(client: &Client) {
    let (status, location, body) = post(
        client,
        "/admin/users",
        "name=Ada+Lovelace&email=ada%40example.test&password=analytical+engine&roles[]=&roles[]=author",
    );
    assert_eq!(status, Status::SeeOther, "{body}");
    assert_eq!(location.as_deref(), Some("/admin/users"));
}

#[test]
fn the_users_screen_is_listed_with_its_add_new_link() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let (_, dashboard) = get(&client, "/admin");
    assert!(dashboard.contains("href=\"/admin/users\""));
    assert!(dashboard.contains(">Users<"));
    assert!(dashboard.contains("href=\"/admin/users/new\""));
    assert!(dashboard.contains("href=\"/admin/roles\""));

    let (status, list) = get(&client, "/admin/users");
    assert_eq!(status, Status::Ok);
    for text in [EMAIL, "Admin", "Super Admin", "Active", "Roles", "Status"] {
        assert!(list.contains(text), "{text}");
    }
    // WordPress's views: All, then a view per role that has users.
    assert!(list.contains("tab=all"));
    assert!(list.contains("tab=super-admin"));
    // The built-in Delete button is a row action, so a refusal is a message.
    assert!(list.contains("/actions/delete"));
}

#[test]
fn a_user_is_added_with_a_name_email_password_and_roles() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let (status, form) = get(&client, "/admin/users/new");
    assert_eq!(status, Status::Ok);
    assert!(form.contains("name=\"password\""));
    assert!(form.contains("value=\"author\""));
    assert!(form.contains(">Author<"));

    add_ada(&client);
    assert_eq!(roles_of("ada@example.test"), vec!["author".to_string()]);

    // The new user signs in with the password.
    let ada = client_with(panel);
    ada.get("/admin/login").dispatch();
    let (status, _, _) = post(
        &ada,
        "/admin/login",
        "email=ada%40example.test&password=analytical+engine",
    );
    assert_eq!(status, Status::SeeOther);
}

#[test]
fn a_short_password_or_a_taken_email_is_refused_on_the_form() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let (status, _, body) = post(
        &client,
        "/admin/users",
        "name=Short&email=short%40example.test&password=short",
    );
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(body.contains("The password must be at least 8 characters."));

    let (status, _, body) = post(
        &client,
        "/admin/users",
        &format!(
            "name=Twin&email={}&password=long+enough",
            EMAIL.replace('@', "%40")
        ),
    );
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(body.contains("Another user already has this email."));

    let (status, _, _) = post(
        &client,
        "/admin/users",
        "name=Ghost&email=ghost%40example.test&password=long+enough&roles[]=no-such-role",
    );
    assert_eq!(status, Status::UnprocessableEntity, "an unknown role");
}

#[test]
fn the_list_searches_and_filters_by_role() {
    let _guard = reset();
    let client = signed_in_with(panel);
    add_ada(&client);

    let (_, list) = get(&client, "/admin/users?q=lovelace");
    assert!(list.contains("ada@example.test"));
    assert!(!list.contains(EMAIL));

    let (_, list) = get(&client, "/admin/users?tab=author");
    assert!(list.contains("ada@example.test"));
    assert!(!list.contains(EMAIL));
    assert!(list.contains("Author"));

    let (_, list) = get(&client, "/admin/users?tab=all");
    assert!(list.contains("ada@example.test"));
    assert!(list.contains(EMAIL));
}

#[test]
fn a_user_is_edited_and_a_blank_password_keeps_the_old_one() {
    let _guard = reset();
    let client = signed_in_with(panel);
    add_ada(&client);
    let id = user_id("ada@example.test");

    let (status, form) = get(&client, &format!("/admin/users/{id}/edit"));
    assert_eq!(status, Status::Ok);
    assert!(form.contains("value=\"Ada Lovelace\""));

    let (status, _, body) = post(
        &client,
        &format!("/admin/users/{id}"),
        "name=Augusta+Ada&email=augusta%40example.test&password=&roles[]=",
    );
    assert_eq!(status, Status::SeeOther, "{body}");
    let t = test_db();
    let user = t.run(async { users::find_by_id(&t.db, id).await.unwrap().unwrap() });
    assert_eq!(user.name, "Augusta Ada");
    assert_eq!(user.email, "augusta@example.test");
    assert!(users::verify_password(
        &user.password_hash,
        "analytical engine"
    ));
    assert!(roles_of("augusta@example.test").is_empty());

    // A new password is stored.
    let (status, _, _) = post(
        &client,
        &format!("/admin/users/{id}"),
        "name=Augusta+Ada&email=augusta%40example.test&password=difference+engine&roles[]=author",
    );
    assert_eq!(status, Status::SeeOther);
    let user = t.run(async { users::find_by_id(&t.db, id).await.unwrap().unwrap() });
    assert!(users::verify_password(
        &user.password_hash,
        "difference engine"
    ));
    assert_eq!(roles_of("augusta@example.test"), vec!["author".to_string()]);
}

#[test]
fn the_last_super_admin_keeps_the_role() {
    let _guard = reset();
    let client = signed_in_with(panel);
    let id = user_id(EMAIL);
    let (status, _, body) = post(
        &client,
        &format!("/admin/users/{id}"),
        &format!(
            "name=Admin&email={}&password=&roles[]=author",
            EMAIL.replace('@', "%40")
        ),
    );
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(body.contains("last super-admin"));
    assert!(roles_of(EMAIL).contains(&SUPER_ADMIN.to_string()));
}

#[test]
fn a_user_is_disabled_enabled_and_deleted_but_not_the_users_own_account() {
    let _guard = reset();
    let client = signed_in_with(panel);
    add_ada(&client);
    let id = user_id("ada@example.test");

    let (status, location, _) = post(&client, &format!("/admin/users/{id}/actions/disable"), "");
    assert_eq!(status, Status::SeeOther);
    assert_eq!(location.as_deref(), Some("/admin/users"));
    let t = test_db();
    let disabled = |t: &TestDb| {
        t.run(async { users::find_by_id(&t.db, id).await.unwrap().unwrap() })
            .disabled_at
            .is_some()
    };
    assert!(disabled(&t));
    let (_, list) = get(&client, "/admin/users");
    assert!(list.contains("User disabled."));
    assert!(list.contains("Disabled"));
    assert!(list.contains(&format!("/admin/users/{id}/actions/enable")));

    post(&client, &format!("/admin/users/{id}/actions/enable"), "");
    assert!(!disabled(&t));

    // The signed-in user's own account.
    let own = user_id(EMAIL);
    post(&client, &format!("/admin/users/{own}/actions/delete"), "");
    let (_, list) = get(&client, "/admin/users");
    assert!(list.contains("You cannot delete your own account."));
    post(&client, &format!("/admin/users/{own}/actions/disable"), "");
    assert!(get(&client, "/admin/users")
        .1
        .contains("You cannot disable your own account."));

    // The built-in delete route is refused: delete is the row action.
    let (status, _, _) = post(&client, &format!("/admin/users/{id}/delete"), "");
    assert_eq!(status, Status::Forbidden);

    post(&client, &format!("/admin/users/{id}/actions/delete"), "");
    assert!(t
        .run(async { users::find_by_id(&t.db, id).await.unwrap() })
        .is_none());
    assert!(get(&client, "/admin/users").1.contains("User deleted."));
}

#[test]
fn a_user_who_is_not_a_super_admin_cannot_give_or_change_super_admin() {
    let _guard = reset();
    let t = test_db();
    let client = client_with(|ignite| {
        ignite
            .role(Role::make("manager").abilities([
                "users.viewAny",
                "users.create",
                "users.update",
                "users.delete",
            ]))
            .user_management()
    });
    t.run(async {
        users::create_user(&t.db, "Manager", "manager@example.test", PASSWORD)
            .await
            .unwrap();
        authz::assign_role(&t.db, "manager@example.test", "manager")
            .await
            .unwrap();
    });
    client.get("/admin/login").dispatch();
    post(
        &client,
        "/admin/login",
        &format!("email=manager%40example.test&password={PASSWORD}"),
    );
    let (status, form) = get(&client, "/admin/users/new");
    assert_eq!(status, Status::Ok);
    assert!(!form.contains("value=\"super-admin\""));

    let (status, _, _) = post(
        &client,
        "/admin/users",
        "name=Sneaky&email=sneaky%40example.test&password=long+enough&roles[]=super-admin",
    );
    assert_eq!(status, Status::UnprocessableEntity);

    let admin = user_id(EMAIL);
    post(
        &client,
        &format!("/admin/users/{admin}/actions/disable"),
        "",
    );
    assert!(get(&client, "/admin/users")
        .1
        .contains("Only a super-admin can disable a super-admin."));
    let (status, _, _) = post(
        &client,
        &format!("/admin/users/{admin}"),
        &format!(
            "name=Owned&email={}&password=&roles[]=",
            EMAIL.replace('@', "%40")
        ),
    );
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(roles_of(EMAIL).contains(&SUPER_ADMIN.to_string()));
}

#[test]
fn an_app_extends_the_users_screen() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        ignite.runtime_resource(
            users_resource()
                .navigation_label("People")
                .navigation_create_label("Add New User")
                .column(ColumnDef {
                    key: "id".into(),
                    label: "User ID".into(),
                    sortable: false,
                    searchable: false,
                })
                .row_action(RowAction::make("profile").url("/people/{id}")),
        )
    });
    let (_, dashboard) = get(&client, "/admin");
    assert!(dashboard.contains(">People<"));
    assert!(dashboard.contains("Add New User"));
    assert!(
        !dashboard.contains("href=\"/admin/roles\""),
        "only the users screen"
    );
    let (_, list) = get(&client, "/admin/users");
    assert!(list.contains("User ID"));
    assert!(list.contains(&format!("/people/{}", user_id(EMAIL))));
}
