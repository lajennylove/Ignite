//! Repeaters, groups and flexible content edited as sub-fields, as Filament's `Repeater`,
//! `Fieldset` and `Builder`: the settings of a repeater or flexible content
//! ([`RepeaterSettings`]), the values a form sends for one ([`structured_input`]), and the
//! add, delete and move buttons ([`apply_operation`]).
//!
//! The form names each sub-field input after its place, as PHP and Laravel read nested
//! arrays: `members[0][name]` is the `name` of the first row of `members`, `seo[title]` the
//! `title` of the group `seo`, and `content[1][data][heading]` the `heading` of the second
//! block of `content`, whose type is `content[1][type]`. The panel turns them into the JSON
//! value the field stores, a list of rows or one object, so the checks and the data layer see
//! the same value as when the form sends the field as JSON text.
//!
//! An error on a sub-field is keyed by its dotted path, as Laravel keys the errors of a nested
//! array: `members.0.name`, `seo.title`, `content.1.data.heading`. The settings path of a
//! block's sub-field names the block: `content.hero.items`. A repeater's settings are
//! keyed by the names of the fields that lead to it, without row numbers: `members`, or
//! `groups.rules` for the repeater `rules` inside each row of `groups`, as Filament's state
//! path without the item keys.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::form::FormInput;
use crate::runtime_resource::{FieldDef, FieldKindDef};

/// The settings of a repeater or flexible content field, as Filament's `Repeater` and
/// `Builder` methods. The default allows any number of rows, adds, deletes and moves rows,
/// and labels each row by its number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeaterSettings {
    min_items: Option<usize>,
    max_items: Option<usize>,
    item_label: Option<String>,
    add_action_label: Option<String>,
    addable: bool,
    deletable: bool,
    reorderable: bool,
}

impl Default for RepeaterSettings {
    fn default() -> Self {
        RepeaterSettings {
            min_items: None,
            max_items: None,
            item_label: None,
            add_action_label: None,
            addable: true,
            deletable: true,
            reorderable: true,
        }
    }
}

impl RepeaterSettings {
    /// The default settings.
    pub fn new() -> Self {
        RepeaterSettings::default()
    }

    /// The fewest rows the field saves with, as Filament's `minItems()`. Fewer is refused with
    /// "<label> must have at least <n> items.", as Laravel's array `min` rule words it.
    pub fn min_items(mut self, count: usize) -> Self {
        self.min_items = Some(count);
        self
    }

    /// The most rows the field takes, as Filament's `maxItems()`. The add button hides at the
    /// limit, and more is refused with "<label> must not have more than <n> items.".
    pub fn max_items(mut self, count: usize) -> Self {
        self.max_items = Some(count);
        self
    }

    /// Labels each row by the value of its sub-field `field`, as Filament's
    /// `itemLabel(fn (array $state) => $state['name'])` and ACF's "collapsed" setting. A row
    /// whose value is blank shows its number.
    pub fn item_label(mut self, field: impl Into<String>) -> Self {
        self.item_label = Some(field.into());
        self
    }

    /// The label of the add button, as Filament's `addActionLabel()`. The default is "Add to
    /// <label>", as Filament's.
    pub fn add_action_label(mut self, label: impl Into<String>) -> Self {
        self.add_action_label = Some(label.into());
        self
    }

    /// Whether rows can be added, as Filament's `addable()`. The default is `true`.
    pub fn addable(mut self, addable: bool) -> Self {
        self.addable = addable;
        self
    }

    /// Whether rows can be deleted, as Filament's `deletable()`. The default is `true`.
    pub fn deletable(mut self, deletable: bool) -> Self {
        self.deletable = deletable;
        self
    }

    /// Whether rows can be moved up and down, as Filament's `reorderableWithButtons()`. The
    /// default is `true`; `false` is Filament's `reorderable(false)`.
    pub fn reorderable(mut self, reorderable: bool) -> Self {
        self.reorderable = reorderable;
        self
    }

    /// The fewest rows, if set.
    pub fn get_min_items(&self) -> Option<usize> {
        self.min_items
    }

    /// The most rows, if set.
    pub fn get_max_items(&self) -> Option<usize> {
        self.max_items
    }

    /// The sub-field whose value labels each row, if set.
    pub fn get_item_label(&self) -> Option<&str> {
        self.item_label.as_deref()
    }

    /// The label of the add button, if set.
    pub fn get_add_action_label(&self) -> Option<&str> {
        self.add_action_label.as_deref()
    }

    /// Whether rows can be added.
    pub fn is_addable(&self) -> bool {
        self.addable
    }

    /// Whether rows can be deleted.
    pub fn is_deletable(&self) -> bool {
        self.deletable
    }

