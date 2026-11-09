//! Compiles the panel's views into Rust, so a release build renders them without
//! a template interpreter. The generated module is included in `views.rs`.

fn main() {
    propel_compiler::build::Views::new("views/ignite")
        .namespace("ignite", "views/ignite")
        .view("dashboard", "crate::views::DashboardPage")
        .view("form", "crate::views::FormPage")
        .view("index", "crate::views::IndexPage")
        .view("login", "crate::views::LoginPage")
        .build();
}
