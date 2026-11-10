//! The panel's cookies get the `Secure` flag when the request was made over HTTPS. Directly,
//! that is a server configured for TLS, which a local client can simulate through its
//! configuration without opening a listener. Through a proxy, the flag follows
//! `X-Forwarded-Proto`, but only when the socket is listed in `IGNITE_TRUSTED_PROXIES`.

mod common;

use common::{client, client_on, reset};
use rocket::http::Header;
use rocket::local::blocking::Client;

const PROXY: &str = "192.0.2.10:4000";
const DIRECT: &str = "198.51.100.50:4000";

/// A client that trusts 192.0.2.10. The variable is read when the panel is built.
fn proxied_client() -> Client {
    std::env::set_var("IGNITE_TRUSTED_PROXIES", "192.0.2.10");
    let client = client(None);
    std::env::remove_var("IGNITE_TRUSTED_PROXIES");
    client
}

/// A client for a server configured for TLS. The certificate files are named but not read,
/// because the local client never starts a listener.
fn tls_client() -> Client {
    let figment = rocket::Config::figment()
        .merge(("tls.certs", "tests/fixtures/tls/cert.pem"))
        .merge(("tls.key", "tests/fixtures/tls/key.pem"));
    client_on(None, rocket::custom(figment))
}

/// The `Set-Cookie` values of a login page request, from `remote`, with `proto` as the
/// `X-Forwarded-Proto` header when it is given.
fn panel_cookies(client: &Client, remote: &str, proto: Option<&str>) -> Vec<String> {
    let mut request = client
        .get("/admin/login")
        .remote(remote.parse::<std::net::SocketAddr>().unwrap());
    if let Some(proto) = proto {
        request = request.header(Header::new("X-Forwarded-Proto", proto.to_string()));
    }
    let response = request.dispatch();
    response
        .headers()
        .get("Set-Cookie")
        .filter(|c| c.starts_with("ignite_"))
        .map(str::to_string)
        .collect()
}

fn assert_all_secure(cookies: &[String]) {
    assert!(!cookies.is_empty(), "the login page sets the token cookie");
    for cookie in cookies {
        assert!(cookie.contains("; Secure"), "missing Secure: {cookie}");
    }
}

fn assert_none_secure(cookies: &[String]) {
    assert!(!cookies.is_empty(), "the login page sets the token cookie");
    for cookie in cookies {
        assert!(!cookie.contains("Secure"), "unexpected Secure: {cookie}");
    }
}

#[test]
fn a_direct_http_request_gets_cookies_without_secure() {
    let _guard = reset();
    let client = client(None);
    assert_none_secure(&panel_cookies(&client, DIRECT, None));
}

#[test]
fn a_server_configured_for_tls_gets_secure_cookies_on_every_request() {
    let _guard = reset();
    assert_all_secure(&panel_cookies(&tls_client(), DIRECT, None));
    // A fresh client, because the first response's cookie would otherwise be kept and not re-sent.
    assert_all_secure(&panel_cookies(&tls_client(), DIRECT, Some("http")));
}

#[test]
fn a_trusted_proxy_saying_https_gets_secure_cookies() {
    let _guard = reset();
    let client = proxied_client();
    assert_all_secure(&panel_cookies(&client, PROXY, Some("https")));
}

#[test]
fn a_trusted_proxy_saying_http_gets_cookies_without_secure() {
    let _guard = reset();
    let client = proxied_client();
    assert_none_secure(&panel_cookies(&client, PROXY, Some("http")));
}

#[test]
fn a_forwarded_scheme_from_an_untrusted_socket_is_ignored() {
    let _guard = reset();
    let client = client(None);
    assert_none_secure(&panel_cookies(&client, DIRECT, Some("https")));
}
