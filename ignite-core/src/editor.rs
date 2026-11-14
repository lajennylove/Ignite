//! The rich text editor of [`FieldKind::Wysiwyg`](crate::FieldKind::Wysiwyg) fields.
//!
//! Ignite is MIT licensed and bundles no editor. The panel renders a wysiwyg field as a
//! `<textarea data-ignite-editor="wysiwyg">` holding the HTML, and, when the app has
//! registered an editor script, loads it on each form that has such a field. The script
//! turns each marked textarea into an editor and keeps the textarea's value up to date, so
//! the form submits the HTML as usual.
//!
//! ```ignore
//! ignite_core::register_editor("/vendor/editor/init.js");
//! ```

use std::sync::{PoisonError, RwLock};

static EDITOR: RwLock<Option<String>> = RwLock::new(None);

/// Sets the URL of the script that turns wysiwyg textareas into an editor. The panel loads
/// it with `<script src="..." defer>` on each form with a wysiwyg field, from the next request
/// on. Registering again replaces the URL. With no script registered, a wysiwyg field is a
/// plain textarea.
pub fn register_editor(script_url: impl Into<String>) {
    let mut editor = EDITOR.write().unwrap_or_else(PoisonError::into_inner);
    *editor = Some(script_url.into());
}

/// The URL of the registered editor script, if the app registered one.
pub fn editor_script() -> Option<String> {
    EDITOR
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}
