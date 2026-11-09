//! Debug builds read the panel's views from a directory when one is named, so an
//! edit shows on refresh. A release build renders compiled views and ignores it.

use ignite_panel::Ignite;
use rocket::http::Status;
use rocket::local::blocking::Client;
#[cfg(debug_assertions)]
use std::path::{Path, PathBuf};

#[cfg(debug_assertions)]
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[cfg(debug_assertions)]
#[test]
fn the_panel_renders_from_a_copied_views_directory() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("views/ignite");
    let copy: PathBuf = std::env::temp_dir().join(format!("ignite-views-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&copy);
    copy_dir(&source, &copy);

    let client =
        Client::tracked(rocket::build().attach(Ignite::new().views_dir(&copy).fairing())).unwrap();
    let response = client.get("/admin/login").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response.into_string().unwrap().contains("Sign in"));

    let _ = std::fs::remove_dir_all(&copy);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "could not find its views")]
fn a_missing_views_directory_stops_the_app_at_startup() {
    let _ = Client::tracked(
        rocket::build().attach(
            Ignite::new()
                .views_dir("/nonexistent/ignite-views")
                .fairing(),
        ),
    );
}

#[cfg(not(debug_assertions))]
#[test]
fn a_release_build_ignores_the_views_directory_and_renders_compiled_views() {
    std::env::set_var(
        "IGNITE_SECRET_KEY",
        "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=",
    );
    let client = Client::tracked(
        rocket::build().attach(
            Ignite::new()
                .views_dir("/nonexistent/ignite-views")
                .fairing(),
        ),
    )
    .unwrap();
    let response = client.get("/admin/login").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response.into_string().unwrap().contains("Sign in"));
}
