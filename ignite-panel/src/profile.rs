//! The profile: the fields every user has (name, email, password and picture), the
//! fields the app adds with [`Ignite::profile_field`](crate::Ignite::profile_field), the fields
//! whose values the app keeps itself ([`ProfileFields`], added with
//! [`Ignite::profile_fields`](crate::Ignite::profile_fields)), the app's own picture field
//! ([`ProfilePicture`], set with [`Ignite::profile_picture`](crate::Ignite::profile_picture)),
//! and their validation.

use std::collections::HashMap;
use std::sync::Arc;

use ignite_core::{
    hash_password, ActingUser, Database, Field, FieldDef, FieldErrors, FieldKind, FormInput, Row,
    RuntimeFuture, ValidationError,
};
use serde_json::{Map, Value};

/// Loads the values of a [`ProfileFields`] for the signed-in user, by field name.
type LoadFn = dyn for<'a> Fn(&'a Database, &'a ActingUser) -> RuntimeFuture<'a, Row> + Send + Sync;

/// Saves the validated values of a [`ProfileFields`] for the signed-in user.
type SaveFn = dyn for<'a> Fn(&'a Database, &'a ActingUser, Row) -> RuntimeFuture<'a, Result<(), ValidationError>>
    + Send
    + Sync;

/// The choices of a field of a [`ProfileFields`] that loads its options.
type OptionsFn = dyn for<'a> Fn(&'a Database, &'a ActingUser, &'a str) -> RuntimeFuture<'a, Vec<(String, String)>>
    + Send
    + Sync;

/// The URL of the signed-in user's picture, from the app ([`ProfilePicture::avatar_url`]).
type AvatarFn =
    dyn for<'a> Fn(&'a Database, &'a ActingUser) -> RuntimeFuture<'a, Option<String>> + Send + Sync;

/// Fields the app adds to the profile form whose values it keeps itself, such as a user's meta
/// rows, as the fields a Filament app adds to its `EditProfile` page's `form()` beside
/// `getNameFormComponent()` and `getEmailFormComponent()`. Add them with
/// [`Ignite::profile_fields`](crate::Ignite::profile_fields).
///
/// The fields render in the profile's "Your details" form after the name, the email and the
/// fields of [`Ignite::profile_field`](crate::Ignite::profile_field), with the same markup as
/// those (the panel's `partials.field`). [`load`](Self::load) gives their values for the
/// signed-in user each time the page shows. "Save details" checks them as a runtime resource's
/// fields are checked (see [`ignite_core::validate_fields_for_update`]), with the other fields,
/// and shows each message under its field with 422 and the typed values kept. When every field
/// passes, [`save`](Self::save) is given the validated values, then the name and the email are
/// saved, and the page says "Profile saved.". The save may refuse with field messages
/// ([`ValidationError`]), shown the same way.
///
/// ```ignore
/// ProfileFields::make()
///     .field(FieldDef { name: "first_name".into(), label: "First Name".into(),
///                       kind: FieldKindDef::Text, required: false })
///     .load(|db, user| Box::pin(async move { user_meta(db, user.id()).await }))
///     .save(|db, user, values| Box::pin(async move {
///         save_user_meta(db, user.id(), values).await?;
///         Ok(Ok(()))
///     }))
/// ```
#[derive(Clone)]
pub struct ProfileFields {
    fields: Vec<FieldDef>,
    load: Arc<LoadFn>,
    save: Arc<SaveFn>,
    options: Arc<OptionsFn>,
}

impl Default for ProfileFields {
    fn default() -> Self {
        ProfileFields {
            fields: Vec::new(),
            load: Arc::new(|_, _| Box::pin(async { Ok(Row::new()) })),
            save: Arc::new(|_, _, _| Box::pin(async { Ok(Ok(())) })),
            options: Arc::new(|_, _, _| Box::pin(async { Ok(Vec::new()) })),
        }
    }
}

impl std::fmt::Debug for ProfileFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileFields")
            .field("fields", &self.fields)
            .finish_non_exhaustive()
    }
}

impl ProfileFields {
    /// No fields yet, whose values load as none and save nowhere.
    pub fn make() -> Self {
        ProfileFields::default()
    }

    /// Adds a field, after the ones added before. Its `name` keys its value in
    /// [`load`](Self::load) and [`save`](Self::save).
    pub fn field(mut self, field: impl Into<FieldDef>) -> Self {
        self.fields.push(field.into());
        self
    }

