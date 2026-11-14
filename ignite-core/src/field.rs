//! Form fields and table columns, as the panel renders them.

use std::str::FromStr;

use chrono::{NaiveDate, NaiveDateTime};

use crate::form::{FieldErrors, FormInput};

/// The kind of input a form field renders as, and how its value is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A single line of text.
    Text,
    /// Several lines of text.
    Textarea,
    /// A password. The form never shows the stored value.
    Password,
    /// A whole number, submitted as text. A resource's integer field is checked as a whole
    /// number. A profile field accepts decimals as well (see [`Field::check_number`]).
    Number,
    /// A decimal number, submitted as text, such as `9.5`. The string is the browser's
    /// step for the input: `"any"` accepts every value, and `"0.01"` steps by cents.
    Decimal(&'static str),
    /// A yes/no value. Read it with [`FormInput::checkbox`](crate::FormInput::checkbox).
    Checkbox,
    /// One choice from a list of `(value, label)` pairs.
    Select(&'static [(&'static str, &'static str)]),
    /// One choice from options that the resource loads from the database each time its
    /// form is shown, such as the rows of a related table. See
    /// [`IgniteResource::options`](crate::IgniteResource::options).
    Lookup,
    /// A calendar date, as `YYYY-MM-DD`.
    Date,
    /// A date and time, as `YYYY-MM-DDTHH:MM`, with seconds when given.
    DateTime,
    /// An email address, as Filament's `TextInput::make()->email()`. The value must be a
    /// valid address, as the browser's `type="email"` input defines one (see
    /// [`Field::check_email`]).
    Email,
    /// A URL, as Filament's `TextInput::make()->url()`. The value must parse as an absolute
    /// URL (see [`Field::check_url`]).
    Url,
    /// One choice from a list of `(value, label)` pairs, shown as radio buttons, as
    /// Filament's `Radio`. The value is checked as a [`Select`](Self::Select) is.
    Radio(&'static [(&'static str, &'static str)]),
    /// A color, as Filament's `ColorPicker`. The value is `#rrggbb`, stored in lowercase
    /// (see [`Field::check_color`]).
    Color,
    /// An image from the media library, as Filament's `FileUpload::make()->image()`. The
    /// value is the item's id, a positive whole number, stored as text. The panel shows the
    /// item through the app's [`MediaLibrary`](crate::MediaLibrary), when one is registered.
    Image,
    /// A file from the media library, as Filament's `FileUpload`. Stored as
    /// [`Image`](Self::Image) is.
    File,
    /// A link (ACF's link field): the JSON text `{"url": ..., "title": ..., "target": ...}`.
    /// The URL is required and must parse; `target` is `""` or `"_blank"` (see
    /// [`Field::check_link`]).
    Link,
    /// Rich text, as Filament's `RichEditor`. The panel renders a textarea marked with
    /// `data-ignite-editor`, and loads the editor script the app registers with
    /// [`register_editor`](crate::register_editor). Ignite bundles no editor. The value is
    /// the submitted HTML, unchanged.
    Wysiwyg,
    /// One post, chosen from options loaded each time the form is shown, as a
    /// [`Lookup`](Self::Lookup) is (ACF's post object field).
    PostObject,
    /// Several choices from options loaded each time the form is shown (ACF's relationship
    /// field): the multi-value [`PostObject`](Self::PostObject). The value is a JSON array
    /// of the chosen option values, as strings, such as `["12","40"]`.
    Relationship,
    /// One taxonomy term, chosen from loaded options, as a [`Lookup`](Self::Lookup) is.
    Taxonomy,
    /// One user, chosen from loaded options, as a [`Lookup`](Self::Lookup) is.
    User,
    /// Rows of the given sub-fields, as Filament's `Repeater`. The value is a JSON array
    /// with one object per row, keyed by sub-field name, such as
    /// `[{"title": "One"}, {"title": "Two"}]`. Each row is checked against the sub-fields.
    Repeater(&'static [Field]),
    /// The given sub-fields, kept together (ACF's group field). The value is one JSON
    /// object keyed by sub-field name.
    Group(&'static [Field]),
    /// Rows that each take one of the given blocks, as Filament's `Builder`. The value is a
    /// JSON array of `{"type": <block name>, "data": {<sub-field values>}}` objects, as
    /// Filament stores a builder.
    FlexibleContent(&'static [Block]),
    /// Several images from the media library (ACF's gallery field). The value is a JSON
    /// array of item ids, such as `[3,8]`.
    Gallery,
    /// The fields of another field group, reused (ACF's clone field). Ignite has no field
    /// groups of its own: the app resolves the group it clones and passes its fields here.
    /// The value is stored and checked as a [`Group`](Self::Group) of those fields is.
    Clone(&'static [Field]),
}

impl FieldKind {
    /// Whether the field's choices are loaded each time its form is shown, through
    /// [`IgniteResource::options`](crate::IgniteResource::options) or
    /// [`RuntimeData::options`](crate::RuntimeData::options): a lookup, post object,
    /// relationship, taxonomy or user field.
    pub fn loads_options(&self) -> bool {
        matches!(
            self,
            FieldKind::Lookup
                | FieldKind::PostObject
                | FieldKind::Relationship
                | FieldKind::Taxonomy
                | FieldKind::User
        )
    }
}

/// One block of a [`FieldKind::FlexibleContent`] field, as Filament's `Builder\Block` (an
/// ACF flexible content layout): a name, a label, and its sub-fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// The block's name, stored as the `type` of each row that uses it.
    pub name: &'static str,
    /// The label shown when choosing the block.
    pub label: &'static str,
    /// The block's sub-fields, in display order.
    pub fields: &'static [Field],
}

/// One input on a create or edit form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// The form field name. It matches a key of the record.
    pub name: &'static str,
    /// The label shown next to the input.
    pub label: &'static str,
    /// The input type, and how the value is read.
    pub kind: FieldKind,
    /// Whether the form rejects a blank value.
    pub required: bool,
}

impl Field {
    /// Returns the trimmed value of a required field. If it is missing or
    /// blank, records "<label> is required" in `errors` under the field name
    /// and returns `None`.
    pub fn check_required<'a>(
        &self,
        form: &'a FormInput,
        errors: &mut FieldErrors,
    ) -> Option<&'a str> {
        check_required(self.name, self.label, form, errors)
    }

    /// Parses `text` as a whole number of type `T`. If it does not parse, records
    /// "<label> must be a whole number." in `errors` and returns `None`. Resources and
    /// profile fields both check numbers this way.
    pub fn check_whole_number<T: FromStr>(
        &self,
        text: &str,
        errors: &mut FieldErrors,
    ) -> Option<T> {
        check_whole_number(self.name, self.label, text, errors)
    }

    /// Parses `text` as a number, whole or decimal, such as `42` or `42.5`. Values that are
    /// not finite (`inf`, `NaN`) are refused. If it does not parse, records
    /// "<label> must be a number." in `errors` and returns `None`.
    pub fn check_number(&self, text: &str, errors: &mut FieldErrors) -> Option<f64> {
        check_number(self.name, self.label, text, errors)
    }

    /// Parses `text` as a decimal number of type `T` (`f32` or `f64`), such as `9.5`. A
    /// value that is not finite (`inf`, `NaN`, or an `f32` too large) is refused. If it
    /// does not parse, records "<label> must be a number." in `errors` and returns `None`.
    pub fn check_decimal<T: FromStr + Into<f64> + Copy>(
        &self,
        text: &str,
        errors: &mut FieldErrors,
    ) -> Option<T> {
        check_decimal(self.name, self.label, text, errors)
    }

    /// Parses `text` as a date and time: `YYYY-MM-DDTHH:MM` as a date-time input sends it,
    /// or with a space in place of the `T`, and with seconds when given. If it does not
    /// parse, records "<label> must be a date and time, as YYYY-MM-DD HH:MM." in `errors`
    /// and returns `None`.
    pub fn check_datetime(&self, text: &str, errors: &mut FieldErrors) -> Option<NaiveDateTime> {
        check_datetime(self.name, self.label, text, errors)
    }

    /// Checks that `text` is the value of one of `choices`. If it is not, records
    /// "Choose one of the options." in `errors` and returns false.
    pub fn check_choice(
        &self,
        text: &str,
        choices: &[(&str, &str)],
        errors: &mut FieldErrors,
    ) -> bool {
        check_choice(self.name, text, choices, errors)
    }

    /// Parses `text` as a `YYYY-MM-DD` date. If it does not parse, records
    /// "<label> must be a date, as YYYY-MM-DD." in `errors` and returns `None`.
    pub fn check_date(&self, text: &str, errors: &mut FieldErrors) -> Option<NaiveDate> {
        check_date(self.name, self.label, text, errors)
    }

    /// Checks that `text`, trimmed, is an email address, as the browser's `type="email"`
    /// input defines one: a local part of letters, digits and ``.!#$%&'*+/=?^_`{|}~-``, an
    /// `@`, and a domain of dot-separated labels of letters, digits and inner hyphens. If it
    /// is not, records "<label> must be a valid email address." and returns `None`.
    pub fn check_email(&self, text: &str, errors: &mut FieldErrors) -> Option<String> {
        check_email(self.name, self.label, text, errors)
    }

    /// Checks that `text`, trimmed, parses as an absolute URL, such as
    /// `https://example.com/about`. The `javascript:`, `data:` and `vbscript:` schemes are
    /// refused, because a link to one runs code. If it does not parse, records "<label> must
    /// be a valid URL." and returns `None`.
    pub fn check_url(&self, text: &str, errors: &mut FieldErrors) -> Option<String> {
        check_url(self.name, self.label, text, errors)
    }

    /// Checks that `text`, trimmed, is a color as `#rrggbb`, and returns it in lowercase. If
    /// it is not, records "<label> must be a color, as #rrggbb." and returns `None`.
    pub fn check_color(&self, text: &str, errors: &mut FieldErrors) -> Option<String> {
        check_color(self.name, self.label, text, errors)
    }

    /// Checks that `text` is the JSON of a link, `{"url": ..., "title": ..., "target": ...}`,
    /// whose URL passes [`check_url`](Self::check_url) and whose target is `""` or
    /// `"_blank"`. The title and target may be left out. Returns the link as JSON text with
    /// all three keys. Otherwise records "<label> must be a link with a valid URL." and
    /// returns `None`.
    pub fn check_link(&self, text: &str, errors: &mut FieldErrors) -> Option<String> {
        check_link(self.name, self.label, text, errors)
    }

    /// Checks `text` against the field's kind, and returns the text to store, for the kinds
    /// whose value is text: email, URL, radio, color, image, file, link, wysiwyg, post
    /// object, relationship, taxonomy, user, repeater, group, flexible content, gallery and
    /// clone. The structured kinds (relationship, repeater, group, flexible content, gallery,
    /// clone) take JSON text, check each value against its sub-field, and return the checked
    /// JSON; reading it back gives the same structure. An empty list or group is blank, so a
    /// required field records "<label> is required.". A required field must first pass
    /// [`check_required`](Self::check_required).
    ///
    /// The other kinds have checks of their own (such as
    /// [`check_whole_number`](Self::check_whole_number)); for them `text` is returned
    /// unchanged.
    pub fn check_value(&self, text: &str, errors: &mut FieldErrors) -> Option<String> {
        let kind = crate::FieldKindDef::from(self.kind);
        crate::value::check_value(self.name, self.label, self.required, &kind, text, errors)
    }
}

// The checks themselves, by field name and label, so a static `Field` and a runtime
// `FieldDef` run the same code. `name` is the key of the error in `errors`.

/// See [`Field::check_required`].
pub(crate) fn check_required<'a>(
    name: &str,
    label: &str,
    form: &'a FormInput,
    errors: &mut FieldErrors,
) -> Option<&'a str> {
    match form.get(name).map(str::trim) {
        Some(value) if !value.is_empty() => Some(value),
        _ => {
            errors.insert(name.to_string(), format!("{label} is required."));
            None
        }
    }
}

