//! The pages of the panel as snapshot tests compare them: the HTML of a page with the CSRF
//! token replaced, and the snapshots in `tests/fixtures/theme`.

use std::path::PathBuf;

use rocket::http::Status;
use rocket::local::blocking::Client;

use super::{client, csrf_token, signed_in};

/// The file of the snapshot `name`.
pub fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/theme")
        .join(format!("{name}.html"))
}

/// The HTML of `path`, with the client's CSRF token replaced by `{csrf}`, because the token
/// is new for each client.
pub fn page(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    let body = response.into_string().unwrap();
    match client.cookies().get("ignite_csrf") {
        Some(_) => body.replace(&csrf_token(client), "{csrf}"),
        None => body,
    }
}

/// Checks `html` against the snapshot `name`, or writes it when `IGNITE_BLESS` is set.
pub fn assert_snapshot(name: &str, html: &str) {
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
pub fn pages() -> Vec<(&'static str, String)> {
    let user = signed_in(None);
    let mut pages = vec![
        ("dashboard", page(&user, "/admin")),
        ("index", page(&user, "/admin/widgets")),
        ("create", page(&user, "/admin/widgets/new")),
        ("edit", page(&user, "/admin/widgets/1/edit")),
        ("profile", page(&user, "/admin/profile")),
    ];
    let guest = client(None);
    pages.push(("login", page(&guest, "/admin/login")));
    pages.push(("forgot", page(&guest, "/admin/password/forgot")));
    pages
}