    /// Whether rows can be moved.
    pub fn is_reorderable(&self) -> bool {
        self.reorderable
    }
}

/// The settings of a form's repeaters and flexible content fields, by path (see the [module
/// documentation](self)). A field with no settings has the default ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Repeaters(BTreeMap<String, RepeaterSettings>);

impl Repeaters {
    /// No settings: every repeater has the default ones.
    pub fn new() -> Self {
        Repeaters::default()
    }

    /// Sets the settings of the repeater at `path`, such as `"members"` or `"groups.rules"`.
    pub fn repeater(mut self, path: impl Into<String>, settings: RepeaterSettings) -> Self {
        self.0.insert(path.into(), settings);
        self
    }

    /// The settings of the repeater at `path`, or the default ones.
    pub fn get(&self, path: &str) -> RepeaterSettings {
        self.0.get(path).cloned().unwrap_or_default()
    }
}

/// Whether a field of `kind` is edited as sub-fields: a repeater, a group, flexible content
/// or a clone.
pub fn is_structured(kind: &FieldKindDef) -> bool {
    matches!(
        kind,
        FieldKindDef::Repeater(_)
            | FieldKindDef::Group(_)
            | FieldKindDef::FlexibleContent(_)
            | FieldKindDef::Clone(_)
    )
}

/// `form`, with the value of each structured field of `fields` built from the sub-field
/// inputs it sent (`members[0][name]` and so on), as JSON text under the field's name: a list
/// of rows for a repeater or flexible content, one object for a group or a clone. Rows come
/// in the order of their numbers, numbered again from 0. A field the form sent no sub-field
/// input for keeps what the form sent under its name, such as JSON text, or nothing.
pub fn structured_input(fields: &[FieldDef], form: &FormInput) -> FormInput {
    let mut values = form.clone();
    for field in fields.iter().filter(|f| is_structured(&f.kind)) {
        let prefix = format!("{}[", field.name);
        if !form.names().any(|name| name.starts_with(&prefix)) {
            continue;
        }
        let value = collect(&field.kind, &field.name, form);
        values.insert(field.name.clone(), value.to_string());
    }
    values
}

/// The value of a structured `kind` whose inputs are named after `base`.
fn collect(kind: &FieldKindDef, base: &str, form: &FormInput) -> Value {
    match kind {
        FieldKindDef::Repeater(fields) => Value::Array(
            rows(base, form)
                .iter()
                .map(|row| Value::Object(object(fields, &format!("{base}[{row}]"), form)))
                .collect(),
        ),
        FieldKindDef::FlexibleContent(blocks) => Value::Array(
            rows(base, form)
                .iter()
                .filter_map(|row| {
                    let at = format!("{base}[{row}]");
                    let kind = form.raw(&format!("{at}[type]"))?.trim().to_string();
                    let fields = blocks
                        .iter()
                        .find(|block| block.name == kind)
                        .map(|block| block.fields.as_slice())
                        .unwrap_or(&[]);
                    let data = object(fields, &format!("{at}[data]"), form);
                    Some(serde_json::json!({ "type": kind, "data": data }))
                })
                .collect(),
        ),
        FieldKindDef::Group(fields) | FieldKindDef::Clone(fields) => {
            Value::Object(object(fields, base, form))
        }
        _ => Value::Null,
    }
}

/// The values of `fields` whose inputs are named `{base}[{name}]`. A sub-field the form did
/// not send is left out, except an unticked checkbox, which is `false`.
fn object(fields: &[FieldDef], base: &str, form: &FormInput) -> Map<String, Value> {
    let mut values = Map::new();
    for field in fields {
        let name = format!("{base}[{}]", field.name);
        let value = match &field.kind {
            kind if is_structured(kind) => {
                let prefix = format!("{name}[");
                if form.names().any(|key| key.starts_with(&prefix)) {
                    collect(kind, &name, form)
                } else {
                    // A list with no rows sends only its blank marker.
                    match form.raw(&name) {
                        Some(text) if text.trim().is_empty() => Value::Array(Vec::new()),
                        Some(text) => serde_json::from_str(text).unwrap_or(Value::Null),
                        None => continue,
                    }
                }
            }
            FieldKindDef::Checkbox => Value::Bool(form.checkbox(&name)),
            FieldKindDef::Link => {
                let part = |key: &str| form.raw(&format!("{name}[{key}]")).unwrap_or_default();
                if ["url", "title", "target"]
                    .iter()
                    .all(|key| part(key).trim().is_empty())
                {
                    match form.raw(&name) {
                        Some(text) => Value::from(text),
                        None => continue,
                    }
                } else {
                    serde_json::json!({
                        "url": part("url"),
                        "title": part("title"),
                        "target": part("target"),
                    })
                }
            }
            kind if takes_list(kind) => match form.raw(&name) {
                Some(text) if text.trim().is_empty() => Value::Array(Vec::new()),
                Some(text) => serde_json::from_str(text).unwrap_or_else(|_| Value::from(text)),
                None => continue,
            },
            _ => match form.raw(&name) {
                Some(text) => Value::from(text),
                None => continue,
            },
        };
        values.insert(field.name.clone(), value);
    }
    values
}

