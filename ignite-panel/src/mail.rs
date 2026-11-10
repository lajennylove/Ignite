//! Sending email, behind a trait, as Laravel's `Mail` facade sits over its mailers. The
//! panel's password reset sends through a [`Mailer`], so an app picks the provider: the
//! [`EmailitMailer`] here, or another provider written as one more implementation. A
//! [`RecordingMailer`] keeps messages in memory for tests, and the default [`NotConfigured`]
//! refuses to send, so an app that sets no mailer never sends by accident.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rocket::async_trait;
use serde_json::{json, Value};

/// The Emailit API's send endpoint.
const EMAILIT_URL: &str = "https://api.emailit.com/v2/emails";

/// The environment variable that holds the Emailit API key. Never logged.
pub const EMAILIT_API_KEY_ENV: &str = "EMAILIT_API_KEY";

/// The environment variable that holds the sender, such as `Ignite <hello@mail.example.test>`.
pub const MAIL_FROM_ENV: &str = "IGNITE_MAIL_FROM";

/// How long one send may take before it fails.
const SEND_TIMEOUT: Duration = Duration::from_secs(15);

/// One message to one recipient. The sender is set by the mailer, not by the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The recipient's address.
    pub to: String,
    /// The subject line.
    pub subject: String,
    /// The plain-text body.
    pub text: String,
    /// The HTML body.
    pub html: String,
    /// A key the provider uses to drop a repeat of the same send. Providers that have no
    /// such header ignore it.
    pub idempotency_key: Option<String>,
}

/// Why a send failed. The messages never contain the API key.
#[derive(Debug)]
pub enum MailError {
    /// No mailer is configured, so nothing was sent. See [`NotConfigured`].
    NotConfigured,
    /// 401 or 403: the API key is invalid, or the sending domain is not verified for it.
    InvalidKey(u16),
    /// 402: the account has no credits left.
    InsufficientCredits,
    /// 422: the provider refused the message. The text is the provider's message, if it gave one.
    Validation(String),
    /// 429: too many sends. `retry_after` is the number of seconds to wait, if the provider said.
    RateLimited { retry_after: Option<u64> },
    /// 404: a template the message names does not exist.
    TemplateNotFound,
    /// Any other status the provider answered with.
    Rejected(u16),
    /// The request did not complete: no connection, a timeout, or an unreadable response.
    Transport(String),
}

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailError::NotConfigured => {
                write!(f, "no mailer is configured, so the email was not sent")
            }
            MailError::InvalidKey(status) => write!(
                f,
                "the mail provider refused the API key or the sending domain ({status})"
            ),
            MailError::InsufficientCredits => {
                write!(f, "the mail provider account has no credits left (402)")
            }
            MailError::Validation(message) if message.is_empty() => {
                write!(f, "the mail provider refused the message (422)")
            }
            MailError::Validation(message) => {
                write!(f, "the mail provider refused the message (422): {message}")
            }
            MailError::RateLimited {
                retry_after: Some(seconds),
            } => write!(
                f,
                "the mail provider rate limit is reached; retry in {seconds} seconds"
            ),
            MailError::RateLimited { retry_after: None } => {
                write!(f, "the mail provider rate limit is reached")
            }
            MailError::TemplateNotFound => {
                write!(f, "the mail provider template was not found (404)")
            }
            MailError::Rejected(status) => {
                write!(f, "the mail provider answered with status {status}")
            }
            MailError::Transport(message) => write!(f, "the email could not be sent: {message}"),
        }
    }
}

impl std::error::Error for MailError {}

/// A provider that sends [`Message`]s. Implement it to add a provider, such as Postmark or
/// Mailgun; the password reset only sees this trait.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Sends `message`. Returns once the provider has accepted it.
    async fn send(&self, message: &Message) -> Result<(), MailError>;
}

/// The mailer an app has when it sets none. It refuses every send and says so, so a missing
/// `Ignite::mailer` call shows up as a logged error, not as a silent drop.
#[derive(Debug, Clone, Copy, Default)]
pub struct NotConfigured;

#[async_trait]
impl Mailer for NotConfigured {
    async fn send(&self, _message: &Message) -> Result<(), MailError> {
        Err(MailError::NotConfigured)
    }
}

/// A mailer that keeps each message in memory instead of sending it. Clones share the same
/// list, so a test keeps one clone and gives another to the app.
#[derive(Debug, Clone, Default)]
pub struct RecordingMailer {
    sent: Arc<Mutex<Vec<Message>>>,
}

