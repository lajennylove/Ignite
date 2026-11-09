//! CSRF protection for an app's own routes, as Laravel's `VerifyCsrfToken` gives
//! the app's POST, PUT, PATCH and DELETE routes.
//!
//! A fairing cannot read a request body in Rocket, and a form's token travels in
//! the body. So the check is a data guard that an app's route takes instead of
//! `Form` or `Data`: [`CsrfForm`]. It reads the token from the `_token` field
//! (or the `X-CSRF-TOKEN` header, as Laravel accepts), compares it with the
//! browser session's token, and answers 419 when they do not match. The
//! exempt list from [`Ignite::csrf_exempt`](crate::Ignite::csrf_exempt) skips the
//! check for the paths it names.

use std::collections::HashMap;

use rocket::data::{self, Data, FromData, ToByteUnit};
use rocket::http::{CookieJar, Status};
use rocket::request::{FromRequest, Outcome, Request};

use crate::ignite::Registry;
use crate::session::{check_csrf, csrf_token};
use ignite_core::FormInput;

/// The largest form body the guard reads.
const MAX_BODY_BYTES: u64 = 64 * 1024;

/// The CSRF token of the browser session. Put it in a view's context and write
/// it into each form as a hidden `_token` input:
///
/// ```ignore
/// #[get("/checkout")]
/// fn checkout(token: CsrfToken) -> PropelView { /* context: csrf_token: token.value() */ }
/// ```
///
/// ```text
/// <input type="hidden" name="_token" value="{{ $csrf_token }}">
/// ```
#[derive(Debug, Clone)]
pub struct CsrfToken(String);

impl CsrfToken {
    /// The token to write into a form.
    pub fn value(&self) -> &str {
        &self.0
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for CsrfToken {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        let Some(registry) = request.rocket().state::<Registry>() else {
            return Outcome::Error((Status::InternalServerError, ()));
        };
        let cookies: &CookieJar<'_> = request.cookies();
        Outcome::Success(CsrfToken(csrf_token(cookies, &registry.secret)))
    }
}

/// The form of a request that passed the CSRF check. Use it as the data guard of
/// a POST, PUT, PATCH or DELETE route. It reads the same fields as `Form`:
///
/// ```ignore
/// #[post("/orders", data = "<form>")]
/// fn order(form: CsrfForm) -> String { form.get("item").unwrap_or_default().to_string() }
/// ```
#[derive(Debug, Clone)]
pub struct CsrfForm(FormInput);

impl CsrfForm {
    /// The submitted value of `name`, or `None` when it is missing or blank.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name)
    }

    /// The submitted form, for `Field::check_required` and `validate`.
    pub fn input(&self) -> &FormInput {
        &self.0
    }
}

#[rocket::async_trait]
impl<'r> FromData<'r> for CsrfForm {
    type Error = ();

    async fn from_data(request: &'r Request<'_>, data: Data<'r>) -> data::Outcome<'r, Self> {
        let Some(registry) = request.rocket().state::<Registry>() else {
            return data::Outcome::Error((Status::InternalServerError, ()));
        };
        let path = request.uri().path().as_str();
        let exempt = registry
            .csrf_exempt
            .iter()
            .any(|entry| path_matches(entry, path));

        let body = match data.open(MAX_BODY_BYTES.bytes()).into_string().await {
            Ok(body) if body.is_complete() => body.into_inner(),
            Ok(_) => return data::Outcome::Error((Status::PayloadTooLarge, ())),
            Err(_) => return data::Outcome::Error((Status::BadRequest, ())),
        };
        let values: HashMap<String, String> = form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect();
        let form = FormInput::new(values);

        if !exempt {
            let submitted = form
                .get("_token")
                .or_else(|| request.headers().get_one("X-CSRF-TOKEN"));
            if check_csrf(request.cookies(), &registry.secret, submitted).is_err() {
                return data::Outcome::Error((Status::new(419), ()));
            }
        }
        data::Outcome::Success(CsrfForm(form))
    }
}

/// True when `path` is `entry`, or `entry` ends with `/*` and `path` is under it.
fn path_matches(entry: &str, path: &str) -> bool {
    match entry.strip_suffix("/*") {
        Some(prefix) => path == prefix || path.starts_with(&format!("{prefix}/")),
        None => entry == path,
    }
}

#[cfg(test)]
mod tests {
    use super::path_matches;

    #[test]
    fn an_exact_entry_matches_only_its_path() {
        assert!(path_matches("/hooks/stripe", "/hooks/stripe"));
        assert!(!path_matches("/hooks/stripe", "/hooks/stripe/extra"));
    }

    #[test]
    fn a_wildcard_entry_matches_everything_under_it() {
        assert!(path_matches("/hooks/*", "/hooks"));
        assert!(path_matches("/hooks/*", "/hooks/stripe"));
        assert!(!path_matches("/hooks/*", "/hooksuffix"));
    }
}