/// See [`Field::check_whole_number`].
pub(crate) fn check_whole_number<T: FromStr>(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<T> {
    match text.parse::<T>() {
        Ok(value) => Some(value),
        Err(_) => {
            errors.insert(name.to_string(), format!("{label} must be a whole number."));
            None
        }
    }
}

/// See [`Field::check_number`].
pub(crate) fn check_number(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<f64> {
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() => Some(value),
        _ => {
            errors.insert(name.to_string(), format!("{label} must be a number."));
            None
        }
    }
}

/// See [`Field::check_decimal`].
pub(crate) fn check_decimal<T: FromStr + Into<f64> + Copy>(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<T> {
    match text.parse::<T>() {
        Ok(value) if Into::<f64>::into(value).is_finite() => Some(value),
        _ => {
            errors.insert(name.to_string(), format!("{label} must be a number."));
            None
        }
    }
}

/// See [`Field::check_datetime`].
pub(crate) fn check_datetime(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<NaiveDateTime> {
    const FORMATS: [&str; 4] = [
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
    ];
    let parsed = FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(text, format).ok());
    if parsed.is_none() {
        errors.insert(
            name.to_string(),
            format!("{label} must be a date and time, as YYYY-MM-DD HH:MM."),
        );
    }
    parsed
}

/// See [`Field::check_choice`].
pub(crate) fn check_choice(
    name: &str,
    text: &str,
    choices: &[(&str, &str)],
    errors: &mut FieldErrors,
) -> bool {
    if choices.iter().any(|(value, _)| *value == text) {
        return true;
    }
    errors.insert(name.to_string(), "Choose one of the options.".to_string());
    false
}

/// See [`Field::check_date`].
pub(crate) fn check_date(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<NaiveDate> {
    match NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        Ok(value) => Some(value),
        Err(_) => {
            errors.insert(
                name.to_string(),
                format!("{label} must be a date, as YYYY-MM-DD."),
            );
            None
        }
    }
}

/// See [`Field::check_email`].
pub(crate) fn check_email(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<String> {
    let text = text.trim();
    if is_email(text) {
        return Some(text.to_string());
    }
    errors.insert(
        name.to_string(),
        format!("{label} must be a valid email address."),
    );
    None
}

/// Whether `text` is a valid email address, as the HTML standard defines one for
/// `<input type="email">`.
pub(crate) fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&b));
    let domain_ok = !domain.is_empty()
        && domain.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    local_ok && domain_ok
}

