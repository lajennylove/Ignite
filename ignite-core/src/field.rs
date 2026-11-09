//! Form fields and table columns, as the panel renders them.

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
    /// A whole or decimal number, submitted as text.
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
