//! The panel's checks use the abilities of the user's roles: a role without an ability cannot
//! see or do the action (an author cannot open the Settings screens; an administrator can), a
//! change on the Roles screen applies from the next request, and the app's own abilities guard
//! its routes and navigation items. The super-admin passes every check, as before. Runs against
//! the `ignite_test` database.

mod common;

use common::*;
use ignite_core::{
    ColumnDef, FieldDef, FieldKindDef, ListQuery, Page, Row, RuntimeData, RuntimeFuture,
    RuntimeResource,
};
use ignite_panel::{authz, users, AdminUser, Ignite, NavigationItem, Role};
use orbit_orm::Database;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use rocket::{get, routes};
use serde_json::json;

/// One settings row in memory.
struct SettingsData;

fn setting() -> Row {
    json!({"id": 1, "blogname": "My site"})
        .as_object()
        .unwrap()
        .clone()
}

impl RuntimeData for SettingsData {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async {
            Ok(Page {
                rows: vec![setting()],
                total: 1,
            })
        })
    }
    fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async { Ok(Some(setting())) })
    }
    fn create<'a>(&'a self, _: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move { Ok(input) })
    }
    fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Ok(setting()) })
    }
    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(1) })
    }
}

fn settings() -> RuntimeResource {
    RuntimeResource::new("settings", "Settings", "Setting", SettingsData)
        .field(FieldDef {
            name: "blogname".into(),
            label: "Site Title".into(),
            kind: FieldKindDef::Text,
            required: true,
        })
        .column(ColumnDef {
            key: "blogname".into(),
            label: "Site Title".into(),
            sortable: false,
            searchable: false,
        })
}

/// An app's own screen under the panel, guarded by the app's ability.
#[get("/admin/reading")]
fn reading(user: AdminUser) -> Result<&'static str, Status> {
    user.authorize("settings.manage")?;
    Ok("Reading settings")
}

fn ignite() -> Ignite {
    Ignite::new()
        .storage_dir(storage_dir())
        .resource::<Widgets>()
        .runtime_resource(settings())
        .abilities(["settings.manage"])
        .navigation_items([NavigationItem::make("Reading")
            .url("/admin/reading")
            .ability("settings.manage")])
        .role(
            Role::make("author")
                .label("Author")
                .abilities(["widgets.viewAny"]),
        )
        .role(
            Role::make("administrator")
                .label("Administrator")
                .abilities([
                    "widgets.viewAny",
                    "widgets.create",
                    "settings.viewAny",
                    "settings.update",
                    "settings.manage",
                ]),
        )
        .user_management()
}

fn client() -> Client {
    ensure_secret();
    Client::tracked(
        rocket::build()
            .mount("/", routes![reading])
            .attach(ignite().fairing()),
    )
    .expect("valid rocket")
}

/// A client signed in as a new user with the role `role`.
fn signed_in_with_role(role: &str) -> Client {
    let client = client();
    let email = format!("{role}@example.test");
    let t = test_db();
    t.run(async {
        users::create_user(&t.db, role, &email, PASSWORD)
            .await
            .unwrap();
        authz::assign_role(&t.db, &email, role).await.unwrap();
    });
    client.get("/admin/login").dispatch();
    let status = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email={}&password={PASSWORD}", email.replace('@', "%40")),
        ))
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther);
    client
}

fn status(client: &Client, path: &str) -> Status {
    client.get(path).dispatch().status()
}

fn body(client: &Client, path: &str) -> String {
    client
        .get(path)
        .dispatch()
        .into_string()
        .unwrap_or_default()
}

#[test]
fn an_author_cannot_open_the_settings_screens_and_an_administrator_can() {
    let _guard = reset();
    let author = signed_in_with_role("author");
    assert_eq!(status(&author, "/admin/settings"), Status::Forbidden);
    assert_eq!(status(&author, "/admin/settings/1/edit"), Status::Forbidden);
    assert_eq!(status(&author, "/admin/reading"), Status::Forbidden);
    assert_eq!(status(&author, "/admin/users"), Status::Forbidden);
    assert_eq!(status(&author, "/admin/roles"), Status::Forbidden);
    assert_eq!(status(&author, "/admin/widgets"), Status::Ok);
    assert_eq!(status(&author, "/admin/widgets/new"), Status::Forbidden);
    let nav = body(&author, "/admin");
    assert!(!nav.contains("href=\"/admin/settings\""));
    assert!(!nav.contains("href=\"/admin/reading\""));
    assert!(!nav.contains("href=\"/admin/users\""));
    assert!(nav.contains("href=\"/admin/widgets\""));
    assert!(!nav.contains("href=\"/admin/widgets/new\""));

    let administrator = signed_in_with_role("administrator");
    assert_eq!(status(&administrator, "/admin/settings"), Status::Ok);
    assert_eq!(status(&administrator, "/admin/settings/1/edit"), Status::Ok);
    assert_eq!(status(&administrator, "/admin/reading"), Status::Ok);
    assert_eq!(status(&administrator, "/admin/widgets/new"), Status::Ok);
    let nav = body(&administrator, "/admin");
    assert!(nav.contains("href=\"/admin/settings\""));
    assert!(nav.contains("href=\"/admin/reading\""));
}

#[test]
fn a_change_on_the_roles_screen_applies_from_the_next_request() {
    let _guard = reset();
    let author = signed_in_with_role("author");
    assert_eq!(status(&author, "/admin/settings"), Status::Forbidden);

    // The super-admin gives the author role the settings abilities on the Roles screen.
    let admin = signed_in_with(|ignite| ignite.user_management());
    let id = test_db().count("SELECT id FROM roles WHERE name = 'author'", &[]);
    let response = admin
        .post(format!("/admin/roles/{id}"))
        .header(ContentType::Form)
        .body(with_token(
            &admin,
            "name=author&abilities[]=widgets.viewAny&abilities[]=settings.viewAny&abilities[]=settings.manage",
        ))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    assert_eq!(status(&author, "/admin/settings"), Status::Ok);
    assert_eq!(status(&author, "/admin/reading"), Status::Ok);
    assert_eq!(status(&author, "/admin/settings/1/edit"), Status::Forbidden);
}

#[test]
fn the_super_admin_passes_every_check_as_before() {
    let _guard = reset();
    let client = client();
    client.get("/admin/login").dispatch();
    client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email={}&password={PASSWORD}", EMAIL.replace('@', "%40")),
        ))
        .dispatch();
    for path in [
        "/admin/settings",
        "/admin/reading",
        "/admin/users",
        "/admin/roles",
        "/admin/widgets/new",
    ] {
        assert_eq!(status(&client, path), Status::Ok, "{path}");
    }
}
