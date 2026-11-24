//! Form layouts, as Filament's layout components: a form with a `FormLayout` renders its
//! fields in the grid's columns, each section a card, with the columns and spans it was given,
//! and a sidebar group stacks its sections. `Width::Full` lifts the 1536px cap of the page. A
//! form without a layout renders as before (the snapshots in `theme.rs` check its HTML). Runs
//! against the test database, for the signed-in user.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ColumnDef, Database, FieldDef, FieldKindDef, FormLayout,
    Group, ListQuery, Page, Row, RuntimeData, RuntimeFuture, RuntimeResource, Section, Width,
};
use orbit_orm::Error as OrbitError;

/// Rows that are never read: the tests only render the new-record form and the empty list.
struct NoRows;

impl RuntimeData for NoRows {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async {
            Ok(Page {
                rows: Vec::new(),
                total: 0,
            })
        })
    }

    fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async { Ok(None) })
    }

    fn create<'a>(&'a self, _: &'a Database, _: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Err(OrbitError::NotFound { table: "none" }) })
    }

    fn update<'a>(&'a self, _: &'a Database, _: i64, _: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Err(OrbitError::NotFound { table: "none" }) })
    }

    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }
}

fn field(name: &str) -> FieldDef {
    FieldDef {
        name: name.into(),
        label: name.into(),
        kind: FieldKindDef::Text,
        required: false,
    }
}

/// A resource at `slug` with the fields of a post, laid out by `layout`.
fn posts(slug: &str, layout: FormLayout) -> RuntimeResource {
    let mut resource = RuntimeResource::new(slug, "Posts", "Post", NoRows);
    for name in [
        "title", "slug", "body", "status", "category", "excerpt", "secret",
    ] {
        resource = resource.field(field(name));
    }
    resource
        .column(ColumnDef {
            key: "title".into(),
            label: "Title".into(),
            sortable: false,
            searchable: false,
        })
        .hidden("secret", |_| true)
        .form_layout(layout)
}

/// The new-record form of `resource`, for the signed-in test user.
fn new_form(resource: RuntimeResource) -> String {
    let slug = resource.slug.clone();
    let _ = unregister_runtime(&slug);
    register_runtime(resource).expect("register");
    let client = signed_in(None);
    let body = client
        .get(format!("/admin/{slug}/new"))
        .dispatch()
        .into_string()
        .unwrap();
    unregister_runtime(&slug).expect("unregister");
    body
}

/// The position of `needle` in `body`, which must contain it.
fn at(body: &str, needle: &str) -> usize {
    body.find(needle)
        .unwrap_or_else(|| panic!("missing {needle}:\n{body}"))
}

#[test]
fn a_form_with_a_layout_renders_its_columns_with_their_spans() {
    let _guard = reset();
    let layout = FormLayout::new().columns(3).schema([
        Group::make([Section::make("Content").schema(["title", "slug", "body"])]).column_span(2),
        Group::make([
            Section::make("Publish")
                .description("When and how")
                .schema(["status"]),
            Section::make("Category").schema(["category"]),
        ])
        .column_span(1),
    ]);
    let body = new_form(posts("layout-posts", layout));

    // The grid has three columns from the `lg` breakpoint; the main column takes two and the
    // sidebar one.
    assert!(
        body.contains(
            r#"<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-5 lg:grid-cols-3" data-ignite-fields>"#
        ),
        "{body}"
    );
    let main = at(
        &body,
        r#"<div class="grid min-w-0 content-start gap-5 lg:col-span-2">"#,
    );
    let sidebar = at(
        &body,
        r#"<div class="grid min-w-0 content-start gap-5 lg:col-span-1">"#,
    );
    assert!(main < sidebar);
    // The form is no longer one capped card: the sections are the cards.
    assert!(
        body.contains(r#"<form class="grid gap-5" method="post""#),
        "{body}"
    );
    assert!(!body.contains("max-w-2xl"));
    assert_eq!(
        body.matches(r#"<section class="ig-card grid min-w-0 gap-5">"#)
            .count(),
        4
    );

    // Each field is in its section, in order, and the sidebar stacks its two sections.
    let content = at(&body, "<h2 class=\"m-0\">Content</h2>");
    let publish = at(&body, "<h2 class=\"m-0\">Publish</h2>");
    let category = at(&body, "<h2 class=\"m-0\">Category</h2>");
    assert!(content < at(&body, r#"name="title""#));
    assert!(at(&body, r#"name="body""#) < sidebar);
    assert!(sidebar < publish && publish < at(&body, r#"name="status""#));
    assert!(body.contains("When and how"));
    assert!(publish < category && category < at(&body, r#"name="category""#));

    // A field the layout does not place still shows, after the others in the first column;
    // a hidden field does not.
    let excerpt = at(&body, r#"name="excerpt""#);
    assert!(at(&body, r#"name="body""#) < excerpt && excerpt < sidebar);
    assert!(!body.contains(r#"name="secret""#));

    // The page loads the layout's stylesheet, and keeps the panel's width.
    assert!(body.contains("/build/assets/form-"), "{body}");
    assert!(body.contains(r#"<main class="mx-auto w-full max-w-[1536px] flex-1 px-5 py-6">"#));
}

#[test]
fn a_full_width_form_renders_without_the_1536px_cap() {
    let _guard = reset();
    let layout = FormLayout::new()
        .columns(2)
        .max_content_width(Width::Full)
        .schema([
            Section::make("General").schema(["title", "slug"]),
            Section::make("Reading").schema(["status", "category"]),
            Section::make("").column_span_full().schema(["body"]),
        ]);
    let body = new_form(posts("settings-posts", layout));

    assert!(
        body.contains(r#"<main class="mx-auto w-full max-w-none flex-1 px-5 py-6">"#),
        "{body}"
    );
    assert!(!body.contains("max-w-[1536px]"));
    assert!(body.contains("lg:grid-cols-2"));
    // Two equal columns, and a full-width section without a heading under them.
    assert_eq!(
        body.matches(r#"<div class="grid min-w-0 content-start gap-5 lg:col-span-1">"#)
            .count(),
        2
    );
    let full = at(
        &body,
        r#"<div class="grid min-w-0 content-start gap-5 col-span-full">"#,
    );
    assert!(full < at(&body, r#"name="body""#));
    assert_eq!(body.matches("<h2 class=\"m-0\">").count(), 2);
}

#[test]
fn a_form_without_a_layout_renders_as_before() {
    let _guard = reset();
    let body = new_form(posts("plain-posts", FormLayout::new()));

    assert!(
        body.contains(r#"<form class="ig-card grid max-w-2xl gap-5" method="post""#),
        "{body}"
    );
    assert!(body.contains(r#"<div class="grid gap-5" data-ignite-fields>"#));
    assert!(body.contains(r#"<main class="mx-auto w-full max-w-[1536px] flex-1 px-5 py-6">"#));
    assert!(
        !body.contains("/build/assets/form-"),
        "no layout stylesheet"
    );
    assert!(!body.contains("<section"));
    assert!(!body.contains("lg:col-span"));
}

#[test]
fn other_pages_keep_the_panel_width() {
    let _guard = reset();
    let client = signed_in(None);
    let body = client.get("/admin").dispatch().into_string().unwrap();
    assert!(body.contains(r#"<main class="mx-auto w-full max-w-[1536px] flex-1 px-5 py-6">"#));
}
