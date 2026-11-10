//! Password reset by email, modelled on Laravel's password reset. A user who forgot the
//! password asks for a link. The link carries a random token, which the table keeps only as
//! a hash, valid for 60 minutes and for one use. The forgot page answers the same way whether
//! or not the email has an account, and a disabled user gets no link.
//!
//! The routes are `GET` and `POST` `/password/forgot`, and `GET` `/password/reset/<token>?email=`
//! with `POST` `/password/reset`, under the panel's prefix. The link is built from
//! `IGNITE_APP_URL` when it is set, and from the request's own scheme and host otherwise.

use std::net::IpAddr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, NaiveDateTime, Utc};
use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::result::QueryResult;
use ignite_core::{FieldErrors, Pool};
use propel_rocket::PropelView;
use rocket::data::Data;
use rocket::http::{CookieJar, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::Redirect;
use rocket::{get, post, routes, Route, State};
use sha2::{Digest, Sha256};

use crate::ignite::Registry;
use crate::mail::Message;
use crate::profile::MIN_PASSWORD_CHARS;
use crate::proxy::{self, TrustedProxies};
use crate::routes::{blocking, db_status, read_form, ClientIp, Reply};
use crate::session::{check_csrf, csrf_token, set_flash, take_flash};
use crate::sessions;
use crate::throttle;
use crate::users;
use crate::views::{compiled, render_page, ForgotPage, Layout, ResetPage};

/// How long a reset link works, in minutes. Laravel's `auth.passwords.users.expire`.
const EXPIRE_MINUTES: i64 = 60;

/// How long after a link is sent before another is sent to the same email, in seconds.
/// Laravel's `auth.passwords.users.throttle`. A repeat inside the window sends nothing, and
/// the answer is the same, so a double submit does not send twice.
const RESEND_SECONDS: i64 = 60;

/// The answer to every forgot request, whether or not the email has an account.
const SENT_MESSAGE: &str =
    "If an account exists for that email, we have emailed a password reset link.";

/// The answer to a reset whose link is wrong, used, or too old.
const INVALID_LINK: &str = "This password reset link is invalid or has expired. Request a new one.";

diesel::table! {
    /// The reset links, one row for each email. See `migrations/20261115000000_password_reset_tokens`.
    password_reset_tokens (email) {
        email -> Text,
        token -> Text,
        created_at -> Timestamp,
    }
}

/// The routes of the reset, mounted under the panel's prefix with the other routes.
pub(crate) fn all() -> Vec<Route> {
    routes![forgot_form, forgot_send, reset_form, reset_save]
}

/// A link that [`issue`] made: the token to put in the URL, and the key the mail carries.
struct Issued {
    token: String,
    idempotency_key: String,
}

/// Makes a new link for `email`, replacing the earlier one. Returns `None` when a link was
/// made in the last [`RESEND_SECONDS`], so nothing is made or sent. The row is locked while
/// it is read, so two submits at once cannot both pass the check.
fn issue(conn: &mut MysqlConnection, email: &str) -> QueryResult<Option<Issued>> {
    conn.transaction(|conn| {
        let now = Utc::now().naive_utc();
        let last: Option<NaiveDateTime> = password_reset_tokens::table
            .find(email)
            .select(password_reset_tokens::created_at)
            .for_update()
            .first(conn)
            .optional()?;
        if let Some(sent) = last {
            if sent > now - Duration::seconds(RESEND_SECONDS) {
                return Ok(None);
            }
        }

        let token = random_token();
        let hash = users::hash_password(&token).map_err(hash_error)?;
        diesel::replace_into(password_reset_tokens::table)
            .values((
                password_reset_tokens::email.eq(email),
                password_reset_tokens::token.eq(hash),
                password_reset_tokens::created_at.eq(now),
            ))
            .execute(conn)?;
        // Links older than the expiry can never work again, so the table keeps none of them.
        diesel::delete(
            password_reset_tokens::table.filter(
                password_reset_tokens::created_at.lt(now - Duration::minutes(EXPIRE_MINUTES)),
            ),
        )
        .execute(conn)?;
        Ok(Some(Issued {
            token,
            idempotency_key: idempotency_key(email, now),
        }))
    })
}

/// Whether `token` is the live link of `email`: the stored hash matches, and the link was
/// made within [`EXPIRE_MINUTES`]. It does not use the link up.
fn is_valid(conn: &mut MysqlConnection, email: &str, token: &str) -> QueryResult<bool> {
    let row: Option<(String, NaiveDateTime)> = password_reset_tokens::table
        .find(email)
        .select((
            password_reset_tokens::token,
            password_reset_tokens::created_at,
        ))
        .first(conn)
        .optional()?;
    let Some((hash, created_at)) = row else {
        return Ok(false);
    };
    let fresh = created_at > Utc::now().naive_utc() - Duration::minutes(EXPIRE_MINUTES);
    Ok(fresh && users::verify_password(&hash, token))
}

/// A random token of 256 bits, in URL-safe base64.
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The key that stops a repeat of the same send. It is made from the email and the moment the
/// link was made, so each link has its own key. The email itself is not sent in the key.
fn idempotency_key(email: &str, made: NaiveDateTime) -> String {
    let mut hasher = Sha256::new();
    hasher.update(email.as_bytes());
    hasher.update(b"|");
    hasher.update(made.and_utc().timestamp_micros().to_be_bytes());
    format!(
        "password-reset-{}",
        URL_SAFE_NO_PAD.encode(hasher.finalize())
    )
}

/// Turns a hashing failure into a database error, so the handlers have one error type.
fn hash_error(error: impl std::fmt::Display) -> diesel::result::Error {
    diesel::result::Error::SerializationError(Box::new(std::io::Error::other(error.to_string())))
}

/// Deletes the link of `email`, after it is used.
fn consume(conn: &mut MysqlConnection, email: &str) -> QueryResult<usize> {
    diesel::delete(password_reset_tokens::table.find(email)).execute(conn)
}

/// The panel's base URL for the links: `IGNITE_APP_URL` when the app set it, otherwise the
/// scheme and host of the request, with the same HTTPS check the session cookies use.
struct AppBase(String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AppBase {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        let configured = request
            .rocket()
            .state::<Registry>()
            .and_then(|registry| registry.app_url.clone());
        if let Some(url) = configured {
            return Outcome::Success(AppBase(url));
        }
        let fallback = TrustedProxies::default();
        let trusted = request
            .rocket()
            .state::<Registry>()
            .map_or(&fallback, |registry| &registry.trusted_proxies);
        let socket = request.remote().map(|remote| remote.ip());
        let forwarded = request
            .headers()
            .get("X-Forwarded-Proto")
            .collect::<Vec<_>>()
            .join(",");
        let forwarded = (!forwarded.is_empty()).then_some(forwarded);
        let secure = proxy::request_is_secure(
            request.rocket().config().tls_enabled(),
            socket,
            forwarded.as_deref(),
            trusted,
        );
        let scheme = if secure { "https" } else { "http" };
        let host = request
            .host()
            .map(|host| host.to_string())
            .unwrap_or_default();
        Outcome::Success(AppBase(format!("{scheme}://{host}")))
    }
}

/// The address the throttle counts a forgot request against: its own prefix, so it never
/// shares a count with sign-in or the password change.
fn forgot_key(email: &str, ip: Option<IpAddr>) -> String {
    format!("reset|{}", throttle::key(email, ip))
}

/// The forgot page, with the message of the last request.
#[get("/password/forgot")]
fn forgot_form(cookies: &CookieJar<'_>, registry: &State<Registry>) -> PropelView {
    let flash = take_flash(cookies, &registry.secret);
    let token = csrf_token(cookies, &registry.secret);
    forgot_view(registry, flash, token, "", None)
}

fn forgot_view(
    registry: &Registry,
    flash: Option<String>,
    csrf_token: String,
    email: &str,
    error: Option<&str>,
) -> PropelView {
    let layout = Layout::new(registry, None, "", flash, csrf_token);
    let page = ForgotPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
        title: "Forgot password",
        action: format!("{}/password/forgot", registry.mount),
        login_url: format!("{}/login", registry.mount),
        email: email.to_string(),
        has_error: error.is_some(),
        error: error.unwrap_or_default().to_string(),
    };
    render_page("ignite::forgot", &page, compiled::forgot::render)
}

