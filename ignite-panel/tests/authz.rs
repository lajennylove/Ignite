//! Integration tests for roles and permissions: the server refuses what a user may not do,
//! and the navigation and buttons show only what the user may do. Runs against `ignite_test`.

mod common;

use common::*;
use ignite_panel::authz::{self, AuthzError, SUPER_ADMIN};
use ignite_panel::users;
use rocket::http::{ContentType, Status};

const VIEWER: &str = "viewer@example.test";
const NOBODY: &str = "nobody@example.test";

fn connection() -> TestDb {
    test_db()
}

/// Creates a user with `email`, and a role `role` that holds `permissions`. Call it after a
/// client has started, so the permission rows exist.
fn user_with_role(email: &str, role: &str, permissions: &[&str]) {
    let conn = connection();
    conn.run(users::create_user(&conn.db, "Test user", email, PASSWORD))
        .expect("create user");
    conn.run(authz::create_role(&conn.db, role))
        .expect("create role");
    for permission in permissions {
        conn.run(authz::grant_permission(&conn.db, role, permission))
            .expect("grant permission");
    }
    conn.run(authz::assign_role(&conn.db, email, role))
        .expect("assign role");
}

#[test]
fn the_app_registers_one_permission_per_resource_action() {
    let _guard = reset();
    let _client = client(None);
    let _again = client(None);

    let count = connection().count(
        "SELECT COUNT(*) FROM permissions WHERE name LIKE ?",
        &["widgets.%".into()],
    );
    assert_eq!(count, 5, "viewAny, view, create, update and delete");
}

#[test]
fn a_viewer_without_delete_sees_no_delete_button_and_gets_403_on_delete() {
    let _guard = reset();
    let _ = client(None);
    user_with_role(VIEWER, "viewer", &["widgets.viewAny", "widgets.update"]);
    let viewer = signed_in_as(None, VIEWER, PASSWORD);

    let response = viewer.get("/admin/widgets").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().unwrap();
    assert!(
        body.contains("href=\"/admin/widgets/1/edit\""),
        "Edit is allowed"
    );
    assert!(!body.contains("/delete\""), "no Delete form: {body}");
    assert!(!body.contains("ig-btn-danger"), "no Delete button: {body}");
    assert!(
        !body.contains("href=\"/admin/widgets/new\""),
        "no New button: {body}"
    );

    let response = viewer
        .post("/admin/widgets/1/delete")
        .header(ContentType::Form)
        .body(with_token(&viewer, ""))
        .dispatch();
    assert_eq!(response.status(), Status::Forbidden);
    assert_eq!(count_widgets(), 3, "the row is still there");
}

#[test]
fn a_user_without_view_any_is_not_listed_and_gets_403_on_the_list() {
    let _guard = reset();
    let _ = client(None);
    user_with_role(NOBODY, "dashboard-only", &[]);
    let nobody = signed_in_as(None, NOBODY, PASSWORD);

    let response = nobody.get("/admin").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().unwrap();
    assert!(
        !body.contains("href=\"/admin/widgets\""),
        "no navigation entry: {body}"
    );
    assert!(
        !body.contains("font-semibold\">Widgets<"),
        "no stat card: {body}"
    );
    assert!(!body.contains("Latest widgets"), "no latest table: {body}");

    for path in [
        "/admin/widgets",
        "/admin/widgets?q=a",
        "/admin/widgets/new",
        "/admin/widgets/1/edit",
    ] {
        assert_eq!(
            nobody.get(path).dispatch().status(),
            Status::Forbidden,
            "{path}"
        );
    }
    let body = with_token(&nobody, "name=Date&price=5");
    let create = nobody
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(body)
        .dispatch();
    assert_eq!(create.status(), Status::Forbidden);
    assert_eq!(count_widgets(), 3, "nothing was created");
    assert_eq!(
        nobody.get("/admin/nope").dispatch().status(),
        Status::NotFound,
        "an unknown resource is not found, not forbidden"
    );
}

