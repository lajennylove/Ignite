//! The panel's own stylesheet and script: its pages link the built files with a
//! hash, the files are served under the mount, and the stylesheet has the classes
//! the views use. Runs in debug and release, which read the build differently.

mod common;

use common::{client, reset, signed_in};
use rocket::http::Status;

/// The link to a built file with `extension` in `body`, such as
/// `/admin/build/assets/app-1a2b.css`.
fn built_url(body: &str, extension: &str) -> String {
    body.match_indices("/admin/build/assets/app-")
        .map(|(start, _)| {
            let rest = &body[start..];
            rest[..rest.find('"').expect("the link is quoted")].to_string()
        })
        .find(|url| url.ends_with(extension))
        .unwrap_or_else(|| panic!("no .{extension} file linked in: {body}"))
}

/// The hash in a built file's name, between `app-` and the extension.
fn hash_of(url: &str) -> &str {
    let name = url.rsplit('/').next().unwrap();
    let start = name.find("app-").unwrap() + 4;
    let end = name.rfind('.').unwrap();
    &name[start..end]
}

#[test]
fn the_login_page_links_the_built_stylesheet_and_script_with_a_hash() {
    let _guard = reset();
    let client = client(None);
    let body = client.get("/admin/login").dispatch().into_string().unwrap();

    let css = built_url(&body, ".css");
    let js = built_url(&body, ".js");
    assert!(
        body.contains(&format!("href=\"{css}\"")),
        "stylesheet link: {css}"
    );
    assert!(body.contains(&format!("src=\"{js}\"")), "script link: {js}");
    assert!(!hash_of(&css).is_empty(), "the stylesheet name has a hash");
    assert!(!hash_of(&js).is_empty(), "the script name has a hash");
}

#[test]
fn the_built_stylesheet_is_served_and_has_the_classes_the_views_use() {
    let _guard = reset();
    let client = client(None);
    let body = client.get("/admin/login").dispatch().into_string().unwrap();
    let css_url = built_url(&body, ".css");

    let response = client.get(css_url).dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .content_type()
        .unwrap()
        .to_string()
        .starts_with("text/css"));
    let css = response.into_string().unwrap();
    assert!(
        css.contains(".bg-accent"),
        "a utility the views use is in the stylesheet"
    );
    assert!(
        css.contains(".text-danger"),
        "the error colour is in the stylesheet"
    );
}

#[test]
fn the_built_script_is_served() {
    let _guard = reset();
    let client = client(None);
    let body = client.get("/admin/login").dispatch().into_string().unwrap();
    let js_url = built_url(&body, ".js");

    let response = client.get(js_url).dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(
        response.into_string().unwrap().contains("confirm("),
        "the confirm dialog script"
    );
}

#[test]
fn an_unknown_built_file_is_not_found() {
    let _guard = reset();
    let client = client(None);
    assert_eq!(
        client
            .get("/admin/build/assets/missing-0000.css")
            .dispatch()
            .status(),
        Status::NotFound
    );
}

#[test]
fn the_signed_in_pages_load_the_stylesheet() {
    let _guard = reset();
    let client = signed_in(None);
    for path in ["/admin", "/admin/widgets", "/admin/widgets/new"] {
        let response = client.get(path).dispatch();
        assert_eq!(response.status(), Status::Ok, "{path}");
        let body = response.into_string().unwrap();
        assert!(
            body.contains("/admin/build/assets/app-"),
            "{path} links the stylesheet"
        );
    }
}

#[test]
fn the_419_page_loads_the_stylesheet() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/widgets")
        .header(rocket::http::ContentType::Form)
        .body("name=Date&price=5")
        .dispatch();

    assert_eq!(response.status().code, 419);
    let body = response.into_string().unwrap();
    assert!(body.contains("Page Expired"));
    assert!(
        body.contains("/admin/build/assets/app-"),
        "the expired page is styled too"
    );
}