/// Answers a forgot request. Every request counts against the throttle. The answer is the
/// same whether or not the email has an account, so the form does not show who is registered.
#[post("/password/forgot", data = "<body>")]
async fn forgot_send(
    body: Data<'_>,
    client_ip: ClientIp,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    pool: &State<Pool>,
    base: AppBase,
) -> Result<Reply, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let email = form.get("email").unwrap_or("").trim().to_string();
    let key = forgot_key(&email, client_ip.0);

    // Refuse before any link is made while the key is at its limit.
    let check = key.clone();
    let waiting = blocking(pool, move |conn| {
        throttle::retry_after(conn, &check, throttle::LIMIT).map_err(db_status)
    })
    .await?;
    if let Some(seconds) = waiting {
        let message = format!("Too many requests. Try again in {seconds} seconds.");
        let page = forgot_view(
            registry,
            None,
            csrf_token(cookies, &registry.secret),
            &email,
            Some(&message),
        );
        return Ok(Reply::Throttled(seconds, page));
    }

    let lookup = email.clone();
    let issued = blocking(pool, move |conn| {
        throttle::record_failure(conn, &key).map_err(db_status)?;
        let user = users::find_by_email(conn, &lookup).map_err(db_status)?;
        match user.filter(|user| user.disabled_at.is_none()) {
            Some(_) => issue(conn, &lookup).map_err(db_status),
            None => {
                // Hash a value anyway, so an unknown email takes about as long as a known one.
                let _ = users::hash_password(&lookup);
                Ok(None)
            }
        }
    })
    .await?;

    if let Some(issued) = issued {
        let link = format!(
            "{}{}/password/reset/{}?email={}",
            base.0,
            registry.mount,
            issued.token,
            form_urlencoded::byte_serialize(email.as_bytes()).collect::<String>()
        );
        let message = reset_message(&email, &link, issued.idempotency_key);
        // A failed send is logged, not shown, so the answer does not change. The operator sees it in the log.
        if let Err(error) = registry.mailer.send(&message).await {
            rocket::error!("ignite: password reset email was not sent: {error}");
        }
    }

    set_flash(cookies, &registry.secret, SENT_MESSAGE);
    Ok(Reply::Redirect(Box::new(Redirect::to(format!(
        "{}/password/forgot",
        registry.mount
    )))))
}

