//! Compiles the panel's views into Rust, and lists the panel's built files, so the
//! panel can serve them from the crate. The generated module is included in
//! `views.rs` and `assets.rs`.

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // The panel's built CSS and JS are compiled in, so a change to them rebuilds the crate.
    println!("cargo:rerun-if-changed=ui/dist/build");
    println!("cargo:rerun-if-changed=views/ignite");

    // An app that renders a view which extends the panel layout compiles it against
    // these views. Cargo passes the directory to dependents as DEP_IGNITE_PANEL_VIEWS_DIR.
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:views_dir={manifest}/views/ignite");

    write_asset_files(Path::new(&manifest));

    propel_compiler::build::Views::new("views/ignite")
        .namespace("ignite", "views/ignite")
        .asset_namespace("ui")
        .view("dashboard", "crate::views::DashboardPage")
        .view("form", "crate::views::FormPage")
        .view("index", "crate::views::IndexPage")
        .view("login", "crate::views::LoginPage")
        .build();
}

/// Writes `panel_files.rs`: every built file as `(path under build/, bytes)`.
fn write_asset_files(manifest: &Path) {
    let build = manifest.join("ui/dist/build");
    let mut files = Vec::new();
    collect(&build, &build, &mut files);
    files.sort();

    let mut source = String::from("pub(crate) static FILES: &[(&str, &[u8])] = &[\n");
    for (relative, absolute) in &files {
        source.push_str(&format!(
            "    ({relative:?}, include_bytes!({:?})),\n",
            absolute.display().to_string()
        ));
    }
    source.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("panel_files.rs");
    fs::write(out, source).expect("write panel_files.rs");
}

/// Every file under `dir`, with its path relative to `root` (forward slashes).
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|error| {
        panic!(
            "the panel's build is missing ({}): run `npm run build` in ui/: {error}",
            dir.display()
        )
    });
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path));
        }
    }
}
