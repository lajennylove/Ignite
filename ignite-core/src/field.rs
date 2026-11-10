//! Form fields and table columns, as the panel renders them.

use std::str::FromStr;

use chrono::NaiveDate;

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
    /// A number, submitted as text. A resource's integer field is checked as a whole
    /// number. A profile field accepts decimals as well (see [`Field::check_number`]).
    Number,
    /// A yes/no value. Read it with [`FormInput::checkbox`](crate::FormInput::checkbox).
    Checkbox,
    /// One choice from a list of `(value, label)` pairs.
    Select(&'static [(&'static str, &'static str)]),
    /// A calendar date, as `YYYY-MM-DD`.
    Date,
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
        match form.get(self.name).map(str::trim) {
            Some(value) if !value.is_empty() => Some(value),
            _ => {
                errors.insert(self.name, format!("{} is required.", self.label));
                None
            }
        }
    }

    /// Parses `text` as a whole number of type `T`. If it does not parse, records
    /// "<label> must be a whole number." in `errors` and returns `None`. Resources and
    /// profile fields both check numbers this way.
    pub fn check_whole_number<T: FromStr>(
        &self,
        text: &str,
        errors: &mut FieldErrors,
    ) -> Option<T> {
        match text.parse::<T>() {
            Ok(value) => Some(value),
            Err(_) => {
                errors.insert(self.name, format!("{} must be a whole number.", self.label));
                None
            }
        }
    }

    /// Parses `text` as a number, whole or decimal, such as `42` or `42.5`. Values that are
    /// not finite (`inf`, `NaN`) are refused. If it does not parse, records
    /// "<label> must be a number." in `errors` and returns `None`.
    pub fn check_number(&self, text: &str, errors: &mut FieldErrors) -> Option<f64> {
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Some(value),
            _ => {
                errors.insert(self.name, format!("{} must be a number.", self.label));
                None
            }
        }
    }

    /// Parses `text` as a `YYYY-MM-DD` date. If it does not parse, records
    /// "<label> must be a date, as YYYY-MM-DD." in `errors` and returns `None`.
    pub fn check_date(&self, text: &str, errors: &mut FieldErrors) -> Option<NaiveDate> {
        match NaiveDate::parse_from_str(text, "%Y-%m-%d") {
            Ok(value) => Some(value),
            Err(_) => {
                errors.insert(
                    self.name,
                    format!("{} must be a date, as YYYY-MM-DD.", self.label),
                );
                None
            }
        }
    }
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
