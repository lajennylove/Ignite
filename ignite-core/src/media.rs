//! The media library that [`Image`](crate::FieldKind::Image),
//! [`File`](crate::FieldKind::File) and [`Gallery`](crate::FieldKind::Gallery) fields pick
//! from.
//!
//! Ignite assumes no storage, table or upload path: the app implements [`MediaLibrary`]
//! and registers it with [`register_media_library`], as Laravel binds an implementation to
//! a contract. A media field stores item ids. With a library registered, the panel shows
//! the stored items and refuses an id the library does not have. With none, the fields
//! still render, and their ids are checked as positive whole numbers only.
//!
//! ```ignore
//! ignite_core::register_media_library(Attachments);
//! ```

use std::sync::{Arc, PoisonError, RwLock};

use orbit_orm::Database;
use serde::Serialize;

use crate::RuntimeFuture;

/// One item of the media library, as a form shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaItem {
    /// The item's id, as a media field stores it.
    pub id: i64,
    /// The item's title, shown next to it.
    pub title: String,
    /// A URL that shows the item: the image itself, or the file to download.
    pub url: String,
}

/// The app's media library. Implement it and add it with [`register_media_library`].
pub trait MediaLibrary: Send + Sync + 'static {
    /// The items among `ids` that exist, in any order. An id with no item is left out.
    fn find<'a>(&'a self, db: &'a Database, ids: &'a [i64]) -> RuntimeFuture<'a, Vec<MediaItem>>;

    /// The URL of the page where a user browses and uploads media, which the form links to
    /// so the user can find an item's id. The default is none.
    fn browse_url(&self) -> Option<String> {
        None
    }
}

static LIBRARY: RwLock<Option<Arc<dyn MediaLibrary>>> = RwLock::new(None);

/// Sets the media library that media fields use, from the next request on. Registering
/// again replaces it.
pub fn register_media_library(library: impl MediaLibrary) {
    let mut slot = LIBRARY.write().unwrap_or_else(PoisonError::into_inner);
    *slot = Some(Arc::new(library));
}

/// The registered media library, if the app registered one.
pub fn media_library() -> Option<Arc<dyn MediaLibrary>> {
    LIBRARY
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}
