//! Submitted form values and the per-field error messages.

use std::collections::HashMap;

/// Submitted form values, by field name. Empty fields are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormInput(HashMap<String, String>);

impl FormInput {
    /// Wraps the values of a submitted form, keyed by field name.
    pub fn new(values: HashMap<String, String>) -> Self {
        FormInput(values)
    }

    /// Returns the value of `name`, or `None` when the field is missing or
    /// blank. The value is not trimmed.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }

    /// Returns true when the form sent the field `name` at all, even blank. A field that is
    /// sent blank is one the user cleared; a field that is not sent is one the form does not
    /// have, so an update keeps its stored value. An unticked HTML checkbox sends nothing, so
    /// use [`checkbox`](Self::checkbox) for a checkbox, not this.
    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Returns true when the checkbox `name` was ticked. An HTML checkbox
    /// sends `on` when ticked and nothing when not, so `on`, `1` and `true`
    /// (in any case) count as ticked.
    pub fn checkbox(&self, name: &str) -> bool {
        self.get(name).is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "on" | "1" | "true"
            )
        })
    }
}

/// Error messages for a form, keyed by field name. The panel shows each
/// message next to its field when it re-renders the form.
///
/// The keys are owned `String`s, as the keys of Laravel's `MessageBag` are, so a
/// runtime resource (whose field names are data) can report errors without leaking
/// its names. A hand-written resource writes a literal key with `.to_string()`:
///
/// ```
/// use ignite_core::FieldErrors;
///
/// let mut errors = FieldErrors::new();
/// errors.insert("price".to_string(), "Price must be a whole number.".to_string());
/// assert_eq!(errors.get("price").map(String::as_str), Some("Price must be a whole number."));
/// ```
///
/// Since 0.3.0. Before, the keys were `&'static str`.
pub type FieldErrors = HashMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;

    fn form(pairs: &[(&str, &str)]) -> FormInput {
        FormInput::new(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn blank_and_missing_fields_are_none() {
        let input = form(&[("name", "Ada"), ("email", "   ")]);

        assert_eq!(input.get("name"), Some("Ada"));
        assert_eq!(input.get("email"), None);
        assert_eq!(input.get("missing"), None);
    }

    #[test]
    fn checkbox_is_true_only_for_ticked_values() {
        let input = form(&[
            ("a", "on"),
            ("b", "1"),
            ("c", "TRUE"),
            ("d", "0"),
            ("e", ""),
        ]);

        assert!(input.checkbox("a"));
        assert!(input.checkbox("b"));
        assert!(input.checkbox("c"));
        assert!(!input.checkbox("d"));
        assert!(!input.checkbox("e"));
        assert!(!input.checkbox("missing"));
    }

    #[test]
    fn has_tells_a_blank_field_from_a_missing_one() {
        let input = form(&[("name", "Ada"), ("note", "")]);

        assert!(input.has("name"));
        assert!(input.has("note"), "sent blank");
        assert_eq!(input.get("note"), None);
        assert!(!input.has("missing"));
    }
}
