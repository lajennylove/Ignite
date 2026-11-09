//! Compiles the panel's views into Rust, so a release build renders them without
//! a template interpreter. The generated module is included in `views.rs`.

fn main() {
    // An app that renders a view which extends the panel layout compiles it against
    // these views. Cargo passes the directory to dependents as DEP_IGNITE_PANEL_VIEWS_DIR.
    let views = format!("{}/views/ignite", std::env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:views_dir={views}");

    propel_compiler::build::Views::new("views/ignite")
        .namespace("ignite", "views/ignite")
        .view("dashboard", "crate::views::DashboardPage")
        .view("form", "crate::views::FormPage")
        .view("index", "crate::views::IndexPage")
        .view("login", "crate::views::LoginPage")
        .build();
}
