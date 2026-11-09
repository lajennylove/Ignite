//! End-to-end tests for `#[derive(IgniteResource)]`: a derived model served by
//! the panel, against the `ignite_test` database.

mod common;

use chrono::NaiveDate;
use common::{reset, EMAIL, PASSWORD};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::sql_query;
use ignite_core::IgniteResource;
use ignite_panel::Ignite;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde::Serialize;

mod schema {
    diesel::table! {
        gadgets (id) {
            id -> Integer,
            name -> Text,
            price -> Integer,
            note -> Nullable<Text>,
            active -> Bool,
            due -> Nullable<Date>,
            secret -> Text,
        }
    }
}

#[derive(diesel::Queryable, diesel::Selectable, Serialize, IgniteResource)]
#[diesel(table_name = schema::gadgets)]
#[ignite(table = schema::gadgets, title = "Gadgets")]
pub struct Gadget {
    id: i32,
    #[ignite(searchable, sortable)]
    name: String,
    #[ignite(sortable, label = "Price (cents)")]
    price: i32,
    #[ignite(form_type = "textarea")]
    note: Option<String>,
    active: bool,
    due: Option<NaiveDate>,
    #[ignite(form_type = "password")]
    secret: String,
}

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("set DATABASE_URL to the test database, such as mysql://user@localhost/ignite_test")
}

/// Resets the shared database, creates the `gadgets` table, and seeds two rows.
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    let mut conn = MysqlConnection::establish(&database_url()).unwrap();
    sql_query("DROP TABLE IF EXISTS gadgets")
        .execute(&mut conn)
        .unwrap();
    sql_query(
        "CREATE TABLE gadgets (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL, price INT NOT NULL, \
         note TEXT NULL, active TINYINT(1) NOT NULL, due DATE NULL, secret VARCHAR(255) NOT NULL, \
         UNIQUE KEY gadgets_name_unique (name))",
    )
    .execute(&mut conn)
    .unwrap();
    let seed = ignite_core::hash_password("seed-password").unwrap();
    diesel::insert_into(schema::gadgets::table)
        .values(vec![
            (
                schema::gadgets::name.eq("Anchor"),
                schema::gadgets::price.eq(5),
                schema::gadgets::note.eq(None::<String>),
                schema::gadgets::active.eq(true),
                schema::gadgets::due.eq(None::<NaiveDate>),
                schema::gadgets::secret.eq(seed.clone()),
            ),
            (
                schema::gadgets::name.eq("Beacon"),
                schema::gadgets::price.eq(9),
                schema::gadgets::note.eq(Some("Bright".to_string())),
                schema::gadgets::active.eq(false),
                schema::gadgets::due.eq(NaiveDate::from_ymd_opt(2026, 3, 1)),
                schema::gadgets::secret.eq(seed),
            ),
        ])
        .execute(&mut conn)
        .unwrap();
    guard
}

fn signed_in() -> Client {
    let client =
        Client::tracked(rocket::build().attach(Ignite::new().resource::<Gadget>().fairing()))
            .unwrap();
    let status = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(format!("email={EMAIL}&password={PASSWORD}"))
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther);
    client
}

fn stored_secret(id: i32) -> String {
    let mut conn = MysqlConnection::establish(&database_url()).unwrap();
    schema::gadgets::table
        .find(id)
        .select(schema::gadgets::secret)
        .first(&mut conn)
        .unwrap()
}

fn gadget_count() -> i64 {
    let mut conn = MysqlConnection::establish(&database_url()).unwrap();
    schema::gadgets::table
        .count()
        .get_result(&mut conn)
        .unwrap()
}

#[test]
fn the_derived_list_searches_sorts_and_pages() {
    let _guard = setup();
    let client = signed_in();

    let body = client
        .get("/admin/gadgets?q=ea")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains("Beacon"));
    assert!(!body.contains("Anchor"));

    let body = client
        .get("/admin/gadgets?sort=price&dir=desc")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.find("Beacon").unwrap() < body.find("Anchor").unwrap());
    assert!(
        body.contains("Price (cents)"),
        "the label comes from the attribute"
    );
}

#[test]
fn the_derived_form_validates_each_field_type() {
    let _guard = setup();
    let client = signed_in();
    let response = client
        .post("/admin/gadgets")
        .header(ContentType::Form)
        .body("name=&price=many&due=2026-13-40")
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body = response.into_string().unwrap();
    assert!(body.contains("Name is required."));
    assert!(body.contains("Price (cents) must be a whole number."));
    assert!(body.contains("Due must be a date, as YYYY-MM-DD."));
    assert!(body.contains("Secret is required."));
    assert_eq!(gadget_count(), 2);
}

