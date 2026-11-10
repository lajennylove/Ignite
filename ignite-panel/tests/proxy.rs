//! Which address the login throttle counts: the socket address, unless the socket is a
//! trusted proxy listed in `IGNITE_TRUSTED_PROXIES`. Then the right-most forwarded
//! entry that is not a trusted proxy. A spoofed `X-Real-IP` never changes the bucket.

mod common;

use common::{client, reset, with_token, EMAIL, PASSWORD};
use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::{Client, LocalResponse};

/// A trusted proxy, and the networks it may forward for.
const PROXY: &str = "192.0.2.10:4000";
const TRUSTED: &str = "192.0.2.10, 10.0.0.0/8";
/// A socket address that is not a trusted proxy.
const DIRECT: &str = "198.51.100.50:4000";

type Headers = Vec<(&'static str, String)>;

/// A client built with `IGNITE_TRUSTED_PROXIES` set. The variable is read when the
/// panel is built, so it is removed again straight away. Callers hold the database lock.
fn proxied_client() -> Client {
    std::env::set_var("IGNITE_TRUSTED_PROXIES", TRUSTED);
    let client = client(None);
    std::env::remove_var("IGNITE_TRUSTED_PROXIES");
    client
}

/// One sign-in attempt from `remote` with `headers` and a fresh token.
fn attempt<'c>(
    client: &'c Client,
    password: &str,
    remote: &str,
    headers: &Headers,
) -> LocalResponse<'c> {
    client.get("/admin/login").dispatch();
    let body = with_token(client, &format!("email={EMAIL}&password={password}"));
    let mut request = client
        .post("/admin/login")
        .remote(remote.parse::<std::net::SocketAddr>().unwrap())
        .header(ContentType::Form);
    for (name, value) in headers {
        request = request.header(Header::new(*name, value.clone()));
    }
    request.body(body).dispatch()
}

/// Five wrong passwords from `remote`. Attempt `n` (from 1) carries `headers(n)`.
fn fail_five_times(client: &Client, remote: &str, headers: impl Fn(usize) -> Headers) {
    for n in 1..=5 {
        assert_eq!(
            attempt(client, "wrong-password", remote, &headers(n)).status(),
            Status::UnprocessableEntity
        );
    }
}

fn header(name: &'static str, value: &str) -> Headers {
    vec![(name, value.to_string())]
}

#[test]
fn a_spoofed_real_ip_does_not_change_the_bucket() {
    let _guard = reset();
    let client = client(None);
    fail_five_times(&client, DIRECT, |n| {
        header("X-Real-IP", &format!("203.0.113.{n}"))
    });

    // A sixth attempt with a brand-new X-Real-IP is still the same client.
    let refused = attempt(
        &client,
        PASSWORD,
        DIRECT,
        &header("X-Real-IP", "203.0.113.99"),
    );
    assert_eq!(refused.status(), Status::TooManyRequests);
}

#[test]
fn forwarded_for_is_ignored_from_an_untrusted_socket() {
    let _guard = reset();
    let client = client(None);
    fail_five_times(&client, DIRECT, |n| {
        header("X-Forwarded-For", &format!("203.0.113.{n}"))
    });

    let refused = attempt(
        &client,
        PASSWORD,
        DIRECT,
        &header("X-Forwarded-For", "203.0.113.99"),
    );
    assert_eq!(refused.status(), Status::TooManyRequests);
}

#[test]
fn forwarded_for_is_honoured_from_a_trusted_socket() {
    let _guard = reset();
    let client = proxied_client();
    fail_five_times(&client, PROXY, |_| header("X-Forwarded-For", "203.0.113.9"));

    // The same forwarded client is still limited.
    let same = attempt(
        &client,
        PASSWORD,
        PROXY,
        &header("X-Forwarded-For", "203.0.113.9"),
    );
    assert_eq!(same.status(), Status::TooManyRequests);

    // Another forwarded client behind the same proxy has its own bucket.
    let other = attempt(
        &client,
        PASSWORD,
        PROXY,
        &header("X-Forwarded-For", "203.0.113.20"),
    );
    assert_eq!(other.status(), Status::SeeOther);
}

#[test]
fn the_right_most_untrusted_forwarded_entry_is_the_client() {
    let _guard = reset();
    let client = proxied_client();
    // The left entries are what the client claimed; 203.0.113.9 is the right-most untrusted one.
    fail_five_times(&client, PROXY, |_| {
        header("X-Forwarded-For", "1.1.1.1, 203.0.113.9, 10.0.0.4")
    });

    let same = attempt(
        &client,
        PASSWORD,
        PROXY,
        &header("X-Forwarded-For", "2.2.2.2, 203.0.113.9, 10.0.0.5"),
    );
    assert_eq!(same.status(), Status::TooManyRequests);
}

#[test]
fn a_spoofed_real_ip_from_a_trusted_socket_is_still_ignored() {
    let _guard = reset();
    let client = proxied_client();
    let forwarded = |real: &str| -> Headers {
        vec![
            ("X-Real-IP", real.to_string()),
            ("X-Forwarded-For", "203.0.113.9".to_string()),
        ]
    };
    fail_five_times(&client, PROXY, |_| forwarded("203.0.113.1"));

    let refused = attempt(&client, PASSWORD, PROXY, &forwarded("203.0.113.77"));
    assert_eq!(refused.status(), Status::TooManyRequests);
}
