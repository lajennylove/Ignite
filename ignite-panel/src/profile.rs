//! The profile: the fields every user has (name, email, password and picture), the
//! fields the app adds with [`Ignite::profile_field`](crate::Ignite::profile_field), and
//! their validation.

use std::collections::HashMap;

use ignite_core::{Field, FieldErrors, FieldKind, FormInput};
use serde_json::{Map, Value};

use crate::ignite::Registry;

/// The largest picture the panel accepts, in bytes. Rocket's default upload limit is 1 MiB.
pub(crate) const MAX_AVATAR_BYTES: usize = 1024 * 1024;

/// The shortest new password. The same minimum as the CLI's.
pub(crate) const MIN_PASSWORD_CHARS: usize = 8;

const NAME: Field = Field {
    name: "name",
    label: "Name",
    kind: FieldKind::Text,
    required: true,
};

const EMAIL: Field = Field {
    name: "email",
    label: "Email",
    kind: FieldKind::Text,
    required: true,
};

/// The password form: the current password, then the new one twice.
pub(crate) const PASSWORD_FIELDS: [Field; 3] = [
    Field {
        name: "current_password",
        label: "Current password",
        kind: FieldKind::Password,
        required: true,
    },
    Field {
        name: "new_password",
        label: "New password",
        kind: FieldKind::Password,
        required: true,
    },
    Field {
        name: "confirm_password",
        label: "Confirm new password",
        kind: FieldKind::Password,
        required: true,
    },
];

/// The fields of the profile form: the name, the email, then the app's own fields.
pub(crate) fn profile_fields(registry: &Registry) -> Vec<Field> {
    [NAME, EMAIL]
        .into_iter()
        .chain(registry.profile_fields.iter().copied())
        .collect()
}

/// The values a profile form starts with: the saved name and email, and the app's
/// fields as stored. A checkbox that is set is stored as `true`.
pub(crate) fn form_values(name: &str, email: &str, custom: Option<&str>) -> FormInput {
    let mut values = HashMap::new();
    values.insert("name".to_string(), name.to_string());
    values.insert("email".to_string(), email.to_string());
    if let Some(Value::Object(stored)) = custom.and_then(|json| serde_json::from_str(json).ok()) {
        for (key, value) in stored {
            let text = match value {
                Value::Bool(true) => "on".to_string(),
                Value::Bool(false) | Value::Null => continue,
                Value::String(text) => text,
                other => other.to_string(),
            };
            values.insert(key, text);
        }
    }
    FormInput::new(values)
}

/// The profile values that passed validation, ready to store.
pub(crate) struct Saved {
    pub(crate) name: String,
    pub(crate) email: String,
    /// The app's fields as a JSON object, keyed by field name.
    pub(crate) custom: String,
}

/// Checks a submitted profile form. Each field is read as its kind says, required
/// fields must not be blank, a select must be one of its choices, and the email must
/// look like an address. Returns the values to store, or the error of each field.
pub(crate) fn validate(fields: &[Field], input: &FormInput) -> Result<Saved, FieldErrors> {
    let mut errors = FieldErrors::new();
    let mut saved = Saved {
        name: String::new(),
        email: String::new(),
        custom: String::new(),
    };
    let mut custom = Map::new();
    for field in fields {
        if field.kind == FieldKind::Checkbox {
            custom.insert(
                field.name.to_string(),
                Value::Bool(input.checkbox(field.name)),
            );
            continue;
        }
        let value = input.get(field.name).unwrap_or("").trim().to_string();
        if field.required && value.is_empty() {
            errors.insert(field.name, format!("{} is required", field.label));
        }
        if let FieldKind::Select(choices) = field.kind {
            if !value.is_empty() && !choices.iter().any(|(choice, _)| *choice == value) {
                errors.insert(field.name, "Choose one of the options.".to_string());
            }
        }
        match field.name {
            "name" => saved.name = value,
            "email" => {
                if !value.is_empty() && !value.contains('@') {
                    errors.insert(field.name, "Enter an email address.".to_string());
                }
                saved.email = value;
            }
            _ => {
                custom.insert(field.name.to_string(), Value::String(value));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    saved.custom = Value::Object(custom).to_string();
    Ok(saved)
}

/// Checks the password form. `current_ok` says whether the current password matched.
pub(crate) fn password_errors(current_ok: bool, new: &str, confirm: &str) -> FieldErrors {
    let mut errors = FieldErrors::new();
    if !current_ok {
        errors.insert(
            "current_password",
            "The current password is incorrect.".to_string(),
        );
    }
    if new.chars().count() < MIN_PASSWORD_CHARS {
        errors.insert(
            "new_password",
            format!("The new password must be at least {MIN_PASSWORD_CHARS} characters."),
        );
    }
    if new != confirm {
        errors.insert(
            "confirm_password",
            "The passwords do not match.".to_string(),
        );
    }
    errors
}

/// The type of a picture, read from its first bytes. Only PNG, JPEG and WebP are
/// accepted; anything else is `None`, whatever its file name says.
pub(crate) fn picture_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_recognised_by_its_bytes() {
        assert_eq!(picture_type(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(picture_type(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(picture_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(picture_type(b"GIF89a"), None);
        assert_eq!(picture_type(b"<svg>"), None);
    }

    #[test]
    fn the_password_rules_are_checked_together() {
        let errors = password_errors(false, "short", "other");
        assert!(errors.contains_key("current_password"));
        assert!(errors.contains_key("new_password"));
        assert!(errors.contains_key("confirm_password"));
        assert!(password_errors(true, "long enough", "long enough").is_empty());
    }
}
