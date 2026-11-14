//! Runtime resources: owned field and column definitions, the registry, and the validation
//! of a submitted form against the definitions. No database is needed: the data source of
//! these resources is never called.
//!
//! The registry is shared by the whole test binary and the tests run in parallel, so each
//! test uses slugs of its own.

use std::collections::HashMap;

use ignite_core::{
    register_runtime, reserve_slugs, runtime_resource, runtime_resources, unregister_runtime,
    Column, ColumnDef, Database, Field, FieldDef, FieldKind, FieldKindDef, FormInput, ListQuery,
    Page, Row, RuntimeData, RuntimeError, RuntimeFuture, RuntimeResource,
};
use serde_json::{json, Value};

/// A data source that is never called: these tests only register and validate.
struct NoData;

impl RuntimeData for NoData {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        unreachable!()
    }
    fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
        unreachable!()
    }
    fn create<'a>(&'a self, _: &'a Database, _: Row) -> RuntimeFuture<'a, Row> {
        unreachable!()
    }
    fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
        unreachable!()
    }
    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        unreachable!()
    }
}

fn text(name: &str, label: &str, required: bool) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        label: label.to_string(),
        kind: FieldKindDef::Text,
        required,
    }
}

fn resource(slug: &str) -> RuntimeResource {
    RuntimeResource::new(slug, "Books", "Book", NoData)
        .field(text("title", "Title", true))
        .column(ColumnDef {
            key: "title".to_string(),
            label: "Title".to_string(),
            sortable: true,
            searchable: true,
        })
}

fn form(pairs: &[(&str, &str)]) -> FormInput {
    FormInput::new(
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<HashMap<_, _>>(),
    )
}

#[test]
fn a_static_field_converts_to_an_owned_definition() {
    const STATUS: Field = Field {
        name: "status",
        label: "Status",
        kind: FieldKind::Select(&[("draft", "Draft"), ("live", "Live")]),
        required: true,
    };
    let def = FieldDef::from(STATUS);
    assert_eq!(def.name, "status");
    assert_eq!(def.label, "Status");
    assert!(def.required);
    assert_eq!(
        def.kind,
        FieldKindDef::Select(vec![
            ("draft".to_string(), "Draft".to_string()),
            ("live".to_string(), "Live".to_string()),
        ])
    );
    assert_eq!(
        FieldKindDef::from(FieldKind::Decimal("0.01")),
        FieldKindDef::Decimal("0.01".to_string())
    );
    for (kind, def) in [
        (FieldKind::Text, FieldKindDef::Text),
        (FieldKind::Textarea, FieldKindDef::Textarea),
        (FieldKind::Password, FieldKindDef::Password),
        (FieldKind::Number, FieldKindDef::Number),
        (FieldKind::Checkbox, FieldKindDef::Checkbox),
        (FieldKind::Lookup, FieldKindDef::Lookup),
        (FieldKind::Date, FieldKindDef::Date),
        (FieldKind::DateTime, FieldKindDef::DateTime),
    ] {
        assert_eq!(FieldKindDef::from(kind), def);
    }
}

#[test]
fn a_static_column_converts_to_an_owned_definition() {
    let column = Column {
        key: "name",
        label: "Name",
        sortable: true,
        searchable: false,
    };
    assert_eq!(
        ColumnDef::from(column),
        ColumnDef {
            key: "name".to_string(),
            label: "Name".to_string(),
            sortable: true,
            searchable: false,
        }
    );
}

#[test]
fn a_registered_resource_is_listed_until_it_is_unregistered() {
    register_runtime(resource("rt-listed")).expect("register");

    let found = runtime_resource("rt-listed").expect("registered");
    assert_eq!(found.slug(), "rt-listed");
    assert_eq!(found.title(), "Books");
    assert_eq!(found.singular(), "Book");
    assert_eq!(found.fields()[0].name, "title");
    assert_eq!(found.fields()[0].kind, FieldKindDef::Text);
    assert_eq!(found.columns()[0].key, "title");
    assert!(runtime_resources().iter().any(|r| r.slug() == "rt-listed"));

    unregister_runtime("rt-listed").expect("unregister");
    assert!(runtime_resource("rt-listed").is_none());
    assert!(!runtime_resources().iter().any(|r| r.slug() == "rt-listed"));
}

#[test]
fn unregistering_an_unknown_slug_is_an_error() {
    let error = unregister_runtime("rt-never").unwrap_err();
    assert_eq!(error, RuntimeError::NotRegistered("rt-never".to_string()));
}

#[test]
fn a_second_resource_with_the_same_slug_is_refused() {
    register_runtime(resource("rt-twice")).expect("register");

    let error = register_runtime(resource("rt-twice")).unwrap_err();
    assert_eq!(error, RuntimeError::SlugTaken("rt-twice".to_string()));
    assert_eq!(
        error.to_string(),
        "the slug `rt-twice` is already used by another Ignite resource"
    );
    unregister_runtime("rt-twice").expect("unregister");
}

