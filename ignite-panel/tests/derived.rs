//! End-to-end tests for `#[derive(IgniteResource)]`: a derived model served by
//! the panel, against the `ignite_test` database.

mod common;

use chrono::NaiveDate;
use common::{reset, test_db, with_token, EMAIL, PASSWORD};
use ignite_core::IgniteResource;
use ignite_panel::Ignite;
use orbit_orm::{Model, OrbitModel, Value};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde::Serialize;

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "gadgets")]
#[ignite(table = gadgets, title = "Gadgets")]
#[ignite(latest_records(limit = 1, columns = "name,price"))]
pub struct Gadget {
    id: i32,
    #[orbit(fillable)]
    #[ignite(searchable, sortable)]
    name: String,
    #[orbit(fillable)]
    #[ignite(sortable, label = "Price (cents)")]
    price: i32,
    #[orbit(fillable)]
    #[ignite(form_type = "textarea")]
    note: Option<String>,
    #[orbit(fillable)]
    active: bool,
    #[orbit(fillable)]
    due: Option<NaiveDate>,
    #[orbit(fillable)]
    #[ignite(form_type = "password")]
    secret: String,
}

/// Resets the shared database, creates the `gadgets` table, and seeds two rows.
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    let conn = test_db();
    conn.exec("DROP TABLE IF EXISTS gadgets");
    conn.exec(
        "CREATE TABLE gadgets (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL, price INT NOT NULL, \
         note TEXT NULL, active TINYINT(1) NOT NULL, due DATE NULL, secret VARCHAR(255) NOT NULL, \
         UNIQUE KEY gadgets_name_unique (name))",
    );
    let seed = ignite_core::hash_password("seed-password").unwrap();
    conn.exec_with(
        "INSERT INTO gadgets (name, price, note, active, due, secret) VALUES (?, ?, ?, ?, ?, ?)",
        &[
            "Anchor".into(),
            5.into(),
            Value::Null,
            true.into(),
            Value::Null,
            seed.as_str().into(),
        ],
    );
    conn.exec_with(
        "INSERT INTO gadgets (name, price, note, active, due, secret) VALUES (?, ?, ?, ?, ?, ?)",
        &[
            "Beacon".into(),
            9.into(),
            "Bright".into(),
            false.into(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap().into(),
            seed.as_str().into(),
        ],
    );
    guard
}

fn signed_in() -> Client {
    common::ensure_secret();
    let client =
        Client::tracked(rocket::build().attach(Ignite::new().resource::<Gadget>().fairing()))
            .unwrap();
    client.get("/admin/login").dispatch();
    let status = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &format!("email={EMAIL}&password={PASSWORD}"),
        ))
        .dispatch()
        .status();
    assert_eq!(status, Status::SeeOther);
    client
}

fn stored_secret(id: i32) -> String {
    let conn = test_db();
    conn.run(Gadget::query().find(&conn.db, id))
        .unwrap()
        .expect("the gadget")
        .secret
}

fn gadget_count() -> i64 {
    test_db().count("SELECT COUNT(*) FROM gadgets", &[])
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
        .body(with_token(&client, "name=&price=many&due=2026-13-40"))
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
        .body(with_token(
            &client,
            "name=Compass&price=7&note=North&active=on&due=2026-06-30&secret=compass-pass",
        ))
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
        .body(with_token(&client, "name=Compass&price=8&note=North"))
        .dispatch();
    assert_eq!(updated.status(), Status::SeeOther);

    let deleted = client
        .post("/admin/gadgets/3/delete")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();
    assert_eq!(deleted.status(), Status::SeeOther);
    assert_eq!(gadget_count(), 2);
}

