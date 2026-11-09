//! Sign-in sessions and flash messages. A sign-in is a row in the `sessions`
//! table, and its signed cookie holds only the row's id, so the server can end a
//! session at any time. Flash messages live in their own signed cookie.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use rocket::catch;
use rocket::http::{Cookie, CookieJar, SameSite, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::content::RawHtml;
use rocket::response::Redirect;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::ignite::Registry;
use crate::{sessions, users};
use ignite_core::Pool;

type HmacSha256 = Hmac<Sha256>;

/// The cookie that holds the signed-in user.
pub(crate) const SESSION_COOKIE: &str = "ignite_session";
/// The cookie that holds the CSRF token of the browser session. Every form
/// sends the token back in a hidden `_token` field.
pub(crate) const CSRF_COOKIE: &str = "ignite_csrf";
/// The cookie that holds the flash message until the next page shows it.
pub(crate) const FLASH_COOKIE: &str = "ignite_flash";
/// The shortest key the panel accepts, in bytes.
const MIN_KEY_BYTES: usize = 32;

#[derive(Serialize, Deserialize)]
struct SessionPayload {
    sid: String,
}

#[derive(Serialize, Deserialize)]
struct CsrfPayload {
    token: String,
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

/// Sets the session cookie for session `session_id`, the id of a `sessions` row.
pub(crate) fn start_session(cookies: &CookieJar<'_>, key: &[u8], session_id: &str) {
    let payload = SessionPayload {
        sid: session_id.to_string(),
    };
    cookies.add(cookie(SESSION_COOKIE, seal(key, "session", &payload)));
}

/// The session id in the request's cookie, if the cookie is validly signed.
/// Whether the session is still live is for the database to say.
pub(crate) fn session_id(cookies: &CookieJar<'_>, key: &[u8]) -> Option<String> {
    cookies
        .get(SESSION_COOKIE)
        .and_then(|c| open::<SessionPayload>(key, "session", c.value()))
        .map(|p| p.sid)
}

/// Ends the sign-in.
pub(crate) fn end_session(cookies: &CookieJar<'_>) {
    cookies.remove(cookie(SESSION_COOKIE, String::new()));
}

/// The CSRF token of this browser session, created when it is first needed.
pub(crate) fn csrf_token(cookies: &CookieJar<'_>, key: &[u8]) -> String {
    if let Some(token) = cookies
        .get(CSRF_COOKIE)
        .and_then(|c| open::<CsrfPayload>(key, "csrf", c.value()))
        .map(|p| p.token)
    {
        return token;
    }
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let payload = CsrfPayload {
        token: token.clone(),
    };
    cookies.add(cookie(CSRF_COOKIE, seal(key, "csrf", &payload)));
    token
}

/// Checks the `_token` of a state-changing request against this browser session's
/// token. A missing or wrong token fails with 419, the status Laravel uses for an
/// expired page or token mismatch.
pub(crate) fn check_csrf(
    cookies: &CookieJar<'_>,
    key: &[u8],
    submitted: Option<&str>,
) -> Result<(), Status> {
    let expected = cookies
        .get(CSRF_COOKIE)
        .and_then(|c| open::<CsrfPayload>(key, "csrf", c.value()))
        .map(|p| p.token);
    match (expected, submitted) {
        (Some(expected), Some(submitted))
            if constant_time_eq(expected.as_bytes(), submitted.as_bytes()) =>
        {
            Ok(())
        }
        _ => Err(Status::new(419)),
    }
}

/// Compares two byte strings without stopping at the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
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
/// and so can an app's route under the panel's mount path, as Laravel's auth
/// guard does for package routes:
///
/// ```ignore
/// #[get("/admin/reports")]
/// fn reports(user: ignite_panel::AdminUser) -> PropelView { /* user.user_id() */ }
/// ```
///
/// The guard checks the session row, so a session that was ended on the server
/// is refused even if its cookie is still valid. A request without a live
/// sign-in fails with 401, and the panel's catcher redirects paths under the
/// mount to the login page.
#[derive(Debug, Clone)]
pub struct AdminUser {
    user_id: i64,
    name: String,
    email: String,
}

impl AdminUser {
    /// The signed-in user's id.
    pub fn user_id(&self) -> i64 {
        self.user_id
    }

    /// The signed-in user's name, as the panel's top bar shows it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The signed-in user's email.
    pub fn email(&self) -> &str {
        &self.email
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AdminUser {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        let (Some(registry), Some(pool)) = (
            request.rocket().state::<Registry>(),
            request.rocket().state::<Pool>(),
        ) else {
            return Outcome::Error((Status::InternalServerError, ()));
        };
        let Some(sid) = session_id(request.cookies(), &registry.secret) else {
            return Outcome::Error((Status::Unauthorized, ()));
        };
        let pool = pool.clone();
        let lookup = sid.clone();
        let user = rocket::tokio::task::spawn_blocking(move || {
            let mut conn = pool.get().ok()?;
            let user_id = sessions::user_for(&mut conn, &lookup).ok().flatten()?;
            users::find_by_id(&mut conn, user_id).ok().flatten()
        })
        .await
        .ok()
        .flatten();
        match user {
            Some(user) => Outcome::Success(AdminUser {
                user_id: user.id,
                name: user.name,
                email: user.email,
            }),
            None => Outcome::Error((Status::Unauthorized, ())),
        }
    }
}

/// Answers a request that failed the CSRF check: the form expired or its token did not match.
/// The page loads the panel's stylesheet, so it looks like the rest of the panel.
#[catch(419)]
pub(crate) fn csrf_mismatch() -> RawHtml<String> {
    let styles = crate::assets::tags();
    RawHtml(format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Page Expired · Ignite</title>
{styles}
</head>
<body class="min-h-screen bg-white font-sans text-text antialiased">
<main class="mx-auto max-w-xl px-4 py-12">
<h1 class="text-2xl font-semibold text-heading">Page Expired</h1>
<p class="text-[#5b6070]">This form expired, or its security token did not match. Go back, reload the page and try again.</p>
</main>
</body>
</html>"#
    ))
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
        let sealed = seal(KEY, "session", &SessionPayload { sid: "abc".into() });

        assert!(
            open::<SessionPayload>(b"another key of at least 32 bytes!!", "session", &sealed)
                .is_none()
        );
        assert!(open::<SessionPayload>(KEY, "flash", &sealed).is_none());
    }

    #[test]
    fn a_tampered_payload_does_not_open() {
        let sealed = seal(KEY, "session", &SessionPayload { sid: "abc".into() });
        let (_, tag) = sealed.split_once('.').unwrap();
        let forged_body = URL_SAFE_NO_PAD.encode(br#"{"sid":"forged"}"#);

        assert!(open::<SessionPayload>(KEY, "session", &format!("{forged_body}.{tag}")).is_none());
    }
}
