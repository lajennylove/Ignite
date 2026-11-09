//! Every state-changing request under the panel must carry the browser session's
//! CSRF token, as Laravel's VerifyCsrfToken requires. A missing or wrong token is
//! refused with 419.

mod common;

use common::{client, count_widgets, csrf_token, reset, signed_in, with_token, EMAIL, PASSWORD};
use rocket::http::{ContentType, Status};

const EXPIRED: u16 = 419;

#[test]
fn a_create_without_a_token_is_refused_with_419() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body("name=Date&price=5")
        .dispatch();

    assert_eq!(response.status().code, EXPIRED);
    assert!(response.into_string().unwrap().contains("Page Expired"));
    assert_eq!(count_widgets(), 3, "nothing is written");
}

#[test]
fn a_create_with_a_wrong_token_is_refused_with_419() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body("name=Date&price=5&_token=not-the-token")
        .dispatch();

    assert_eq!(response.status().code, EXPIRED);
    assert_eq!(count_widgets(), 3);
}

#[test]
fn a_create_with_the_correct_token_is_accepted() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(&client, "name=Date&price=5"))
        .dispatch();

    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(count_widgets(), 4);
}

#[test]
fn a_token_from_another_browser_session_is_refused() {
    let _guard = reset();
    let mine = signed_in(None);
    let theirs = signed_in(None);
    let stolen = csrf_token(&theirs);

    let response = mine
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(format!("name=Date&price=5&_token={stolen}"))
        .dispatch();

    assert_eq!(response.status().code, EXPIRED);
    assert_eq!(count_widgets(), 3);
}

#[test]
fn an_update_and_a_delete_need_the_token_too() {
    let _guard = reset();
    let client = signed_in(None);

    let update = client
        .post("/admin/widgets/1")
        .header(ContentType::Form)
        .body("name=Apricot&price=4")
        .dispatch();
    assert_eq!(update.status().code, EXPIRED);

    let delete = client.post("/admin/widgets/2/delete").dispatch();
    assert_eq!(delete.status().code, EXPIRED);
    assert_eq!(count_widgets(), 3);
}

#[test]
fn a_login_without_a_token_is_refused_and_signs_nobody_in() {
    let _guard = reset();
    let client = client(None);
    client.get("/admin/login").dispatch();
    let response = client
        .post("/admin/login")
        .header(ContentType::Form)
        .body(format!("email={EMAIL}&password={PASSWORD}"))
        .dispatch();

    assert_eq!(response.status().code, EXPIRED);
    assert_eq!(
        client.get("/admin/widgets").dispatch().status(),
        Status::SeeOther
    );
}

#[test]
fn a_logout_without_a_token_keeps_the_session() {
    let _guard = reset();
    let client = signed_in(None);
    let response = client.post("/admin/logout").dispatch();

    assert_eq!(response.status().code, EXPIRED);
    assert_eq!(client.get("/admin/widgets").dispatch().status(), Status::Ok);
}

#[test]
fn every_form_carries_the_token() {
    let _guard = reset();
    let client = signed_in(None);
    let token = csrf_token(&client);
    let field = format!(r#"name="_token" value="{token}""#);

    let new_form = client
        .get("/admin/widgets/new")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(new_form.contains(&field), "the create form has the token");

    let index = client
        .get("/admin/widgets")
        .dispatch()
        .into_string()
        .unwrap();
    assert!(
        index.contains(&field),
        "the delete and sign-out forms have the token"
    );

    let login = client.get("/admin/login").dispatch().into_string().unwrap();
    assert!(login.contains(&field), "the login form has the token");
}
