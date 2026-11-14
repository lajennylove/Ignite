//! The panel's look: the built-in `ignite` theme renders the same HTML as before the theme
//! system, and another theme changes the look of the same screens. Runs against the
//! `ignite_test` database.
//!
//! The snapshots in `tests/fixtures/theme` are the pages as Ignite rendered them before the
//! theme system. After a deliberate change to the `ignite` theme, write them again with
//! `IGNITE_BLESS=1 cargo test -p ignite-panel --test theme`, and review the diff.

mod common;

use std::path::PathBuf;

use common::*;
use rocket::http::Status;
use rocket::local::blocking::Client;

/// The file of the snapshot `name`.
fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/theme")
        .join(format!("{name}.html"))
}

/// The HTML of `path`, with the client's CSRF token replaced by `{csrf}`, because the token
/// is new for each client.
fn page(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    let body = response.into_string().unwrap();
    match client.cookies().get("ignite_csrf") {
        Some(_) => body.replace(&csrf_token(client), "{csrf}"),
        None => body,
    }
}

/// Checks `html` against the snapshot `name`, or writes it when `IGNITE_BLESS` is set.
fn assert_snapshot(name: &str, html: &str) {
    let path = snapshot_path(name);
    if std::env::var_os("IGNITE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, html).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read the snapshot {}: {error}", path.display()));
    assert!(
        html == expected,
        "the page `{name}` differs from its snapshot {}:\n{html}",
        path.display()
    );
}

/// The pages of the panel, signed in and signed out, as `(snapshot, HTML)`.
fn pages() -> Vec<(&'static str, String)> {
    let client = signed_in(None);
    let mut pages = vec![
        ("dashboard", page(&client, "/admin")),
        ("index", page(&client, "/admin/widgets")),
        ("create", page(&client, "/admin/widgets/new")),
        ("edit", page(&client, "/admin/widgets/1/edit")),
        ("profile", page(&client, "/admin/profile")),
    ];
    let guest = client_signed_out();
    pages.push(("login", page(&guest, "/admin/login")));
    pages.push(("forgot", page(&guest, "/admin/password/forgot")));
    pages
}

fn client_signed_out() -> Client {
    client(None)
}

#[test]
fn the_ignite_theme_renders_the_same_html_as_before() {
    let _guard = reset();
    for (name, html) in pages() {
        assert_snapshot(name, &html);
    }
}