/// Whether a field of `kind` sends a list (`name[]`): a relationship or a gallery.
fn takes_list(kind: &FieldKindDef) -> bool {
    matches!(kind, FieldKindDef::Relationship | FieldKindDef::Gallery)
}

/// The row keys the form sent under `base` (`{base}[{row}]...`), in the order of their
/// numbers. A key that is not a number comes after the numbered ones.
fn rows(base: &str, form: &FormInput) -> Vec<String> {
    let prefix = format!("{base}[");
    let mut keys: Vec<String> = form
        .names()
        .filter_map(|name| {
            let rest = name.strip_prefix(&prefix)?;
            let (row, after) = rest.split_once(']')?;
            (after.starts_with('[') && !row.is_empty()).then(|| row.to_string())
        })
        .collect();
    keys.sort_by(|a, b| match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    });
    keys.dedup();
    keys
}

/// `form`, with the button `operation` applied to the structured fields of `fields`, as
/// Filament's repeater actions change the form's state without saving it. An operation is
/// `add:<path>` (a blank row at the end of the repeater at `path`), `add:<path>:<block>` (a
/// blank block of flexible content, as Filament's block picker adds one), `delete:<path>.<row>`, `up:<path>.<row>` or
/// `down:<path>.<row>`. `path` is the dotted path of the field's value, such as `members` or
/// `groups.0.rules`. An operation that does not apply, or that the field's settings in
/// `repeaters` do not allow, changes nothing.
pub fn apply_operation(
    fields: &[FieldDef],
    repeaters: &Repeaters,
    form: &FormInput,
    operation: &str,
) -> FormInput {
    let Some((verb, target)) = operation.split_once(':') else {
        return form.clone();
    };
    let (path, block) = match target.split_once(':') {
        Some((path, block)) => (path, Some(block)),
        None => (target, None),
    };
    let mut segments: Vec<&str> = path.split('.').collect();
    let row = if verb == "add" {
        None
    } else {
        match segments.pop().and_then(|row| row.parse::<usize>().ok()) {
            Some(row) => Some(row),
            None => return form.clone(),
        }
    };
    let Some(field) = fields
        .iter()
        .find(|f| Some(f.name.as_str()) == segments.first().copied())
    else {
        return form.clone();
    };
    let mut value = form
        .get(&field.name)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .unwrap_or(Value::Null);
    let Some((kind, schema, list)) = locate(&field.kind, &field.name, &mut value, &segments[1..])
    else {
        return form.clone();
    };
    let settings = repeaters.get(&schema);
    let changed = match (verb, row) {
        ("add", None) if settings.is_addable() => {
            let full = settings
                .get_max_items()
                .is_some_and(|max| list.len() >= max);
            let blank = match (kind, block) {
                (FieldKindDef::Repeater(_), None) => Some(Value::Object(Map::new())),
                (FieldKindDef::FlexibleContent(blocks), Some(block))
                    if blocks.iter().any(|b| b.name == block) =>
                {
                    Some(serde_json::json!({ "type": block, "data": {} }))
                }
                _ => None,
            };
            match blank.filter(|_| !full) {
                Some(blank) => {
                    list.push(blank);
                    true
                }
                None => false,
            }
        }
        ("delete", Some(row)) if settings.is_deletable() && row < list.len() => {
            list.remove(row);
            true
        }
        ("up", Some(row)) if settings.is_reorderable() && row > 0 && row < list.len() => {
            list.swap(row - 1, row);
            true
        }
        ("down", Some(row)) if settings.is_reorderable() && row + 1 < list.len() => {
            list.swap(row, row + 1);
            true
        }
        _ => false,
    };
    let mut values = form.clone();
    if changed {
        values.insert(field.name.clone(), value.to_string());
    }
    values
}

