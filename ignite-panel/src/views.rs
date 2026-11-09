//! The view models the Propel templates read. Propel expressions can only read
//! a path, so every condition, URL and label is computed here first.

use ignite_core::{Field, FieldErrors, FieldKind, FormInput};
use serde::Serialize;
use serde_json::Value;

use crate::ignite::Registry;

/// The values every page shares: the navigation, the sign-out link and the flash message.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Layout {
    pub(crate) home_url: String,
    pub(crate) signed_in: bool,
    pub(crate) logout_url: String,
    pub(crate) nav: Vec<NavLink>,
    pub(crate) has_flash: bool,
    pub(crate) flash: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NavLink {
    pub(crate) title: &'static str,
    pub(crate) url: String,
}

impl Layout {
    /// The layout for a page. Signed-out pages show no navigation.
    pub(crate) fn new(registry: &Registry, signed_in: bool, flash: Option<String>) -> Self {
        let nav = if signed_in {
            registry
                .resources
                .iter()
                .map(|r| NavLink {
                    title: r.title(),
                    url: format!("{}/{}", registry.mount, r.slug()),
                })
                .collect()
        } else {
            Vec::new()
        };
        Layout {
            home_url: registry.mount.clone(),
            signed_in,
            logout_url: format!("{}/logout", registry.mount),
            nav,
            has_flash: flash.is_some(),
            flash: flash.unwrap_or_default(),
        }
    }
}

/// The sign-in page.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LoginPage {
    #[serde(flatten)]
    pub(crate) layout: Layout,
    pub(crate) title: &'static str,
    pub(crate) action: String,
    pub(crate) email: String,
    pub(crate) has_error: bool,
    pub(crate) error: String,
}

/// The list view of a resource.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct IndexPage {
    #[serde(flatten)]
    pub(crate) layout: Layout,
    pub(crate) title: &'static str,
    pub(crate) index_url: String,
    pub(crate) new_url: String,
    pub(crate) search: String,
    pub(crate) columns: Vec<ColumnHead>,
    pub(crate) colspan: usize,
    pub(crate) rows: Vec<RowView>,
    pub(crate) total: i64,
    pub(crate) page: i64,
    pub(crate) last_page: i64,
    pub(crate) has_prev: bool,
    pub(crate) prev_url: String,
    pub(crate) has_next: bool,
    pub(crate) next_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ColumnHead {
    pub(crate) label: &'static str,
    pub(crate) sortable: bool,
    pub(crate) sort_url: String,
    pub(crate) indicator: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RowView {
    pub(crate) cells: Vec<CellView>,
    pub(crate) edit_url: String,
    pub(crate) delete_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CellView {
    pub(crate) text: String,
}

/// The create and edit forms.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct FormPage {
    #[serde(flatten)]
    pub(crate) layout: Layout,
    pub(crate) title: &'static str,
    pub(crate) heading: String,
    pub(crate) action: String,
    pub(crate) submit_label: &'static str,
    pub(crate) back_url: String,
    pub(crate) fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldView {
    pub(crate) name: &'static str,
    pub(crate) label: &'static str,
    pub(crate) required: bool,
    pub(crate) input_type: &'static str,
    pub(crate) is_textarea: bool,
    pub(crate) is_checkbox: bool,
    pub(crate) is_select: bool,
    pub(crate) value: String,
    pub(crate) checked: bool,
    pub(crate) options: Vec<OptionView>,
    pub(crate) has_error: bool,
    pub(crate) error: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OptionView {
    pub(crate) value: &'static str,
    pub(crate) label: &'static str,
    pub(crate) selected: bool,
}

impl FieldView {
    /// Builds the input for `field`, with the value from `form` and the
    /// error for it, if there is one. A password is never filled in.
    pub(crate) fn new(field: &Field, form: &FormInput, errors: &FieldErrors) -> Self {
        let (input_type, is_textarea, is_checkbox, is_select) = match field.kind {
            FieldKind::Text => ("text", false, false, false),
            FieldKind::Textarea => ("text", true, false, false),
            FieldKind::Password => ("password", false, false, false),
            FieldKind::Number => ("number", false, false, false),
            FieldKind::Checkbox => ("checkbox", false, true, false),
            FieldKind::Select(_) => ("text", false, false, true),
            FieldKind::Date => ("date", false, false, false),
        };
        let value = match field.kind {
            FieldKind::Password => String::new(),
            _ => form.get(field.name).unwrap_or("").to_string(),
        };
        let options = match field.kind {
            FieldKind::Select(choices) => choices
                .iter()
                .map(|(v, label)| OptionView {
                    value: v,
                    label,
                    selected: *v == value,
                })
                .collect(),
            _ => Vec::new(),
        };
        let error = errors.get(field.name).cloned();

        FieldView {
            name: field.name,
            label: field.label,
            required: field.required,
            input_type,
            is_textarea,
            is_checkbox,
            is_select,
            value,
            checked: is_checkbox && form.checkbox(field.name),
            options,
            has_error: error.is_some(),
            error: error.unwrap_or_default(),
        }
    }
}

/// Turns a JSON cell into the text a table shows.
pub(crate) fn cell_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(true)) => "Yes".to_string(),
        Some(Value::Bool(false)) => "No".to_string(),
        Some(other) => other.to_string(),
    }
}

/// Turns a stored JSON record into the values a form starts with. Passwords
/// and empty values are left out, so the form shows them blank.
pub(crate) fn record_form(fields: &[Field], record: &Value) -> FormInput {
    let mut values = std::collections::HashMap::new();
    for field in fields {
        let value = match (field.kind, record.get(field.name)) {
            (FieldKind::Password, _) | (_, None | Some(Value::Null)) => continue,
            (FieldKind::Checkbox, Some(Value::Bool(true))) => "on".to_string(),
            (FieldKind::Checkbox, _) => continue,
            (_, Some(Value::String(s))) => s.clone(),
            (_, Some(other)) => other.to_string(),
        };
        values.insert(field.name.to_string(), value);
    }
    FormInput::new(values)
}

/// The fields of a form page, in display order.
pub(crate) fn field_views(
    fields: &[Field],
    form: &FormInput,
    errors: &FieldErrors,
) -> Vec<FieldView> {
    fields
        .iter()
        .map(|f| FieldView::new(f, form, errors))
        .collect()
}
