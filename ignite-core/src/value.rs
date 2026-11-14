//! The checks of the field kinds whose value is text, and of the values nested in the
//! structured kinds (relationship, gallery, repeater, group, flexible content and clone),
//! which store JSON text. Static fields and runtime fields run the same code: a static
//! [`Field`](crate::Field) is converted to its [`FieldKindDef`] first.

use serde_json::{Map, Value};

use crate::field as check;
use crate::form::FieldErrors;
use crate::runtime_resource::{BlockDef, FieldDef, FieldKindDef};

/// See [`Field::check_value`](crate::Field::check_value). `name` is the key of the error in
/// `errors`.
pub(crate) fn check_value(
    name: &str,
    label: &str,
    required: bool,
    kind: &FieldKindDef,
    text: &str,
    errors: &mut FieldErrors,
) -> Option<String> {
    match kind {
        FieldKindDef::Email => check::check_email(name, label, text, errors),
        FieldKindDef::Url => check::check_url(name, label, text, errors),
        FieldKindDef::Color => check::check_color(name, label, text, errors),
        FieldKindDef::Radio(choices) => {
            let text = text.trim();
            check::check_choice(name, text, &borrowed(choices), errors).then(|| text.to_string())
        }
        FieldKindDef::Image | FieldKindDef::File => match media_id(&Value::from(text.trim())) {
            Some(id) => Some(id.to_string()),
            None => {
                errors.insert(name.to_string(), media_message(label));
                None
            }
        },
        FieldKindDef::PostObject | FieldKindDef::Taxonomy | FieldKindDef::User => {
            Some(text.trim().to_string())
        }
        FieldKindDef::Link
        | FieldKindDef::Relationship
        | FieldKindDef::Gallery
        | FieldKindDef::Repeater(_)
        | FieldKindDef::Group(_)
        | FieldKindDef::FlexibleContent(_)
        | FieldKindDef::Clone(_) => {
            let checked = serde_json::from_str::<Value>(text)
                .map_err(|_| shape_message(kind, label))
                .and_then(|value| structured(kind, label, &value))
                .and_then(|value| {
                    if required && is_empty(&value) {
                        Err(format!("{label} is required."))
                    } else {
                        Ok(value)
                    }
                });
            match checked {
                Ok(value) => Some(value.to_string()),
                Err(message) => {
                    errors.insert(name.to_string(), message);
                    None
                }
            }
        }
        _ => Some(text.to_string()),
    }
}

/// `(value, label)` pairs borrowed from owned ones, for [`check::check_choice`].
fn borrowed(choices: &[(String, String)]) -> Vec<(&str, &str)> {
    choices
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect()
}

/// "<label> must be the id of a media item."
fn media_message(label: &str) -> String {
    format!("{label} must be the id of a media item.")
}

/// The message for a structured value whose JSON does not have the kind's shape.
fn shape_message(kind: &FieldKindDef, label: &str) -> String {
    match kind {
        FieldKindDef::Link => format!("{label} must be a link with a valid URL."),
        FieldKindDef::Relationship => format!("{label} must be a list of choices."),
        FieldKindDef::Gallery => format!("{label} must be a list of media ids."),
        FieldKindDef::Repeater(_) => format!("{label} must be a list of rows."),
        FieldKindDef::FlexibleContent(_) => format!("{label} must be a list of blocks."),
        _ => format!("{label} must be a group of values."),
    }
}

/// Whether a structured value has no rows, choices or values.
fn is_empty(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(values) => values.is_empty(),
        _ => false,
    }
}

/// A media id: a positive whole number, as a JSON number or as text.
fn media_id(value: &Value) -> Option<i64> {
    let id = match value {
        Value::Number(number) => number.as_i64()?,
        Value::String(text) => text.trim().parse::<i64>().ok()?,
        _ => return None,
    };
    (id > 0).then_some(id)
}