#[test]
fn a_super_admin_passes_every_resource_check() {
    let _guard = reset();
    let client = signed_in(None);

    let dashboard = client.get("/admin").dispatch().into_string().unwrap();
    assert!(
        dashboard.contains("href=\"/admin/widgets\""),
        "nav entry shown"
    );
    assert!(
        dashboard.contains("font-semibold\">Widgets<"),
        "stat card shown"
    );

    let list = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(list.contains("href=\"/admin/widgets/new\""));
    assert!(list.contains("/delete\""));
    assert!(list.contains("href=\"/admin/widgets/1/edit\""));

    assert_eq!(
        client.get("/admin/widgets/new").dispatch().status(),
        Status::Ok
    );
    let created = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(&client, "name=Date&price=5"))
        .dispatch();
    assert_eq!(created.status(), Status::SeeOther);
    assert_eq!(
        client.get("/admin/widgets/4/edit").dispatch().status(),
        Status::Ok
    );
    let updated = client
        .post("/admin/widgets/4")
        .header(ContentType::Form)
        .body(with_token(&client, "name=Dates&price=6"))
        .dispatch();
    assert_eq!(updated.status(), Status::SeeOther);
    let deleted = client
        .post("/admin/widgets/4/delete")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();
    assert_eq!(deleted.status(), Status::SeeOther);
    assert_eq!(count_widgets(), 3);
}

#[test]
fn a_user_who_may_only_create_sees_the_new_button_and_no_row_buttons() {
    let _guard = reset();
    let _ = client(None);
    user_with_role(VIEWER, "author", &["widgets.viewAny", "widgets.create"]);
    let author = signed_in_as(None, VIEWER, PASSWORD);

    let list = author
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(list.contains("href=\"/admin/widgets/new\""));
    assert!(!list.contains("/edit\""), "no Edit button: {list}");
    assert!(!list.contains("/delete\""), "no Delete button: {list}");

    assert_eq!(
        author.get("/admin/widgets/new").dispatch().status(),
        Status::Ok
    );
    let created = author
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(&author, "name=Date&price=5"))
        .dispatch();
    assert_eq!(created.status(), Status::SeeOther);
    assert_eq!(count_widgets(), 4);

    assert_eq!(
        author.get("/admin/widgets/1/edit").dispatch().status(),
        Status::Forbidden
    );
    let updated = author
        .post("/admin/widgets/1")
        .header(ContentType::Form)
        .body(with_token(&author, "name=Apple&price=9"))
        .dispatch();
    assert_eq!(updated.status(), Status::Forbidden);
}

#[test]
fn the_last_super_admin_cannot_lose_the_role() {
    let _guard = reset();
    let _ = client(None);
    let conn = connection();

    let refused = conn.run(authz::remove_role(&conn.db, EMAIL, SUPER_ADMIN));
    assert!(
        matches!(refused, Err(AuthzError::LastSuperAdmin)),
        "{refused:?}"
    );
    assert_eq!(
        conn.run(authz::user_roles(&conn.db, EMAIL)).unwrap(),
        vec![SUPER_ADMIN.to_string()],
        "the role is kept"
    );

    conn.run(users::create_user(&conn.db, "Second", NOBODY, PASSWORD))
        .unwrap();
    conn.run(authz::assign_role(&conn.db, NOBODY, SUPER_ADMIN))
        .unwrap();
    assert!(conn
        .run(authz::remove_role(&conn.db, EMAIL, SUPER_ADMIN))
        .unwrap());
    // NOBODY is now the last super-admin, so the same refusal applies to them.
    let refused = conn.run(authz::remove_role(&conn.db, NOBODY, SUPER_ADMIN));
    assert!(
        matches!(refused, Err(AuthzError::LastSuperAdmin)),
        "{refused:?}"
    );
}

#[test]
fn a_grant_names_an_existing_role_and_permission() {
    let _guard = reset();
    let _ = client(None);
    let conn = connection();
    conn.run(authz::create_role(&conn.db, "editors")).unwrap();

    assert!(matches!(
        conn.run(authz::grant_permission(
            &conn.db,
            "editors",
            "widgets.nothing"
        )),
        Err(AuthzError::NotFound(_))
    ));
    assert!(matches!(
        conn.run(authz::grant_permission(
            &conn.db,
            "missing",
            "widgets.viewAny"
        )),
        Err(AuthzError::NotFound(_))
    ));
    conn.run(authz::grant_permission(
        &conn.db,
        "editors",
        "widgets.viewAny",
    ))
    .unwrap();
    assert!(matches!(
        conn.run(authz::create_role(&conn.db, "editors")),
        Err(AuthzError::Exists(_))
    ));
}