    /// The values of the fields for the signed-in user, by field name, as a runtime resource's
    /// row: a string, a number, or `true` or `false` for a checkbox. A field without a value
    /// shows blank. The default is none.
    pub fn load<F>(mut self, load: F) -> Self
    where
        F: for<'a> Fn(&'a Database, &'a ActingUser) -> RuntimeFuture<'a, Row>
            + Send
            + Sync
            + 'static,
    {
        self.load = Arc::new(load);
        self
    }

    /// Saves the validated values for the signed-in user, as
    /// [`RuntimeData::try_update_for`](ignite_core::RuntimeData::try_update_for) is given them:
    /// a field the form did not send and a blank password are left out, an optional field sent
    /// blank is `null`, and a checkbox is `true` or `false`. Answer `Ok(Err(ValidationError))`
    /// to refuse with field messages; an error (`Err`) is a server error. The default saves
    /// nothing.
    pub fn save<F>(mut self, save: F) -> Self
    where
        F: for<'a> Fn(
                &'a Database,
                &'a ActingUser,
                Row,
            ) -> RuntimeFuture<'a, Result<(), ValidationError>>
            + Send
            + Sync
            + 'static,
    {
        self.save = Arc::new(save);
        self
    }

    /// The choices of a field whose kind loads its options (a lookup, post object,
    /// relationship, taxonomy or user field), by the field's name, as
    /// [`RuntimeData::options_for`](ignite_core::RuntimeData::options_for): such as the names a
    /// user can display publicly. A submitted value that is not a choice is refused with
    /// "Choose one of the options.". The default is no choices.
    pub fn options<F>(mut self, options: F) -> Self
    where
        F: for<'a> Fn(
                &'a Database,
                &'a ActingUser,
                &'a str,
            ) -> RuntimeFuture<'a, Vec<(String, String)>>
            + Send
            + Sync
            + 'static,
    {
        self.options = Arc::new(options);
        self
    }

    /// The fields, in display order.
    pub fn get_fields(&self) -> &[FieldDef] {
        &self.fields
    }

    pub(crate) async fn load_values(
        &self,
        db: &Database,
        user: &ActingUser,
    ) -> Result<Row, orbit_orm::Error> {
        (self.load)(db, user).await
    }

    pub(crate) async fn save_values(
        &self,
        db: &Database,
        user: &ActingUser,
        values: Row,
    ) -> Result<Result<(), ValidationError>, orbit_orm::Error> {
        (self.save)(db, user, values).await
    }

    pub(crate) async fn options_of(
        &self,
        db: &Database,
        user: &ActingUser,
        field: &str,
    ) -> Result<Vec<(String, String)>, orbit_orm::Error> {
        (self.options)(db, user, field).await
    }
}

/// The app's own picture field, which replaces the profile's picture upload, as a Filament app
/// puts its own avatar component (such as a `FileUpload`) in its `EditProfile` form. Set it
/// with [`Ignite::profile_picture`](crate::Ignite::profile_picture).
///
/// The profile's "Picture" card keeps the user's picture (or initials) and shows the field in
/// its own form, with the panel's field markup and a "Save picture" button, in place of the
/// upload and remove forms. The field is any field kind: a URL field to an attachment, or an
/// image field, which renders the media library's picker. Its value is loaded and saved through
/// [`load`](Self::load) and [`save`](Self::save), as for [`ProfileFields`]; a message shows
/// under the field with 422, and a save says "Picture saved.". The panel's own upload is the
/// default.
///
/// [`avatar_url`](Self::avatar_url) gives the URL of the user's picture, as Filament's
/// `HasAvatar::getFilamentAvatarUrl()`: the top bar and the card show it.
#[derive(Clone)]
pub struct ProfilePicture {
    fields: ProfileFields,
    avatar_url: Option<Arc<AvatarFn>>,
}

impl std::fmt::Debug for ProfilePicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfilePicture")
            .field("field", &self.fields.fields)
            .finish_non_exhaustive()
    }
}

impl ProfilePicture {
    /// The picture card with `field` in place of the upload.
    pub fn make(field: impl Into<FieldDef>) -> Self {
        ProfilePicture {
            fields: ProfileFields::make().field(field),
            avatar_url: None,
        }
    }