/// See [`Field::check_url`].
pub(crate) fn check_url(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<String> {
    let text = text.trim();
    if is_url(text) {
        return Some(text.to_string());
    }
    errors.insert(name.to_string(), format!("{label} must be a valid URL."));
    None
}

/// Whether `text` parses as an absolute URL whose scheme cannot run code.
pub(crate) fn is_url(text: &str) -> bool {
    match url::Url::parse(text) {
        Ok(url) => !matches!(url.scheme(), "javascript" | "data" | "vbscript"),
        Err(_) => false,
    }
}

/// See [`Field::check_color`].
pub(crate) fn check_color(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<String> {
    let text = text.trim();
    if is_color(text) {
        return Some(text.to_ascii_lowercase());
    }
    errors.insert(
        name.to_string(),
        format!("{label} must be a color, as #rrggbb."),
    );
    None
}

/// Whether `text` is a color as `#rrggbb`, in either case.
pub(crate) fn is_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// See [`Field::check_link`].
pub(crate) fn check_link(
    name: &str,
    label: &str,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<String> {
    let link = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| link_value(&value));
    match link {
        Some(link) => Some(link.to_string()),
        None => {
            errors.insert(
                name.to_string(),
                format!("{label} must be a link with a valid URL."),
            );
            None
        }
    }
}

/// The link in `value`, with its three keys, if it is a link object whose URL is valid and
/// whose target is `""` or `"_blank"`.
pub(crate) fn link_value(value: &serde_json::Value) -> Option<serde_json::Value> {
    let object = value.as_object()?;
    let text = |key: &str| match object.get(key) {
        None | Some(serde_json::Value::Null) => Some(String::new()),
        Some(serde_json::Value::String(text)) => Some(text.trim().to_string()),
        Some(_) => None,
    };
    let url = text("url")?;
    let title = text("title")?;
    let target = text("target")?;
    if !is_url(&url) || !matches!(target.as_str(), "" | "_blank") {
        return None;
    }
    Some(serde_json::json!({ "url": url, "title": title, "target": target }))
}

/// One column of the list view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    /// The record key the column shows. It matches a field name.
    pub key: &'static str,
    /// The column heading.
    pub label: &'static str,
    /// Whether the heading links to a sort by this column.
    pub sortable: bool,
    /// Whether the search box matches this column.
    pub searchable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TITLE: Field = Field {
        name: "title",
        label: "Title",
        kind: FieldKind::Text,
        required: true,
    };

    fn form(pairs: &[(&str, &str)]) -> FormInput {
        FormInput::new(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
        )
    }

    #[test]
    fn required_field_returns_trimmed_value() {
        let mut errors = FieldErrors::new();
        let input = form(&[("title", "  Hello  ")]);

        assert_eq!(TITLE.check_required(&input, &mut errors), Some("Hello"));
        assert!(errors.is_empty());
    }

    #[test]
    fn missing_required_field_records_a_message() {
        let mut errors = FieldErrors::new();

        assert_eq!(TITLE.check_required(&form(&[]), &mut errors), None);
        assert_eq!(
            errors.get("title").map(String::as_str),
            Some("Title is required.")
        );
    }

    const PRICE: Field = Field {
        name: "price",
        label: "Price",
        kind: FieldKind::Decimal("any"),
        required: true,
    };

    const LAUNCH: Field = Field {
        name: "launch",
        label: "Launch",
        kind: FieldKind::DateTime,
        required: true,
    };

    #[test]
    fn a_decimal_accepts_whole_and_fractional_numbers() {
        let mut errors = FieldErrors::new();

        assert_eq!(PRICE.check_decimal::<f64>("9.5", &mut errors), Some(9.5));
        assert_eq!(PRICE.check_decimal::<f64>("-3", &mut errors), Some(-3.0));
        assert_eq!(PRICE.check_decimal::<f32>("0.25", &mut errors), Some(0.25));
        assert!(errors.is_empty());
    }

    #[test]
    fn a_decimal_refuses_text_and_values_that_are_not_finite() {
        for text in ["abc", "9,5", "NaN", "inf", ""] {
            let mut errors = FieldErrors::new();
            assert_eq!(
                PRICE.check_decimal::<f64>(text, &mut errors),
                None,
                "{text}"
            );
            assert_eq!(
                errors.get("price").map(String::as_str),
                Some("Price must be a number."),
                "{text}"
            );
        }
        let mut errors = FieldErrors::new();
        assert_eq!(PRICE.check_decimal::<f32>("1e300", &mut errors), None);
        assert!(errors.contains_key("price"));
    }

    #[test]
    fn a_datetime_accepts_the_input_format_with_or_without_seconds() {
        let mut errors = FieldErrors::new();
        let expected = chrono::NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(14, 30, 0)
            .unwrap();

        assert_eq!(
            LAUNCH.check_datetime("2026-11-10T14:30", &mut errors),
            Some(expected)
        );
        assert_eq!(
            LAUNCH.check_datetime("2026-11-10 14:30:00", &mut errors),
            Some(expected)
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn a_datetime_refuses_what_is_not_a_date_and_time() {
        for text in ["soon", "2026-11-10", "2026-13-40T10:00", "10/11/2026 14:30"] {
            let mut errors = FieldErrors::new();
            assert_eq!(LAUNCH.check_datetime(text, &mut errors), None, "{text}");
            assert_eq!(
                errors.get("launch").map(String::as_str),
                Some("Launch must be a date and time, as YYYY-MM-DD HH:MM."),
                "{text}"
            );
        }
    }

    #[test]
    fn a_choice_must_be_one_of_the_choices() {
        let choices = [("draft", "Draft"), ("published", "Published")];
        let mut errors = FieldErrors::new();

        assert!(TITLE.check_choice("draft", &choices, &mut errors));
        assert!(errors.is_empty());
        assert!(!TITLE.check_choice("archived", &choices, &mut errors));
        assert_eq!(
            errors.get("title").map(String::as_str),
            Some("Choose one of the options.")
        );
    }

    #[test]
    fn blank_required_field_is_missing() {
        let mut errors = FieldErrors::new();

        assert_eq!(
            TITLE.check_required(&form(&[("title", "   ")]), &mut errors),
            None
        );
        assert!(errors.contains_key("title"));
    }
}
