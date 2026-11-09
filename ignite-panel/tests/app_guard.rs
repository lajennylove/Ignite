//! An app's own route under the panel's mount path uses the panel's sign-in guard.

mod common;

use common::{reset, EMAIL, PASSWORD};
use ignite_panel::{Ignite, Session};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use rocket::{get, routes};

#[get("/admin/app-private")]
fn app_private(session: Session) -> String {
    format!("user {}", session.user_id())
}

fn client() -> Client {
    Client::tracked(
        rocket::build()
            .attach(Ignite::new().fairing())
            .mount("/", routes![app_private]),
    )
    .expect("valid rocket")
}

#[test]
fn an_app_route_redirects_to_login_without_a_session() {
    let _guard = reset();
    let client = client();
    let response = client.get("/admin/app-private").dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin/login"));
}

#[test]
fn an_app_route_reads_the_signed_in_user() {
    let _guard = reset();
    let client = client();
    let login = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(format!("email={EMAIL}&password={PASSWORD}"))
        .dispatch();
    assert_eq!(login.status(), Status::SeeOther);

    let response = client.get("/admin/app-private").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response.into_string().unwrap().starts_with("user "));
}