/// The email that carries a reset link. It follows Laravel's reset notification.
fn reset_message(to: &str, link: &str, idempotency_key: String) -> Message {
    let text = format!(
        "Hello,\n\n\
         You are receiving this email because we received a password reset request for your account.\n\n\
         Reset your password: {link}\n\n\
         This password reset link will expire in {EXPIRE_MINUTES} minutes.\n\n\
         If you did not request a password reset, no further action is required.\n"
    );
    let html = format!(
        "<p>Hello,</p>\
         <p>You are receiving this email because we received a password reset request for your account.</p>\
         <p><a href=\"{link}\">Reset your password</a></p>\
         <p>This password reset link will expire in {EXPIRE_MINUTES} minutes.</p>\
         <p>If you did not request a password reset, no further action is required.</p>"
    );
    Message {
        to: to.to_string(),
        subject: "Reset Password Notification".to_string(),
        text,
        html,
        idempotency_key: Some(idempotency_key),
    }
}

/// The reset form, opened from the link. The token and the email come from the link.
#[get("/password/reset/<token>?<email>")]
fn reset_form(
    token: &str,
    email: Option<&str>,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
) -> PropelView {
    let flash = take_flash(cookies, &registry.secret);
    let csrf = csrf_token(cookies, &registry.secret);
    reset_view(registry, flash, csrf, token, email.unwrap_or(""), None)
}

fn reset_view(
    registry: &Registry,
    flash: Option<String>,
    csrf_token: String,
    token: &str,
    email: &str,
    error: Option<&str>,
) -> PropelView {
    let layout = Layout::new(registry, None, "", flash, csrf_token);
    let page = ResetPage {
        home_url: layout.home_url,
        signed_in: layout.signed_in,
        user_name: layout.user_name,
        user_initials: layout.user_initials,
        has_avatar: layout.has_avatar,
        avatar_url: layout.avatar_url,
        theme: layout.theme,
        theme_url: layout.theme_url,
        profile_url: layout.profile_url,
        logout_everywhere_url: layout.logout_everywhere_url,
        csrf_token: layout.csrf_token,
        nav: layout.nav,
        has_flash: layout.has_flash,
        flash: layout.flash,
        title: "Reset password",
        action: format!("{}/password/reset", registry.mount),
        token: token.to_string(),
        email: email.to_string(),
        has_error: error.is_some(),
        error: error.unwrap_or_default().to_string(),
    };
    render_page("ignite::reset", &page, compiled::reset::render)
}

