//! End-to-end tests for the field types of `#[derive(IgniteResource)]`: decimals,
//! date-times, static and loaded selects, and a relation, served by the panel against
//! the `ignite_test` database.

mod common;

use chrono::NaiveDate;
use common::{reset, test_db, with_token, EMAIL, PASSWORD};
use ignite_core::{Database, IgniteResource};
use ignite_panel::Ignite;
use orbit_orm::{Direction, Model, OrbitModel};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde::Serialize;

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "categories")]
#[ignite(table = categories, title = "Categories")]
pub struct Category {
    id: i32,
    #[orbit(fillable)]
    #[ignite(searchable)]
    name: String,
}

/// The tags a product may take: the names of the categories, loaded when the form shows.
async fn tag_choices(db: &Database) -> Result<Vec<(String, String)>, orbit_orm::Error> {
    let names: Vec<(String,)> = Category::query()
        .select(&["name"])
        .order_by("name", Direction::Asc)
        .get_as(db)
        .await?;
    Ok(names
        .into_iter()
        .map(|(name,)| (name.clone(), name))
        .collect())
}

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "products")]
#[ignite(table = products, title = "Products")]
pub struct Product {
    id: i32,
    #[orbit(fillable)]
    #[ignite(searchable, sortable)]
    name: String,
    #[orbit(fillable)]
    #[ignite(belongs_to = Category, display = name, label = "Category")]
    category_id: Option<i32>,
    #[orbit(fillable)]
    #[ignite(options = [("draft", "Draft"), ("published", "Published")])]
    status: String,
    #[orbit(fillable)]
    #[ignite(options = tag_choices)]
    tag: Option<String>,
    #[orbit(fillable)]
    #[ignite(step = "0.01", sortable)]
    price: f64,
    #[orbit(fillable)]
    stock: i16,
    #[orbit(fillable)]
    launch: Option<chrono::NaiveDateTime>,
}

/// Resets the shared database, creates the two tables, and seeds two categories.
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = reset();
    let conn = test_db();
    conn.exec("DROP TABLE IF EXISTS products");
    conn.exec("DROP TABLE IF EXISTS categories");
    conn.exec(
        "CREATE TABLE categories (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL)",
    );
    conn.exec(
        "CREATE TABLE products (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL, \
         category_id INT NULL, status VARCHAR(20) NOT NULL, tag VARCHAR(255) NULL, \
         price DOUBLE NOT NULL, stock SMALLINT NOT NULL, launch DATETIME NULL)",
    );
    conn.exec("INSERT INTO categories (name) VALUES ('Books'), ('Games')");
    guard
}

fn signed_in() -> Client {
    common::ensure_secret();
    let client = Client::tracked(
        rocket::build().attach(
            Ignite::new()
                .resource::<Category>()
                .resource::<Product>()
                .fairing(),
        ),
    )
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

fn product_count() -> i64 {
    test_db().count("SELECT COUNT(*) FROM products", &[])
}

/// A valid product form, with the given overrides applied to the defaults.
fn product_form(overrides: &[(&str, &str)]) -> String {
    let mut fields = vec![
        ("name", "Lantern"),
        ("category_id", "2"),
        ("status", "draft"),
        ("tag", "Books"),
        ("price", "9.5"),
        ("stock", "3"),
        ("launch", "2026-11-10T14:30"),
    ];
    for (key, value) in overrides {
        fields.retain(|(k, _)| k != key);
        fields.push((key, value));
    }
    fields
        .iter()
        .map(|(k, v)| format!("{k}={}", v.replace(' ', "+")))
        .collect::<Vec<_>>()
        .join("&")
}

#[test]
fn a_relation_select_lists_the_related_rows_by_their_display_column() {
    let _guard = setup();
    let client = signed_in();

    let form = client
        .get("/admin/products/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(form.contains(r#"<option value="1" >Books</option>"#));
    assert!(form.contains(r#"<option value="2" >Games</option>"#));
    assert!(form.contains(r#"<option value="" selected>Select an option</option>"#));
}

#[test]
fn a_relation_stores_the_id_and_the_list_shows_the_name() {
    let _guard = setup();
    let client = signed_in();

    let created = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[])))
        .dispatch();
    assert_eq!(created.status(), Status::SeeOther);
    assert_eq!(product_count(), 1);

    let list = client
        .get("/admin/products")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        list.contains("Category"),
        "the column takes its label from the attribute"
    );
    assert!(list.contains("Games"), "the list shows the related name");
    assert!(!list.contains(">2<"), "the list does not show the bare id");

    let edit = client
        .get("/admin/products/1/edit")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(edit.contains(r#"<option value="2" selected>Games</option>"#));
}

#[test]
fn a_relation_refuses_an_id_with_no_row_and_a_value_that_is_not_a_number() {
    let _guard = setup();
    let client = signed_in();

    let response = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[("category_id", "99")])))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Choose one of the options."));

    let response = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &product_form(&[("category_id", "games")]),
        ))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Category must be a whole number."));
    assert_eq!(product_count(), 0);
}