#[test]
fn a_reserved_slug_is_refused_and_a_registered_slug_cannot_be_reserved() {
    reserve_slugs(&["rt-static"]).expect("reserve");
    assert_eq!(
        register_runtime(resource("rt-static")).unwrap_err(),
        RuntimeError::SlugTaken("rt-static".to_string())
    );

    register_runtime(resource("rt-first")).expect("register");
    assert_eq!(
        reserve_slugs(&["rt-first"]).unwrap_err(),
        RuntimeError::SlugTaken("rt-first".to_string())
    );
    unregister_runtime("rt-first").expect("unregister");
    // Reserving the same slug twice is fine: a panel may be built more than once.
    reserve_slugs(&["rt-static"]).expect("reserve again");
}

#[test]
fn a_slug_that_is_not_one_url_segment_is_refused() {
    for slug in ["", "two/parts", "with space", "new?x", "Caps"] {
        assert_eq!(
            register_runtime(resource(slug)).unwrap_err(),
            RuntimeError::InvalidSlug(slug.to_string()),
            "{slug:?}"
        );
    }
}

#[test]
fn two_fields_with_the_same_name_are_refused() {
    let twice = resource("rt-dup-field").field(text("title", "Again", false));
    assert_eq!(
        register_runtime(twice).unwrap_err(),
        RuntimeError::DuplicateField {
            slug: "rt-dup-field".to_string(),
            field: "title".to_string(),
        }
    );
}

/// A resource with one field of each kind that validation reads, registered at `slug`.
fn every_kind(slug: &str) -> std::sync::Arc<ignite_core::RegisteredResource> {
    register_runtime(every_kind_definition(slug)).expect("register");
    runtime_resource(slug).expect("registered")
}

fn every_kind_definition(slug: &str) -> RuntimeResource {
    let def = |name: &str, label: &str, kind: FieldKindDef, required: bool| FieldDef {
        name: name.to_string(),
        label: label.to_string(),
        kind,
        required,
    };
    RuntimeResource::new(slug, "Events", "Event", NoData)
        .field(def("title", "Title", FieldKindDef::Text, true))
        .field(def("body", "Body", FieldKindDef::Textarea, false))
        .field(def("seats", "Seats", FieldKindDef::Number, true))
        .field(def(
            "price",
            "Price",
            FieldKindDef::Decimal("0.01".into()),
            false,
        ))
        .field(def("public", "Public", FieldKindDef::Checkbox, false))
        .field(def(
            "status",
            "Status",
            FieldKindDef::Select(vec![
                ("draft".into(), "Draft".into()),
                ("live".into(), "Live".into()),
            ]),
            true,
        ))
        .field(def("day", "Day", FieldKindDef::Date, false))
        .field(def("starts", "Starts", FieldKindDef::DateTime, false))
        .field(def("secret", "Secret", FieldKindDef::Password, true))
}

#[test]
fn a_valid_form_becomes_a_row_of_typed_values() {
    let row = every_kind("rt-kinds-valid")
        .validate(&form(&[
            ("title", "  Launch  "),
            ("seats", "40"),
            ("price", "9.50"),
            ("public", "on"),
            ("status", "live"),
            ("day", "2026-11-14"),
            ("starts", "2026-11-14T18:30"),
            ("secret", "hunter2"),
        ]))
        .expect("valid");

    assert_eq!(row["title"], json!("Launch"));
    assert_eq!(row["body"], Value::Null, "a blank optional field is null");
    assert_eq!(row["seats"], json!(40));
    assert_eq!(row["price"], json!(9.5));
    assert_eq!(row["public"], json!(true));
    assert_eq!(row["status"], json!("live"));
    assert_eq!(row["day"], json!("2026-11-14"));
    assert_eq!(row["starts"], json!("2026-11-14 18:30:00"));
    let hash = row["secret"].as_str().expect("a hash");
    assert!(ignite_core::verify_password(hash, "hunter2"));
}

#[test]
fn an_invalid_form_returns_the_messages_of_the_static_checks() {
    let errors = every_kind("rt-kinds-invalid")
        .validate(&form(&[
            ("seats", "many"),
            ("price", "cheap"),
            ("status", "archived"),
            ("day", "14/11/2026"),
            ("starts", "soon"),
        ]))
        .unwrap_err();

    let message = |name: &str| errors.get(name).map(String::as_str);
    assert_eq!(message("title"), Some("Title is required."));
    assert_eq!(message("seats"), Some("Seats must be a whole number."));
    assert_eq!(message("price"), Some("Price must be a number."));
    assert_eq!(message("status"), Some("Choose one of the options."));
    assert_eq!(message("day"), Some("Day must be a date, as YYYY-MM-DD."));
    assert_eq!(
        message("starts"),
        Some("Starts must be a date and time, as YYYY-MM-DD HH:MM.")
    );
    assert_eq!(message("secret"), Some("Secret is required."));
    assert_eq!(message("public"), None, "a checkbox is never blank");
}

#[test]
fn a_blank_password_on_an_edit_keeps_the_stored_one() {
    let row = every_kind("rt-kinds-edit")
        .validate_update(&form(&[
            ("title", "Launch"),
            ("seats", "40"),
            ("status", "draft"),
        ]))
        .expect("valid without a password");
    assert!(!row.contains_key("secret"), "the stored password is kept");
    assert_eq!(row["public"], json!(false));
}