/// Sets the new password, if the link is live and the password passes the rules. The link is
/// used up, and every session of the user ends, as a password change does.
#[post("/password/reset", data = "<body>")]
async fn reset_save(
    body: Data<'_>,
    cookies: &CookieJar<'_>,
    registry: &State<Registry>,
    pool: &State<Pool>,
) -> Result<Reply, Status> {
    let form = read_form(body).await?;
    check_csrf(cookies, &registry.secret, form.get("_token"))?;
    let email = form.get("email").unwrap_or("").trim().to_string();
    let token = form.get("token").unwrap_or("").to_string();
    let password = form.get("password").unwrap_or("").to_string();
    let confirm = form.get("password_confirmation").unwrap_or("").to_string();

    let rules = password_rules(&password, &confirm);
    if !rules.is_empty() {
        let message = rules.values().next().cloned().unwrap_or_default();
        let page = reset_view(
            registry,
            None,
            csrf_token(cookies, &registry.secret),
            &token,
            &email,
            Some(&message),
        );
        return Ok(Reply::Page(Status::UnprocessableEntity, page));
    }

    let lookup = email.clone();
    let reset = blocking(pool, move |conn| {
        let user = users::find_by_email(conn, &lookup).map_err(db_status)?;
        let Some(user) = user.filter(|user| user.disabled_at.is_none()) else {
            return Ok(false);
        };
        if !is_valid(conn, &lookup, &token).map_err(db_status)? {
            return Ok(false);
        }
        conn.transaction(|conn| {
            users::update_password(conn, user.id, &password)?;
            sessions::delete_for_user(conn, user.id)?;
            consume(conn, &lookup)?;
            Ok(true)
        })
        .map_err(db_status)
    })
    .await?;

    if !reset {
        let page = reset_view(
            registry,
            None,
            csrf_token(cookies, &registry.secret),
            "",
            &email,
            Some(INVALID_LINK),
        );
        return Ok(Reply::Page(Status::UnprocessableEntity, page));
    }

    set_flash(
        cookies,
        &registry.secret,
        "Your password has been reset. Sign in with the new password.",
    );
    Ok(Reply::Redirect(Box::new(Redirect::to(format!(
        "{}/login",
        registry.mount
    )))))
}

/// Checks the new password: at least [`MIN_PASSWORD_CHARS`] characters, and the same twice.
/// The same rules as the change-password form.
fn password_rules(password: &str, confirm: &str) -> FieldErrors {
    let mut errors = FieldErrors::new();
    if password.chars().count() < MIN_PASSWORD_CHARS {
        errors.insert(
            "password",
            format!("The password must be at least {MIN_PASSWORD_CHARS} characters."),
        );
    }
    if password != confirm {
        errors.insert(
            "password_confirmation",
            "The passwords do not match.".to_string(),
        );
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_rules_match_the_change_password_form() {
        assert!(password_rules("long enough", "long enough").is_empty());
        assert!(password_rules("short", "short").contains_key("password"));
        assert!(password_rules("long enough", "other one").contains_key("password_confirmation"));
    }

    #[test]
    fn each_link_has_its_own_idempotency_key() {
        let first = Utc::now().naive_utc();
        let second = first + Duration::seconds(120);
        assert_eq!(
            idempotency_key("a@example.test", first),
            idempotency_key("a@example.test", first)
        );
        assert_ne!(
            idempotency_key("a@example.test", first),
            idempotency_key("a@example.test", second)
        );
        assert!(!idempotency_key("a@example.test", first).contains("example.test"));
    }

    #[test]
    fn a_forgot_request_counts_under_its_own_prefix() {
        let ip: Option<IpAddr> = Some("203.0.113.9".parse().unwrap());
        assert!(forgot_key("ada@example.test", ip).starts_with("reset|"));
        assert_ne!(
            forgot_key("ada@example.test", ip),
            throttle::key("ada@example.test", ip)
        );
    }
}