#[test]
fn the_derived_resource_creates_updates_and_deletes() {
    let _guard = setup();
    let client = signed_in();

    let created = client
        .post("/admin/gadgets")
        .header(ContentType::Form)
        .body("name=Compass&price=7&note=North&active=on&due=2026-06-30&secret=compass-pass")
        .dispatch();
    assert_eq!(created.status(), Status::SeeOther);
    assert_eq!(gadget_count(), 3);

    let edit = client
        .get("/admin/gadgets/3/edit")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(edit.contains(r#"value="Compass""#));
    assert!(
        edit.contains(r#"checked"#),
        "the checkbox reflects the stored value"
    );
    assert!(edit.contains(r#"value="2026-06-30""#));

    let updated = client
        .post("/admin/gadgets/3")
        .header(ContentType::Form)
        .body("name=Compass&price=8&note=North")
        .dispatch();
    assert_eq!(updated.status(), Status::SeeOther);

    let deleted = client.post("/admin/gadgets/3/delete").dispatch();
    assert_eq!(deleted.status(), Status::SeeOther);
    assert_eq!(gadget_count(), 2);
}

#[test]
fn the_derived_model_reads_its_rows() {
    let _guard = setup();
    let mut conn = MysqlConnection::establish(&database_url()).unwrap();
    let page = <Gadget as IgniteResource>::list(
        &mut conn,
        &ignite_core::ListQuery {
            page: 1,
            per_page: 20,
            search: None,
            sort: None,
        },
    )
    .unwrap();

    assert_eq!(page.total, 2);
    assert_eq!(
        page.rows[0].name, "Anchor",
        "the first row's text is read in full"
    );
    assert_eq!(page.rows[1].due, NaiveDate::from_ymd_opt(2026, 3, 1));
}

#[test]
fn a_password_is_stored_as_an_argon2_hash_not_as_submitted() {
    let _guard = setup();
    let client = signed_in();
    let response = client
        .post("/admin/gadgets")
        .header(ContentType::Form)
        .body("name=Compass&price=7&secret=hunter2-hunter2")
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    let stored = stored_secret(3);
    assert!(
        stored.starts_with("$argon2id$"),
        "unexpected stored value: {stored}"
    );
    assert_ne!(stored, "hunter2-hunter2");
    assert!(ignite_core::verify_password(&stored, "hunter2-hunter2"));
}

#[test]
fn a_blank_password_on_edit_keeps_the_stored_hash() {
    let _guard = setup();
    let client = signed_in();
    let before = stored_secret(1);

    let response = client
        .post("/admin/gadgets/1")
        .header(ContentType::Form)
        .body("name=Anchor&price=6&secret=")
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(stored_secret(1), before);
    assert!(ignite_core::verify_password(&before, "seed-password"));
}

#[test]
fn a_new_password_on_edit_replaces_the_hash() {
    let _guard = setup();
    let client = signed_in();

    let response = client
        .post("/admin/gadgets/1")
        .header(ContentType::Form)
        .body("name=Anchor&price=5&secret=a-new-password")
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    let stored = stored_secret(1);
    assert!(ignite_core::verify_password(&stored, "a-new-password"));
    assert!(!ignite_core::verify_password(&stored, "seed-password"));
}

#[test]
fn the_hash_never_reaches_the_list_or_the_edit_form() {
    let _guard = setup();
    let client = signed_in();
    let stored = stored_secret(1);

    let list = client
        .get("/admin/gadgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(!list.contains("$argon2"));
    assert!(!list.contains(&stored));

    let edit = client
        .get("/admin/gadgets/1/edit")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(!edit.contains("$argon2"));
    assert!(!edit.contains(&stored));
    assert!(edit.contains(r#"type="password""#));
}

#[test]
fn a_duplicate_name_on_create_is_a_field_error_not_a_server_error() {
    let _guard = setup();
    let client = signed_in();
    let response = client
        .post("/admin/gadgets")
        .header(ContentType::Form)
        .body("name=Anchor&price=1&secret=another-pass")
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body = response.into_string().unwrap();
    assert!(body.contains("Name is already taken."), "body: {body}");
    assert_eq!(gadget_count(), 2);
}

#[test]
fn a_duplicate_name_on_edit_is_a_field_error_not_a_server_error() {
    let _guard = setup();
    let client = signed_in();
    let response = client
        .post("/admin/gadgets/2")
        .header(ContentType::Form)
        .body("name=Anchor&price=9&secret=")
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Name is already taken."));
}
