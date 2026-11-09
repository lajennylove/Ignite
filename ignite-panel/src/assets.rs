//! The panel's own stylesheet and script. They are built from `ui/` (Tailwind and
//! Vite) into `ui/dist/build`, which is committed and embedded in the crate, so an
//! app needs no Node and no Vite configuration.
//!
//! Propel's [`Assets`] reads the manifest and the hot file from a directory on disk,
//! and it writes URLs under `/build/`. A debug build points it at `ui/dist`, so
//! `npm run dev` in `ui/` takes over. A release build has no source tree, so it
//! copies the embedded build to a temporary directory and points `Assets` there.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};
use propel_runtime::Assets;

/// The built files, embedded in the crate: `manifest.json`, the hashed CSS and JS.
pub(crate) static BUILD: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/ui/dist/build");

/// The Vite entries the panel loads, as keys of its manifest.
pub(crate) const ENTRIES: &[&str] = &["resources/css/app.css", "resources/js/app.js"];

/// The `Assets` for the panel's build. In a debug build it reads `ui/dist`, so the
/// Vite dev server's hot file is honoured. In a release build it reads a copy of the
/// embedded build, written to the temporary directory.
pub(crate) fn panel_assets() -> io::Result<Assets> {
    if cfg!(debug_assertions) {
        return Ok(Assets::new(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/dist")));
    }
    let root = extracted_root();
    write_embedded(&BUILD, &root.join("build"))?;
    Ok(Assets::new(root))
}

/// The directory a release build extracts its copy into: one per process.
fn extracted_root() -> PathBuf {
    std::env::temp_dir().join(format!("ignite-panel-{}", std::process::id()))
}

/// Writes every embedded file under `dir`, keeping its path relative to `dir`.
fn write_embedded(source: &Dir<'_>, dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for file in source.files() {
        let name = file.path().file_name().expect("embedded files have names");
        fs::write(dir.join(name), file.contents())?;
    }
    for sub in source.dirs() {
        let name = sub
            .path()
            .file_name()
            .expect("embedded directories have names");
        write_embedded(sub, &dir.join(name))?;
    }
    Ok(())
}

/// The embedded file at `path` under the build, such as `assets/app-1a2b.css`.
pub(crate) fn embedded(path: &Path) -> Option<&'static [u8]> {
    BUILD.get_file(path).map(|file| file.contents())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_names_both_entries() {
        let manifest = embedded(Path::new("manifest.json")).expect("manifest is embedded");
        let text = std::str::from_utf8(manifest).unwrap();
        for entry in ENTRIES {
            assert!(text.contains(entry), "manifest lacks {entry}");
        }
    }
}