    /// The field's value for the signed-in user, keyed by the field's name. See
    /// [`ProfileFields::load`].
    pub fn load<F>(mut self, load: F) -> Self
    where
        F: for<'a> Fn(&'a Database, &'a ActingUser) -> RuntimeFuture<'a, Row>
            + Send
            + Sync
            + 'static,
    {
        self.fields = self.fields.load(load);
        self
    }

    /// Saves the field's validated value. See [`ProfileFields::save`].
    pub fn save<F>(mut self, save: F) -> Self
    where
        F: for<'a> Fn(
                &'a Database,
                &'a ActingUser,
                Row,
            ) -> RuntimeFuture<'a, Result<(), ValidationError>>
            + Send
            + Sync
            + 'static,
    {
        self.fields = self.fields.save(save);
        self
    }

    /// The choices of a field that loads its options. See [`ProfileFields::options`].
    pub fn options<F>(mut self, options: F) -> Self
    where
        F: for<'a> Fn(
                &'a Database,
                &'a ActingUser,
                &'a str,
            ) -> RuntimeFuture<'a, Vec<(String, String)>>
            + Send
            + Sync
            + 'static,
    {
        self.fields = self.fields.options(options);
        self
    }

    /// The URL of the signed-in user's picture, or `None` for none, as Filament's
    /// `HasAvatar::getFilamentAvatarUrl()`. It is asked once per signed-in request; the top bar
    /// and the profile's picture card show it, and without one they show the picture the user
    /// uploaded before, or the initials. A failed call is logged and shows no picture.
    pub fn avatar_url<F>(mut self, avatar_url: F) -> Self
    where
        F: for<'a> Fn(&'a Database, &'a ActingUser) -> RuntimeFuture<'a, Option<String>>
            + Send
            + Sync
            + 'static,
    {
        self.avatar_url = Some(Arc::new(avatar_url));
        self
    }

    /// The picture's field.
    pub fn get_field(&self) -> &FieldDef {
        &self.fields.fields[0]
    }

    pub(crate) fn fields(&self) -> &ProfileFields {
        &self.fields
    }

    /// The URL of `user`'s picture, from [`avatar_url`](Self::avatar_url), if the app set it.
    pub(crate) async fn avatar_url_for(
        &self,
        db: &Database,
        user: &ActingUser,
    ) -> Option<Result<Option<String>, orbit_orm::Error>> {
        match &self.avatar_url {
            Some(avatar_url) => Some(avatar_url(db, user).await),
            None => None,
        }
    }
}

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

/// The app's stored profile values, as the JSON object the profile keeps. Empty when unset.
fn stored_map(custom: Option<&str>) -> Map<String, Value> {
    match custom.and_then(|json| serde_json::from_str(json).ok()) {
        Some(Value::Object(stored)) => stored,
        _ => Map::new(),
    }
}

/// The values a profile form starts with: the saved name and email, and the app's
/// fields as stored. A checkbox that is set is stored as `true`. A password field is
/// left out: its stored value is a hash, which the form never shows.
pub(crate) fn form_values(
    fields: &[Field],
    name: &str,
    email: &str,
    custom: Option<&str>,
) -> HashMap<String, String> {
    let mut values = HashMap::new();
    values.insert("name".to_string(), name.to_string());
    values.insert("email".to_string(), email.to_string());
    for (key, value) in stored_map(custom) {
        let is_password = fields
            .iter()
            .any(|f| f.name == key && f.kind == FieldKind::Password);
        if is_password {
            continue;
        }
        let text = match value {
            Value::Bool(true) => "on".to_string(),
            Value::Bool(false) | Value::Null => continue,
            Value::String(text) => text,
            other => other.to_string(),
        };
        values.insert(key, text);
    }
    values
}

/// The text values a form starts with for `row`, the values of `fields` an app loaded: a
/// checkbox that is set is `on`, a blank value and a password are left out.
pub(crate) fn row_values(fields: &[FieldDef], row: &Row) -> HashMap<String, String> {
    let form = crate::views::record_form(fields, &Value::Object(row.clone()));
    fields
        .iter()
        .filter_map(|f| form.get(&f.name).map(|v| (f.name.clone(), v.to_string())))
        .collect()
}

/// The profile values that passed validation, ready to store.
pub(crate) struct Saved {
    pub(crate) name: String,
    pub(crate) email: String,
    /// The app's fields as a JSON object, keyed by field name.
    pub(crate) custom: String,
}