#[test]
fn the_derived_model_reads_its_rows() {
    let _guard = setup();
    let conn = test_db();
    let page = conn
        .run(<Gadget as IgniteResource>::list(
            &conn.db,
            &ignite_core::ListQuery {
                page: 1,
                per_page: 20,
                search: None,
                sort: None,
            },
        ))
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
        .body(with_token(
            &client,
            "name=Compass&price=7&secret=hunter2-hunter2",
        ))
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
        .body(with_token(&client, "name=Anchor&price=6&secret="))
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
        .body(with_token(
            &client,
            "name=Anchor&price=5&secret=a-new-password",
        ))
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
        .body(with_token(
            &client,
            "name=Anchor&price=1&secret=another-pass",
        ))
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
        .body(with_token(&client, "name=Anchor&price=9&secret="))
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Name is already taken."));
}

#[test]
fn headings_use_the_singular_name_and_dates_read_readably() {
    let _guard = setup();
    let client = signed_in();

    let new_form = client
        .get("/admin/gadgets/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(new_form.contains("New Gadget"), "singular heading expected");
    assert!(!new_form.contains("New Gadgets"));

    let list = client
        .get("/admin/gadgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(list.contains("New Gadget"));
    assert!(
        list.contains("1 Mar 2026"),
        "the due date shows readably in the list"
    );
    assert!(
        !list.contains("2026-03-01"),
        "the list does not show the ISO value"
    );

    let edit = client
        .get("/admin/gadgets/2/edit")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        edit.contains(r#"value="2026-03-01""#),
        "the form keeps the ISO value"
    );
    assert!(edit.contains("Edit Gadget #2"));
}

#[test]
fn the_derived_latest_returns_the_newest_rows_first() {
    let _guard = setup();
    let conn = test_db();
    let rows = conn
        .run(<Gadget as IgniteResource>::latest(&conn.db, 1))
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Beacon", "the highest id is the newest");
}

#[test]
fn a_derived_resource_declares_its_dashboard_table() {
    let declared = <Gadget as IgniteResource>::latest_records().expect("declared on the model");

    assert_eq!(
        declared.title, "Latest Gadgets",
        "the default title is Latest and the title"
    );
    assert_eq!(declared.columns, &["name", "price"]);
    assert_eq!(declared.limit, 1);
}

#[test]
fn the_derived_dashboard_shows_the_declared_table() {
    let _guard = setup();
    let client = signed_in();
    let body = client.get("/admin").dispatch().into_string().unwrap();

    assert!(body.contains("Latest Gadgets"));
    assert!(body.contains("Beacon"), "the newest row (highest id) shows");
    assert!(
        !body.contains("Anchor"),
        "only the declared limit of one row shows"
    );
}

fn stored_gadget(id: i32) -> Gadget {
    let conn = test_db();
    conn.run(Gadget::query().find(&conn.db, id))
        .unwrap()
        .expect("the gadget")
}

// Since 0.3.0 an `Option` field left blank on edit is written as NULL, as on create, so a
// user can clear it. Before, it was skipped and kept its value.
#[test]
fn an_optional_field_cleared_on_edit_is_stored_as_null() {
    let _guard = setup();
    let client = signed_in();
    let before = stored_gadget(2);
    assert_eq!(before.note.as_deref(), Some("Bright"));
    assert!(before.due.is_some());

    let response = client
        .post("/admin/gadgets/2")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            "name=Beacon&price=9&note=&due=&secret=",
        ))
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    let after = stored_gadget(2);
    assert_eq!(after.note, None, "a cleared textarea is NULL");
    assert_eq!(after.due, None, "a cleared date is NULL");
    assert_eq!(after.name, "Beacon");
    assert_eq!(
        after.secret, before.secret,
        "a password left blank on the same edit keeps its hash"
    );
    assert!(ignite_core::verify_password(&after.secret, "seed-password"));
}

#[test]
fn a_field_that_is_not_optional_is_still_required_on_edit() {
    let _guard = setup();
    let client = signed_in();

    let response = client
        .post("/admin/gadgets/2")
        .header(ContentType::Form)
        .body(with_token(&client, "name=&price=&note="))
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body = response.into_string().unwrap();
    assert!(body.contains("Name is required."));
    assert!(body.contains("Price (cents) is required."));
    let after = stored_gadget(2);
    assert_eq!(after.name, "Beacon", "nothing is written");
    assert_eq!(after.note.as_deref(), Some("Bright"), "nothing is written");
}