/// The list of rows at `segments` inside `value`, a value of `kind`, with the kind of the field
/// that holds it and its settings path (`schema`, the names without row numbers). A missing
/// list is created empty, so a first row can be added to a field that has no value yet.
fn locate<'k, 'v>(
    kind: &'k FieldKindDef,
    schema: &str,
    value: &'v mut Value,
    segments: &[&str],
) -> Option<(&'k FieldKindDef, String, &'v mut Vec<Value>)> {
    let Some((first, rest)) = segments.split_first() else {
        if !matches!(
            kind,
            FieldKindDef::Repeater(_) | FieldKindDef::FlexibleContent(_)
        ) {
            return None;
        }
        if !value.is_array() {
            *value = Value::Array(Vec::new());
        }
        return Some((kind, schema.to_string(), value.as_array_mut()?));
    };
    match kind {
        FieldKindDef::Repeater(fields) => {
            let row = value
                .as_array_mut()?
                .get_mut(first.parse::<usize>().ok()?)?;
            let (name, rest) = rest.split_first()?;
            let field = fields.iter().find(|f| f.name == *name)?;
            let inner = row
                .as_object_mut()?
                .entry(name.to_string())
                .or_insert(Value::Null);
            locate(&field.kind, &format!("{schema}.{name}"), inner, rest)
        }
        FieldKindDef::FlexibleContent(blocks) => {
            let row = value
                .as_array_mut()?
                .get_mut(first.parse::<usize>().ok()?)?;
            let block = row.get("type")?.as_str()?.to_string();
            let fields = &blocks.iter().find(|b| b.name == block)?.fields;
            let (data, rest) = rest.split_first()?;
            if *data != "data" {
                return None;
            }
            let (name, rest) = rest.split_first()?;
            let field = fields.iter().find(|f| f.name == *name)?;
            let inner = row
                .as_object_mut()?
                .entry("data")
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()?
                .entry(name.to_string())
                .or_insert(Value::Null);
            locate(
                &field.kind,
                &format!("{schema}.{block}.{name}"),
                inner,
                rest,
            )
        }
        FieldKindDef::Group(fields) | FieldKindDef::Clone(fields) => {
            let field = fields.iter().find(|f| f.name == *first)?;
            if !value.is_object() {
                *value = Value::Object(Map::new());
            }
            let inner = value
                .as_object_mut()?
                .entry(first.to_string())
                .or_insert(Value::Null);
            locate(&field.kind, &format!("{schema}.{first}"), inner, rest)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn text(name: &str) -> FieldDef {
        FieldDef {
            name: name.into(),
            label: name.into(),
            kind: FieldKindDef::Text,
            required: false,
        }
    }

    fn members() -> FieldDef {
        FieldDef {
            name: "members".into(),
            label: "Members".into(),
            kind: FieldKindDef::Repeater(vec![
                text("name"),
                FieldDef {
                    name: "admin".into(),
                    label: "Admin".into(),
                    kind: FieldKindDef::Checkbox,
                    required: false,
                },
            ]),
            required: false,
        }
    }

    fn form(pairs: &[(&str, &str)]) -> FormInput {
        FormInput::new(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
        )
    }

    #[test]
    fn rows_are_built_in_the_order_of_their_numbers() {
        let sent = form(&[
            ("members", ""),
            ("members[10][name]", "Ten"),
            ("members[2][name]", "Two"),
            ("members[2][admin]", "on"),
        ]);
        let values = structured_input(&[members()], &sent);
        assert_eq!(
            values.get("members"),
            Some(r#"[{"admin":true,"name":"Two"},{"admin":false,"name":"Ten"}]"#)
        );
    }

    #[test]
    fn a_field_sent_as_json_is_kept() {
        let sent = form(&[("members", r#"[{"name":"A"}]"#)]);
        let values = structured_input(&[members()], &sent);
        assert_eq!(values.get("members"), Some(r#"[{"name":"A"}]"#));
    }

    #[test]
    fn buttons_add_move_and_delete_rows() {
        let fields = [members()];
        let sent = form(&[("members", r#"[{"name":"A"},{"name":"B"}]"#)]);
        let repeaters = Repeaters::new();
        let moved = apply_operation(&fields, &repeaters, &sent, "down:members.0");
        assert_eq!(moved.get("members"), Some(r#"[{"name":"B"},{"name":"A"}]"#));
        let added = apply_operation(&fields, &repeaters, &moved, "add:members");
        assert_eq!(
            added.get("members"),
            Some(r#"[{"name":"B"},{"name":"A"},{}]"#)
        );
        let deleted = apply_operation(&fields, &repeaters, &added, "delete:members.1");
        assert_eq!(deleted.get("members"), Some(r#"[{"name":"B"},{}]"#));
        let limited = Repeaters::new().repeater("members", RepeaterSettings::new().max_items(2));
        let full = apply_operation(&fields, &limited, &deleted, "add:members");
        assert_eq!(full.get("members"), deleted.get("members"));
    }
}
