//! Integration tests for the resource routes, against the `ignite_test` database.

mod common;

use common::*;
use rocket::http::{ContentType, Status};

#[test]
fn list_shows_the_first_row_text() {
    let _guard = reset();
    let client = signed_in(None);
    let body = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();

    // The first row of a text query used to come back empty. See the Diesel fork pinned in Cargo.toml.
    assert!(body.contains("Apple"), "first row missing: {body}");
    assert!(body.contains("Banana"));
    assert!(!body.contains("Cherry"), "page should hold two rows");
}

#[test]
fn list_is_a_page_with_a_pager() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client.get("/admin/widgets?page=2").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().unwrap();

    assert!(body.contains("Cherry"));
    assert!(!body.contains("Apple"));
    assert!(body.contains("Page 2 of 2"));
    assert!(body.contains("3 in total"));
}

#[test]
fn search_matches_names_and_treats_wildcards_literally() {
    let _guard = reset();
    let client = signed_in(None);

    let body = client
        .get("/admin/widgets?q=an")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains("Banana"));
    assert!(!body.contains("Apple"));

    let body = client
        .get("/admin/widgets?q=%25")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        body.contains("No records found."),
        "a literal % should match nothing"
    );
}

#[test]
fn sort_by_a_sortable_column_orders_the_page() {
    let _guard = reset();
    let client = signed_in(None);
    let body = client
        .get("/admin/widgets?sort=price&dir=desc")
        .dispatch()
        .into_string()
        .unwrap();

    assert!(body.contains("Apple"));
    assert!(body.contains("Cherry"));
    assert!(
        !body.contains("Banana"),
        "Banana is cheapest, so it is on page 2"
    );
    assert!(body.find("Apple").unwrap() < body.find("Cherry").unwrap());
}

#[test]
fn unknown_sort_keys_are_ignored() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .get("/admin/widgets?sort=password_hash;drop")
        .dispatch();

    assert_eq!(response.status(), Status::Ok);
    assert!(response.into_string().unwrap().contains("Apple"));
}

#[test]
fn new_form_renders_empty_fields() {
    let _guard = reset();
    let client = signed_in(None);
    let body = client
        .get("/admin/widgets/new")
        .dispatch()
        .into_string()
        .unwrap();

    assert!(body.contains(r#"name="name""#));
    assert!(body.contains(r#"action="/admin/widgets""#));
}

#[test]
fn create_saves_a_valid_form_and_redirects() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(&client, "name=Date&price=5&note=Sweet"))
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(
        response.headers().get_one("Location"),
        Some("/admin/widgets")
    );
    assert_eq!(count_widgets(), 4);
}

#[test]
fn create_rerenders_the_form_with_errors() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(&client, "name=&price=abc"))
        .dispatch();

    assert_eq!(response.status(), Status::UnprocessableEntity);
    let body = response.into_string().unwrap();
    assert!(body.contains("Name is required."));
    assert!(body.contains("Price must be a whole number."));
    assert_eq!(count_widgets(), 3);
}

#[test]
fn edit_form_fills_in_the_stored_values() {
    let _guard = reset();
    let client = signed_in(None);
    let body = client
        .get("/admin/widgets/1/edit")
        .dispatch()
        .into_string()
        .unwrap();

    assert!(body.contains(r#"value="Apple""#));
    assert!(body.contains(r#"value="3""#));
    assert!(body.contains(r#"action="/admin/widgets/1""#));
}

#[test]
fn update_changes_the_row() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/widgets/1")
        .header(ContentType::Form)
        .body(with_token(&client, "name=Apricot&price=4"))
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    let body = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(body.contains("Apricot"));
    assert!(!body.contains("Apple"));
}

#[test]
fn delete_removes_the_row() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/widgets/2/delete")
        .header(ContentType::Form)
        .body(with_token(&client, ""))
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(count_widgets(), 2);
}

#[test]
fn missing_rows_and_resources_are_404() {
    let _guard = reset();
    let client = signed_in(None);

    assert_eq!(
        client.get("/admin/widgets/99/edit").dispatch().status(),
        Status::NotFound
    );
    assert_eq!(
        client
            .post("/admin/widgets/99")
            .header(ContentType::Form)
            .body(with_token(&client, "name=x&price=1"))
            .dispatch()
            .status(),
        Status::NotFound
    );
    assert_eq!(
        client
            .post("/admin/widgets/99/delete")
            .header(ContentType::Form)
            .body(with_token(&client, ""))
            .dispatch()
            .status(),
        Status::NotFound
    );
    assert_eq!(
        client.get("/admin/nope").dispatch().status(),
        Status::NotFound
    );
    assert_eq!(
        client.get("/admin/widgets/abc/edit").dispatch().status(),
        Status::NotFound
    );
}

#[test]
fn the_mount_path_is_configurable() {
    let _guard = reset();
    let client = signed_in(Some("/panel/"));

    assert_eq!(client.get("/panel/widgets").dispatch().status(), Status::Ok);
    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::NotFound
    );
}

#[test]
fn saving_an_unchanged_row_succeeds() {
    let _guard = reset();
    let client = signed_in(None);
    // MySQL reports zero changed rows for an unchanged save; the row still exists.
    let response = client
        .post("/admin/widgets/1")
        .header(ContentType::Form)
        .body(with_token(&client, "name=Apple&price=3"))
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
}