/// Checks a submitted profile form. Each field is read as its kind says: required
/// fields must not be blank, a number must be a number (whole or decimal) and a date
/// must be `YYYY-MM-DD` (checked with the same helpers as a resource's fields), a select
/// must be one of its choices, and the email must look like an address. A password is
/// hashed before it is stored. A blank password keeps the hash already stored in
/// `stored`, the profile JSON the user has now. Returns the values to store, or the
/// error of each field.
pub(crate) fn validate(
    fields: &[Field],
    input: &FormInput,
    stored: Option<&str>,
) -> Result<Saved, FieldErrors> {
    let mut errors = FieldErrors::new();
    let stored = stored_map(stored);
    let mut saved = Saved {
        name: String::new(),
        email: String::new(),
        custom: String::new(),
    };
    let mut custom = Map::new();
    for field in fields {
        match field.kind {
            FieldKind::Checkbox => {
                custom.insert(
                    field.name.to_string(),
                    Value::Bool(input.checkbox(field.name)),
                );
                continue;
            }
            FieldKind::Password => {
                if let Some(hash) = password_to_store(field, input, &stored, &mut errors) {
                    custom.insert(field.name.to_string(), Value::String(hash));
                }
                continue;
            }
            _ => {}
        }

        let mut text = if field.required {
            field.check_required(input, &mut errors).unwrap_or("")
        } else {
            input.get(field.name).unwrap_or("").trim()
        }
        .to_string();
        if !text.is_empty() {
            match field.kind {
                FieldKind::Number | FieldKind::Decimal(_) => {
                    field.check_number(&text, &mut errors);
                }
                FieldKind::Date => {
                    field.check_date(&text, &mut errors);
                }
                FieldKind::DateTime => {
                    field.check_datetime(&text, &mut errors);
                }
                FieldKind::Select(choices)
                    if !choices.iter().any(|(choice, _)| *choice == text) =>
                {
                    errors.insert(
                        field.name.to_string(),
                        "Choose one of the options.".to_string(),
                    );
                }
                // The kinds added for custom fields (email, URL, color, repeater, ...) store
                // the text their check returns.
                _ => {
                    if let Some(value) = field.check_value(&text, &mut errors) {
                        text = value;
                    }
                }
            }
        }
        match field.name {
            "name" => saved.name = text,
            "email" => {
                if !text.is_empty() && !text.contains('@') {
                    errors.insert(
                        field.name.to_string(),
                        "Enter an email address.".to_string(),
                    );
                }
                saved.email = text;
            }
            _ => {
                custom.insert(field.name.to_string(), Value::String(text));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    saved.custom = Value::Object(custom).to_string();
    Ok(saved)
}

/// The value a password field stores. A new password is hashed. A blank one keeps the
/// hash in `stored`. With no hash stored, a required field is reported as missing.
fn password_to_store(
    field: &Field,
    input: &FormInput,
    stored: &Map<String, Value>,
    errors: &mut FieldErrors,
) -> Option<String> {
    if let Some(password) = input.get(field.name) {
        return match hash_password(password) {
            Ok(hash) => Some(hash),
            Err(_) => {
                errors.insert(
                    field.name.to_string(),
                    "Could not save this password.".to_string(),
                );
                None
            }
        };
    }
    match stored.get(field.name) {
        Some(Value::String(hash)) => Some(hash.clone()),
        _ => {
            if field.required {
                field.check_required(input, errors);
            }
            None
        }
    }
}

/// Checks the password form. `current_ok` says whether the current password matched.
pub(crate) fn password_errors(current_ok: bool, new: &str, confirm: &str) -> FieldErrors {
    let mut errors = FieldErrors::new();
    if !current_ok {
        errors.insert(
            "current_password".to_string(),
            "The current password is incorrect.".to_string(),
        );
    }
    if new.chars().count() < MIN_PASSWORD_CHARS {
        errors.insert(
            "new_password".to_string(),
            format!("The new password must be at least {MIN_PASSWORD_CHARS} characters."),
        );
    }
    if new != confirm {
        errors.insert(
            "confirm_password".to_string(),
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

/// The file extension of a picture of type `mime`, one of the types [`picture_type`] accepts.
pub(crate) fn picture_extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_accepted_picture_type_has_an_extension() {
        assert_eq!(picture_extension("image/png"), Some("png"));
        assert_eq!(picture_extension("image/jpeg"), Some("jpg"));
        assert_eq!(picture_extension("image/webp"), Some("webp"));
        assert_eq!(picture_extension("image/gif"), None);
    }

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
