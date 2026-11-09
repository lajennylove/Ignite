//! Sign-in sessions and flash messages. Both live in cookies that are signed
//! with `IGNITE_SECRET_KEY`, so the server keeps no session state.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use rocket::catch;
use rocket::http::{Cookie, CookieJar, SameSite, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::Redirect;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::ignite::Registry;

type HmacSha256 = Hmac<Sha256>;

/// The cookie that holds the signed-in user.
pub(crate) const SESSION_COOKIE: &str = "ignite_session";
/// The cookie that holds the flash message until the next page shows it.
pub(crate) const FLASH_COOKIE: &str = "ignite_flash";
/// How long a sign-in lasts, in seconds: eight hours.
const SESSION_SECONDS: u64 = 8 * 60 * 60;
/// The shortest key the panel accepts, in bytes.
const MIN_KEY_BYTES: usize = 32;

#[derive(Serialize, Deserialize)]
struct SessionPayload {
    uid: i64,
    exp: u64,
}

#[derive(Serialize, Deserialize)]
struct FlashPayload {
    message: String,
}

/// Reads the signing key from `IGNITE_SECRET_KEY`, a base64 string of at least
/// 32 bytes. In a release build a missing or short key panics. In a debug build
/// a missing key is replaced by a random one for this run, with a warning.
pub(crate) fn secret_from_env() -> Vec<u8> {
    match std::env::var("IGNITE_SECRET_KEY") {
        Ok(encoded) => {
            let key = STANDARD
                .decode(encoded.trim())
                .expect("IGNITE_SECRET_KEY must be base64 (try `openssl rand -base64 32`)");
            assert!(
                key.len() >= MIN_KEY_BYTES,
                "IGNITE_SECRET_KEY must decode to at least {MIN_KEY_BYTES} bytes"
            );
            key
        }
        Err(_) if cfg!(debug_assertions) => {
            rocket::warn!("IGNITE_SECRET_KEY is not set: using a random key, so sign-ins end when the app restarts");
            let mut key = vec![0u8; MIN_KEY_BYTES];
            OsRng.fill_bytes(&mut key);
            key
        }
        Err(_) => panic!(
            "IGNITE_SECRET_KEY must be set in a release build (try `openssl rand -base64 32`)"
        ),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn mac(key: &[u8], kind: &str, body: &str) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(kind.as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    mac
}

/// Signs `value` as `body.tag`. The `kind` keeps a session from being read as a flash.
fn seal<T: Serialize>(key: &[u8], kind: &str, value: &T) -> String {
    let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("payloads serialize"));
    let tag = URL_SAFE_NO_PAD.encode(mac(key, kind, &body).finalize().into_bytes());
    format!("{body}.{tag}")
}

/// Returns the payload if the signature is valid for `kind`.
fn open<T: DeserializeOwned>(key: &[u8], kind: &str, sealed: &str) -> Option<T> {
    let (body, tag) = sealed.split_once('.')?;
    let tag = URL_SAFE_NO_PAD.decode(tag).ok()?;
    mac(key, kind, body).verify_slice(&tag).ok()?;
    let payload = URL_SAFE_NO_PAD.decode(body).ok()?;
    serde_json::from_slice(&payload).ok()
}

fn cookie(name: &'static str, value: String) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        // Release builds send the cookie only over HTTPS.
        .secure(!cfg!(debug_assertions))
        .build()
}

/// Starts a sign-in for user `user_id`.
pub(crate) fn start_session(cookies: &CookieJar<'_>, key: &[u8], user_id: i64) {
    let payload = SessionPayload {
        uid: user_id,
        exp: now() + SESSION_SECONDS,
    };
    let mut session = cookie(SESSION_COOKIE, seal(key, "session", &payload));
    session.set_max_age(rocket::time::Duration::seconds(SESSION_SECONDS as i64));
    cookies.add(session);
}

/// Ends the sign-in.
pub(crate) fn end_session(cookies: &CookieJar<'_>) {
    cookies.remove(cookie(SESSION_COOKIE, String::new()));
}

/// Stores a flash message for the next page.
pub(crate) fn set_flash(cookies: &CookieJar<'_>, key: &[u8], message: &str) {
    let payload = FlashPayload {
        message: message.to_string(),
    };
    cookies.add(cookie(FLASH_COOKIE, seal(key, "flash", &payload)));
}

/// Reads the flash message and clears it, so it shows once.
pub(crate) fn take_flash(cookies: &CookieJar<'_>, key: &[u8]) -> Option<String> {
    let message = cookies
        .get(FLASH_COOKIE)
        .and_then(|c| open::<FlashPayload>(key, "flash", c.value()))
        .map(|p| p.message);
    if message.is_some() {
        cookies.remove(cookie(FLASH_COOKIE, String::new()));
    }
    message
}

/// A request guard that needs a signed-in user. The panel's own routes use it,
/// and so can an app's route under the panel's mount path (added in v1):
///
/// ```ignore
/// #[get("/admin/reports")]
/// fn reports(session: ignite_panel::AdminUser) -> PropelView { /* session.user_id() */ }
/// ```
///
/// A request without a valid, unexpired sign-in fails with 401, and the panel's
/// catcher turns that into a redirect to the login page for paths under the mount.
#[derive(Debug, Clone, Copy)]
pub struct AdminUser {
    user_id: i64,
}

impl AdminUser {
    /// The signed-in user's id.
    pub fn user_id(&self) -> i64 {
        self.user_id
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AdminUser {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        let Some(registry) = request.rocket().state::<Registry>() else {
            return Outcome::Error((Status::InternalServerError, ()));
        };
        let payload = request
            .cookies()
            .get(SESSION_COOKIE)
            .and_then(|c| open::<SessionPayload>(&registry.secret, "session", c.value()));
        match payload {
            Some(p) if p.exp > now() => Outcome::Success(AdminUser { user_id: p.uid }),
            _ => Outcome::Error((Status::Unauthorized, ())),
        }
    }
}

/// Sends a request for a page under the panel that has no sign-in to the login page.
/// Other 401 responses keep their status.
#[catch(401)]
pub(crate) fn unauthorized(request: &Request<'_>) -> Result<Redirect, Status> {
    let registry = request
        .rocket()
        .state::<Registry>()
        .ok_or(Status::Unauthorized)?;
    let path = request.uri().path().as_str();
    if path == registry.mount || path.starts_with(&format!("{}/", registry.mount)) {
        Ok(Redirect::to(format!("{}/login", registry.mount)))
    } else {
        Err(Status::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn a_sealed_payload_opens_with_its_key_and_kind() {
        let sealed = seal(
            KEY,
            "flash",
            &FlashPayload {
                message: "Saved.".into(),
            },
        );

        assert_eq!(
            open::<FlashPayload>(KEY, "flash", &sealed).unwrap().message,
            "Saved."
        );
    }

    #[test]
    fn a_payload_does_not_open_under_another_key_or_kind() {
        let sealed = seal(
            KEY,
            "session",
            &SessionPayload {
                uid: 7,
                exp: u64::MAX,
            },
        );

        assert!(
            open::<SessionPayload>(b"another key of at least 32 bytes!!", "session", &sealed)
                .is_none()
        );
        assert!(open::<SessionPayload>(KEY, "flash", &sealed).is_none());
    }

    #[test]
    fn a_tampered_payload_does_not_open() {
        let sealed = seal(
            KEY,
            "session",
            &SessionPayload {
                uid: 7,
                exp: u64::MAX,
            },
        );
        let (_, tag) = sealed.split_once('.').unwrap();
        let forged_body = URL_SAFE_NO_PAD.encode(br#"{"uid":8,"exp":18446744073709551615}"#);

        assert!(open::<SessionPayload>(KEY, "session", &format!("{forged_body}.{tag}")).is_none());
    }
}
