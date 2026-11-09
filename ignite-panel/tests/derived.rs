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
         note TEXT NULL, active TINYINT(1) NOT NULL, due DATE NULL)",
    )
    .execute(&mut conn)
    .unwrap();
    sql_query(
        "INSERT INTO gadgets (name, price, note, active, due) VALUES \
         ('Anchor', 5, NULL, 1, NULL), ('Beacon', 9, 'Bright', 0, '2026-03-01')",
    )
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
    assert_eq!(gadget_count(), 2);
}

#[test]
fn the_derived_resource_creates_updates_and_deletes() {
    let _guard = setup();
    let client = signed_in();

    let created = client
        .post("/admin/gadgets")
        .header(ContentType::Form)
        .body("name=Compass&price=7&note=North&active=on&due=2026-06-30")
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
