//! A live check of the Emailit plugin. It sends one real email, so it runs only when
//! `EMAILIT_API_KEY`, `IGNITE_MAIL_FROM` and `IGNITE_TEST_MAIL_TO` are all set. Otherwise it
//! prints a notice and passes. The rest of the suite never touches the network.

use ignite_panel::{EmailitMailer, Mailer, Message};

#[test]
fn emailit_accepts_a_test_message() {
    rocket::async_test(async {
        let key = std::env::var("EMAILIT_API_KEY").ok();
        let from = std::env::var("IGNITE_MAIL_FROM").ok();
        let to = std::env::var("IGNITE_TEST_MAIL_TO").ok();
        let (Some(key), Some(from), Some(to)) = (key, from, to) else {
            eprintln!(
            "skipped: set EMAILIT_API_KEY, IGNITE_MAIL_FROM and IGNITE_TEST_MAIL_TO to run the live Emailit test"
        );
            return;
        };

        let mailer = EmailitMailer::new(key, from);
        let message = Message {
            to,
            subject: "Ignite live mail test".to_string(),
            text: "This is a test message from the Ignite test suite.".to_string(),
            html: "<p>This is a test message from the Ignite test suite.</p>".to_string(),
            idempotency_key: Some(format!("ignite-live-test-{}", std::process::id())),
        };
        mailer
            .send(&message)
            .await
            .expect("Emailit accepts the test message");
    });
}