/// Checks the JSON `value` of a structured kind (or a link), and returns it as it is stored.
fn structured(kind: &FieldKindDef, label: &str, value: &Value) -> Result<Value, String> {
    let shape = || shape_message(kind, label);
    match kind {
        FieldKindDef::Link => check::link_value(value).ok_or_else(shape),
        FieldKindDef::Relationship => {
            let items = value.as_array().ok_or_else(shape)?;
            items
                .iter()
                .map(|item| match item {
                    Value::String(text) if !text.trim().is_empty() => Ok(Value::from(text.trim())),
                    Value::Number(number) => Ok(Value::from(number.to_string())),
                    _ => Err(shape()),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        FieldKindDef::Gallery => {
            let items = value.as_array().ok_or_else(shape)?;
            items
                .iter()
                .map(|item| media_id(item).map(Value::from).ok_or_else(shape))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        FieldKindDef::Repeater(fields) => {
            let rows = value.as_array().ok_or_else(shape)?;
            let mut checked = Vec::with_capacity(rows.len());
            for (index, row) in rows.iter().enumerate() {
                let row = row.as_object().ok_or_else(shape)?;
                let values = values_of(fields, row)
                    .map_err(|message| format!("Row {}: {message}", index + 1))?;
                checked.push(Value::Object(values));
            }
            Ok(Value::Array(checked))
        }
        FieldKindDef::Group(fields) | FieldKindDef::Clone(fields) => {
            let values = value.as_object().ok_or_else(shape)?;
            values_of(fields, values).map(Value::Object)
        }
        FieldKindDef::FlexibleContent(blocks) => {
            let rows = value.as_array().ok_or_else(shape)?;
            let mut checked = Vec::with_capacity(rows.len());
            for (index, row) in rows.iter().enumerate() {
                checked.push(block_row(blocks, label, index + 1, row).ok_or_else(shape)??);
            }
            Ok(Value::Array(checked))
        }
        _ => Err(shape()),
    }
}

/// One row of a flexible content field: `{"type": <block>, "data": {...}}`. Returns `None`
/// when the row does not have that shape, and an error message when its block is unknown or
/// its data does not pass the block's sub-fields.
fn block_row(
    blocks: &[BlockDef],
    label: &str,
    number: usize,
    row: &Value,
) -> Option<Result<Value, String>> {
    let row = row.as_object()?;
    let block_type = row.get("type")?.as_str()?;
    let data = row.get("data")?.as_object()?;
    let Some(block) = blocks.iter().find(|block| block.name == block_type) else {
        return Some(Err(format!(
            "Block {number}: \"{block_type}\" is not a block of {label}."
        )));
    };
    Some(
        values_of(&block.fields, data)
            .map(|values| serde_json::json!({ "type": block_type, "data": values }))
            .map_err(|message| format!("Block {number}: {message}")),
    )
}

/// Checks the values of `fields` in `values`, keyed by sub-field name, and returns them as
/// they are stored. A key that is not a sub-field is dropped. A sub-field that is missing is
/// left out, unless it is required. The first error is returned.
fn values_of(
    fields: &[FieldDef],
    values: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let mut checked = Map::new();
    for field in fields {
        match values.get(&field.name) {
            Some(value) => {
                checked.insert(field.name.clone(), nested(field, value)?);
            }
            None if field.required && field.kind != FieldKindDef::Checkbox => {
                return Err(format!("{} is required.", field.label));
            }
            None => {}
        }
    }
    Ok(checked)
}

/// Checks one nested value against its sub-field, and returns it as it is stored. A blank
/// value (`null` or blank text) is `null`, or an error when the sub-field is required.
fn nested(field: &FieldDef, value: &Value) -> Result<Value, String> {
    let label = field.label.as_str();
    let blank = match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        _ => false,
    };
    if blank && field.kind != FieldKindDef::Checkbox {
        return if field.required {
            Err(format!("{label} is required."))
        } else {
            Ok(Value::Null)
        };
    }
    // Runs a text check of `field.rs` and returns its message on failure.
    let run = |check: &dyn Fn(&str, &mut FieldErrors) -> Option<Value>| {
        let text = match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => return Err(format!("{label} must be text.")),
        };
        let mut errors = FieldErrors::new();
        check(&text, &mut errors).ok_or_else(|| errors.into_values().next().unwrap_or_default())
    };
    let name = field.name.as_str();
    let checked = match &field.kind {
        FieldKindDef::Text | FieldKindDef::Textarea | FieldKindDef::Wysiwyg => match value {
            Value::String(_) => Ok(value.clone()),
            _ => Err(format!("{label} must be text.")),
        },
        FieldKindDef::Password => Err(format!("{label} cannot be stored in a nested field.")),
        FieldKindDef::Number => match value.as_i64() {
            Some(number) => Ok(Value::from(number)),
            None => run(&|text, errors| {
                check::check_whole_number::<i64>(name, label, text, errors).map(Value::from)
            }),
        },
        FieldKindDef::Decimal(_) => run(&|text, errors| {
            check::check_decimal::<f64>(name, label, text, errors).map(Value::from)
        }),
        FieldKindDef::Checkbox => Ok(Value::Bool(match value {
            Value::Bool(ticked) => *ticked,
            Value::String(text) => {
                matches!(
                    text.trim().to_ascii_lowercase().as_str(),
                    "on" | "1" | "true"
                )
            }
            Value::Number(number) => number.as_i64() == Some(1),
            _ => false,
        })),
        FieldKindDef::Select(choices) | FieldKindDef::Radio(choices) => run(&|text, errors| {
            let text = text.trim();
            check::check_choice(name, text, &borrowed(choices), errors).then(|| Value::from(text))
        }),
        FieldKindDef::Lookup
        | FieldKindDef::PostObject
        | FieldKindDef::Taxonomy
        | FieldKindDef::User => run(&|text, _| Some(Value::from(text.trim()))),
        FieldKindDef::Date => run(&|text, errors| {
            check::check_date(name, label, text.trim(), errors)
                .map(|date| Value::from(date.format("%Y-%m-%d").to_string()))
        }),
        FieldKindDef::DateTime => run(&|text, errors| {
            check::check_datetime(name, label, text.trim(), errors)
                .map(|stamp| Value::from(stamp.format("%Y-%m-%d %H:%M:%S").to_string()))
        }),
        FieldKindDef::Email => {
            run(&|text, errors| check::check_email(name, label, text, errors).map(Value::from))
        }
        FieldKindDef::Url => {
            run(&|text, errors| check::check_url(name, label, text, errors).map(Value::from))
        }
        FieldKindDef::Color => {
            run(&|text, errors| check::check_color(name, label, text, errors).map(Value::from))
        }
        FieldKindDef::Image | FieldKindDef::File => media_id(value)
            .map(Value::from)
            .ok_or_else(|| media_message(label)),
        kind @ (FieldKindDef::Link
        | FieldKindDef::Relationship
        | FieldKindDef::Gallery
        | FieldKindDef::Repeater(_)
        | FieldKindDef::Group(_)
        | FieldKindDef::FlexibleContent(_)
        | FieldKindDef::Clone(_)) => structured(kind, label, value),
    }?;
    if field.required && is_empty(&checked) {
        return Err(format!("{label} is required."));
    }
    Ok(checked)
}