impl RecordingMailer {
    /// An empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every message sent so far, in order.
    pub fn sent(&self) -> Vec<Message> {
        self.sent.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[async_trait]
impl Mailer for RecordingMailer {
    async fn send(&self, message: &Message) -> Result<(), MailError> {
        self.sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(message.clone());
        Ok(())
    }
}

/// Sends through the Emailit API (`https://api.emailit.com/v2/emails`).
///
/// The key and the sender are read by [`from_env`](Self::from_env) at app start. The key is
/// never printed: the `Debug` output and every error leave it out.
#[derive(Clone)]
pub struct EmailitMailer {
    client: reqwest::Client,
    api_key: String,
    from: String,
}

impl EmailitMailer {
    /// A mailer with `api_key`, sending as `from`, such as `Ignite <hello@mail.example.test>`.
    ///
    /// # Panics
    ///
    /// Panics if the HTTP client cannot be built, which only happens when TLS cannot start.
    pub fn new(api_key: impl Into<String>, from: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(SEND_TIMEOUT)
            .build()
            .expect("Ignite could not start the mail client");
        EmailitMailer {
            client,
            api_key: api_key.into(),
            from: from.into(),
        }
    }

    /// Reads `EMAILIT_API_KEY` and `IGNITE_MAIL_FROM` from the environment, after loading the
    /// `.env` file the way [`Ignite::new`](crate::Ignite::new) does.
    ///
    /// # Panics
    ///
    /// Panics when either variable is missing, so the app stops at start, as it does for
    /// `DATABASE_URL`. Call this only in an app that sends mail.
    pub fn from_env() -> Self {
        crate::load_env();
        let key = std::env::var(EMAILIT_API_KEY_ENV)
            .unwrap_or_else(|_| panic!("{EMAILIT_API_KEY_ENV} must be set to send mail"));
        let from = std::env::var(MAIL_FROM_ENV)
            .unwrap_or_else(|_| panic!("{MAIL_FROM_ENV} must be set to send mail"));
        Self::new(key, from)
    }
}

impl fmt::Debug for EmailitMailer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmailitMailer")
            .field("from", &self.from)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[async_trait]
impl Mailer for EmailitMailer {
    async fn send(&self, message: &Message) -> Result<(), MailError> {
        let body = json!({
            "from": self.from,
            "to": [message.to],
            "subject": message.subject,
            "html": message.html,
            "text": message.text,
        });
        let mut request = self
            .client
            .post(EMAILIT_URL)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .body(body.to_string());
        if let Some(key) = &message.idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        let response = request
            .send()
            .await
            .map_err(|error| MailError::Transport(transport_message(&error)))?;

        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let retry_header = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());
        let text = response.text().await.unwrap_or_default();
        Err(error_for_status(status.as_u16(), retry_header, &text))
    }
}

/// Maps a non-success status to its error. `body` is the provider's answer; only its
/// `message` and `retry_after` fields are read, so nothing else from it is echoed.
fn error_for_status(status: u16, retry_header: Option<u64>, body: &str) -> MailError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match status {
        401 | 403 => MailError::InvalidKey(status),
        402 => MailError::InsufficientCredits,
        404 => MailError::TemplateNotFound,
        422 => MailError::Validation(message),
        429 => {
            let body_retry = parsed
                .as_ref()
                .and_then(|value| value.get("retry_after"))
                .and_then(Value::as_u64);
            MailError::RateLimited {
                retry_after: retry_header.or(body_retry),
            }
        }
        other => MailError::Rejected(other),
    }
}

/// The reason a request did not complete. `reqwest` errors can carry the URL, which holds
/// no secret, so the text is safe to show.
fn transport_message(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the mail provider did not answer in time".to_string()
    } else if error.is_connect() {
        "the mail provider could not be reached".to_string()
    } else {
        "the mail provider request failed".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_status_maps_to_its_error() {
        assert!(matches!(
            error_for_status(401, None, ""),
            MailError::InvalidKey(401)
        ));
        assert!(matches!(
            error_for_status(403, None, ""),
            MailError::InvalidKey(403)
        ));
        assert!(matches!(
            error_for_status(402, None, ""),
            MailError::InsufficientCredits
        ));
        assert!(matches!(
            error_for_status(404, None, ""),
            MailError::TemplateNotFound
        ));
        assert!(matches!(
            error_for_status(500, None, ""),
            MailError::Rejected(500)
        ));
    }

    #[test]
    fn a_validation_error_keeps_the_providers_message_only() {
        let error = error_for_status(422, None, r#"{"message":"bad to","secret":"x"}"#);
        match error {
            MailError::Validation(message) => assert_eq!(message, "bad to"),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_rate_limit_reads_retry_after_from_the_header_or_the_body() {
        assert!(matches!(
            error_for_status(429, Some(12), r#"{"retry_after":3}"#),
            MailError::RateLimited {
                retry_after: Some(12)
            }
        ));
        assert!(matches!(
            error_for_status(429, None, r#"{"retry_after":3}"#),
            MailError::RateLimited {
                retry_after: Some(3)
            }
        ));
        assert!(matches!(
            error_for_status(429, None, "not json"),
            MailError::RateLimited { retry_after: None }
        ));
    }

    #[test]
    fn the_debug_output_never_shows_the_key() {
        let mailer = EmailitMailer::new("em_live_secret_value", "Ignite <hi@example.test>");
        let shown = format!("{mailer:?}");
        assert!(!shown.contains("em_live_secret_value"));
        assert!(shown.contains("hi@example.test"));
    }

    #[test]
    fn the_error_text_never_shows_the_key() {
        let error = MailError::InvalidKey(401);
        assert!(!error.to_string().contains("em_live"));
    }

    #[test]
    fn a_recording_mailer_keeps_what_it_is_given() {
        rocket::async_test(async {
            let recorder = RecordingMailer::new();
            let clone = recorder.clone();
            let message = Message {
                to: "ada@example.test".to_string(),
                subject: "Hi".to_string(),
                text: "text".to_string(),
                html: "<p>html</p>".to_string(),
                idempotency_key: None,
            };
            recorder.send(&message).await.unwrap();
            assert_eq!(clone.sent(), vec![message]);
        });
    }

    #[test]
    fn the_default_mailer_refuses_to_send() {
        rocket::async_test(async {
            let message = Message {
                to: "ada@example.test".to_string(),
                subject: String::new(),
                text: String::new(),
                html: String::new(),
                idempotency_key: None,
            };
            assert!(matches!(
                NotConfigured.send(&message).await,
                Err(MailError::NotConfigured)
            ));
        });
    }
}
