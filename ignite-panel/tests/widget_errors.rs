//! A form widget's validation errors show on the widget, on the dashboard, as a Filament
//! widget's form shows its errors on the form
//! (<https://filamentphp.com/docs/4.x/widgets/overview>, "Custom widgets": a widget is a
//! Livewire component, so its form validates in place). A save the resource refuses answers
//! 422 with the dashboard, the messages under the widget's fields and the typed values kept.
//! Runs against the test database.

mod common;

use common::*;
use ignite_core::FormWidget;
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

fn quick_draft() -> FormWidget {
    FormWidget::make("Quick Draft", "widgets")
        .fields(["name", "note"])
        .hidden("price", "5")
        .submit_label("Save Draft")
}

fn second() -> FormWidget {
    FormWidget::make("Another Draft", "widgets").fields(["name"])
}

fn post(client: &Client, body: &str) -> (Status, String) {
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(client, body))
        .dispatch();
    let status = response.status();
    (status, response.into_string().unwrap_or_default())
}

#[test]
fn an_empty_required_field_shows_its_message_on_the_dashboard() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.form_widget(quick_draft()).form_widget(second()));
    let dashboard = client.get("/admin").dispatch().into_string().unwrap();
    assert!(
        dashboard.contains("name=\"_widget\" value=\"0\""),
        "{dashboard}"
    );
    assert!(dashboard.contains("name=\"_widget\" value=\"1\""));

    let before = count_widgets();
    let (status, html) = post(
        &client,
        "_back=%2Fadmin&_widget=0&price=5&name=&note=Typed+%3Cnote%3E",
    );
    assert_eq!(status, Status::UnprocessableEntity);
    assert_eq!(count_widgets(), before, "nothing is saved");

    // The dashboard, not the resource's form page: the cards and both widgets are there.
    assert!(html.contains("ig-tone-primary"), "the stat cards: {html}");
    assert!(html.contains("Quick Draft"));
    assert!(html.contains("Another Draft"));

    // The message is under the refused widget's field, once, and the typed value is kept,
    // escaped.
    assert_eq!(html.matches("Name is required.").count(), 1, "{html}");
    let quick = html.find("Quick Draft").unwrap();
    let message = html.find("Name is required.").unwrap();
    let another = html.find("Another Draft").unwrap();
    assert!(quick < message && message < another, "{html}");
    assert!(html.contains("Typed &lt;note&gt;"), "{html}");
    assert!(html.contains("name=\"price\" value=\"5\""));
}

#[test]
fn the_widget_the_form_names_shows_the_message() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.form_widget(quick_draft()).form_widget(second()));
    let (status, html) = post(&client, "_back=%2Fadmin&_widget=1&price=5&name=");
    assert_eq!(status, Status::UnprocessableEntity);
    let message = html.find("Name is required.").expect("the message");
    assert!(html.find("Another Draft").unwrap() < message, "{html}");
}

#[test]
fn a_form_without_its_widget_key_shows_on_the_resources_first_widget() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.form_widget(quick_draft()));
    for body in [
        "_back=%2Fadmin&price=5&name=",
        "_back=%2Fadmin&_widget=9&price=5&name=",
    ] {
        let (status, html) = post(&client, body);
        assert_eq!(status, Status::UnprocessableEntity);
        assert!(html.contains("Quick Draft"), "{html}");
        assert!(html.contains("Name is required."), "{html}");
    }
}

#[test]
fn the_resources_own_form_keeps_its_errors_page() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.form_widget(quick_draft()));
    // Not from the dashboard: the resource's create form shows the messages, as before.
    let (status, html) = post(&client, "price=5&name=");
    assert_eq!(status, Status::UnprocessableEntity);
    assert!(html.contains("Name is required."));
    assert!(!html.contains("Quick Draft"), "{html}");
}

#[test]
fn a_valid_widget_form_still_comes_back_to_the_dashboard() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.form_widget(quick_draft()));
    let response = client
        .post("/admin/widgets")
        .header(ContentType::Form)
        .body(with_token(
            &client,
            "_back=%2Fadmin&_widget=0&price=5&name=Saved+one",
        ))
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    assert_eq!(response.headers().get_one("Location"), Some("/admin"));
}