#[test]
fn a_static_select_accepts_only_its_choices() {
    let _guard = setup();
    let client = signed_in();

    let form = client
        .get("/admin/products/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(form.contains(r#"<option value="published" >Published</option>"#));

    let response = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            &product_form(&[("status", "archived")]),
        ))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Choose one of the options."));
    assert_eq!(product_count(), 0);
}

#[test]
fn a_select_loaded_at_request_time_accepts_only_the_loaded_choices() {
    let _guard = setup();
    let client = signed_in();

    let form = client
        .get("/admin/products/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(form.contains(r#"<option value="Games" >Games</option>"#));

    let response = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[("tag", "Puzzles")])))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Choose one of the options."));

    // A category added after the app started is a valid choice at once.
    test_db().exec("INSERT INTO categories (name) VALUES ('Puzzles')");
    let created = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[("tag", "Puzzles")])))
        .dispatch();
    assert_eq!(created.status(), Status::SeeOther);
    assert_eq!(product_count(), 1);
}

#[test]
fn a_decimal_refuses_text_and_steps_by_the_attribute() {
    let _guard = setup();
    let client = signed_in();

    let form = client
        .get("/admin/products/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(form.contains(r#"step="0.01""#));

    let response = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[("price", "cheap")])))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Price must be a number."));
}

#[test]
fn a_decimal_is_stored_as_a_number() {
    let _guard = setup();
    let client = signed_in();

    let created = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[("price", "12.25")])))
        .dispatch();
    assert_eq!(created.status(), Status::SeeOther);

    let conn = test_db();
    let price = conn
        .run(Product::query().find(&conn.db, 1))
        .unwrap()
        .expect("the product")
        .price;
    assert_eq!(price, 12.25);
}

#[test]
fn a_date_time_refuses_what_is_not_one_and_shows_readably() {
    let _guard = setup();
    let client = signed_in();

    let form = client
        .get("/admin/products/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(form.contains(r#"type="datetime-local""#));

    let response = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[("launch", "soon")])))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Launch must be a date and time, as YYYY-MM-DD HH:MM."));

    let created = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[])))
        .dispatch();
    assert_eq!(created.status(), Status::SeeOther);

    let list = client
        .get("/admin/products")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(list.contains("10 Nov 2026, 14:30"));

    let conn = test_db();
    let launch = conn
        .run(Product::query().find(&conn.db, 1))
        .unwrap()
        .expect("the product")
        .launch;
    assert_eq!(
        launch,
        NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(14, 30, 0)
    );
}

#[test]
fn a_small_integer_refuses_a_value_out_of_its_range() {
    let _guard = setup();
    let client = signed_in();

    let response = client
        .post("/admin/products")
        .header(ContentType::Form)
        .body(with_token(&client, &product_form(&[("stock", "40000")])))
        .dispatch();
    assert_eq!(response.status(), Status::UnprocessableEntity);
    assert!(response
        .into_string()
        .unwrap()
        .contains("Stock must be a whole number."));
}
