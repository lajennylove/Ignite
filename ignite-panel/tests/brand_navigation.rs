//! The panel's brand and navigation options, as Filament's panel and resources set them: the
//! brand name, logo and favicon, the app's stylesheets, the navigation groups, the sort and the
//! icons. Runs against the `ignite_test` database. Run it with `--release` too: a release build renders the views
//! compiled into the panel, so the options must be in them.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, IgniteResource, ListQuery, Page, Row, RuntimeData,
    RuntimeFuture, RuntimeResource,
};
use ignite_panel::Ignite;
use orbit_orm::{Database, OrbitModel};
use rocket::http::Status;
use rocket::local::blocking::Client;
use serde::Serialize;

/// The HTML of `path`.
fn page(client: &Client, path: &str) -> String {
    let response = client.get(path).dispatch();
    assert_eq!(response.status(), Status::Ok, "GET {path}");
    response.into_string().unwrap()
}

/// The text of the `<title>` element, without the line breaks of the title section.
fn title(html: &str) -> String {
    let start = html.find("<title>").expect("a title") + "<title>".len();
    let end = html.find("</title>").expect("the end of the title");
    html[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const MARK: &str =
    r#"<img src="/admin/build/resources/images/ignite-mark.png" alt="" class="h-9 w-auto">"#;

#[test]
fn the_default_brand_is_ignite_with_its_mark_and_favicons() {
    let _guard = reset();
    let client = signed_in(None);
    let html = page(&client, "/admin/widgets");
    assert_eq!(title(&html), "Widgets · Ignite");
    assert!(html.contains(MARK), "{html}");
    assert!(html.contains(r#"<span class="text-xl">Ignite</span>"#));
    assert!(html.contains(
        r#"<link rel="icon" href="/admin/build/resources/images/favicon.ico" sizes="any">"#
    ));
    assert!(html.contains(r#"<link rel="apple-touch-icon""#));

    let guest = client_with(|ignite| ignite);
    let html = page(&guest, "/admin/login");
    assert_eq!(title(&html), "Sign in · Ignite");
    assert!(html.contains(MARK));
}

#[test]
fn a_brand_name_replaces_the_name_and_the_mark() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.brand_name("Mission Control"));
    let html = page(&client, "/admin");
    assert_eq!(title(&html), "Dashboard · Mission Control");
    assert!(html.contains(r#"<span class="text-xl">Mission Control</span>"#));
    assert!(!html.contains("ignite-mark.png"), "{html}");

    let guest = client_with(|ignite| ignite.brand_name("Mission Control"));
    let html = page(&guest, "/admin/login");
    assert_eq!(title(&html), "Sign in · Mission Control");
    assert!(
        html.contains("Mission Control\n"),
        "the sign-in bar shows the name"
    );
    assert!(!html.contains("ignite-mark.png"));
}

#[test]
fn a_brand_logo_and_favicon_appear_in_the_page() {
    let _guard = reset();
    let configure = |ignite: ignite_panel::Ignite| {
        ignite
            .brand_name("Mission Control")
            .brand_logo("/mc-includes/images/mission-control-logo.webp")
            .brand_logo_height("2.5rem")
            .favicon("/mc-includes/images/favicon.png")
    };
    let logo = r#"<img src="/mc-includes/images/mission-control-logo.webp" alt="Mission Control" class="w-auto" style="height: 2.5rem">"#;
    let favicon = r#"<link rel="icon" href="/mc-includes/images/favicon.png">"#;

    let client = signed_in_with(configure);
    let html = page(&client, "/admin/widgets");
    assert!(html.contains(logo), "{html}");
    assert!(html.contains(favicon));
    assert!(
        !html.contains(r#"<span class="text-xl">"#),
        "the logo replaces the name"
    );
    assert!(!html.contains("ignite-mark.png"));
    assert!(
        !html.contains("favicon.ico"),
        "the app's favicon replaces Ignite's"
    );
    assert!(!html.contains("apple-touch-icon"));

    let guest = client_with(configure);
    let html = page(&guest, "/admin/login");
    assert!(
        html.contains(logo),
        "the sign-in bar shows the logo: {html}"
    );
    assert!(html.contains(favicon));
    assert_eq!(title(&html), "Sign in · Mission Control");
}

#[test]
fn a_logo_without_a_height_takes_filaments_default() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.brand_logo("/logo.svg"));
    let html = page(&client, "/admin");
    assert!(
        html.contains(
            r#"<img src="/logo.svg" alt="Ignite" class="w-auto" style="height: 1.5rem">"#
        ),
        "{html}"
    );
}

#[test]
fn brand_text_is_escaped() {
    let _guard = reset();
    let client = signed_in_with(|ignite| ignite.brand_name("<b>Acme</b> & Co"));
    let html = page(&client, "/admin");
    assert!(html.contains("&lt;b&gt;Acme&lt;/b&gt; &amp; Co"), "{html}");
    assert!(!html.contains("<b>Acme</b>"));
}

/// The head of a page: the text between `<head>` and `</head>`.
fn head(html: &str) -> &str {
    let start = html.find("<head>").expect("a head");
    let end = html.find("</head>").expect("the end of the head");
    &html[start..end]
}

#[test]
fn a_stylesheet_appears_in_the_head_of_every_page() {
    let _guard = reset();
    let link = r#"<link rel="stylesheet" href="/mc-includes/css/dashicons.min.css">"#;
    let configure = |ignite: Ignite| ignite.stylesheet("/mc-includes/css/dashicons.min.css");

    let client = signed_in_with(configure);
    for path in [
        "/admin",
        "/admin/widgets",
        "/admin/widgets/new",
        "/admin/profile",
    ] {
        let html = page(&client, path);
        let head = head(&html);
        assert!(head.contains(link), "{path}: {html}");
        assert!(
            at(head, link) > at(head, r#"<link rel="stylesheet" href="/admin/build/"#),
            "{path}: the app's stylesheet comes after the panel's CSS"
        );
        assert!(
            head.ends_with(&format!("{link}\n")),
            "{path}: the link is at the end of the head: {head}"
        );
    }

    let guest = client_with(configure);
    for path in ["/admin/login", "/admin/password/forgot"] {
        let html = page(&guest, path);
        assert!(head(&html).contains(link), "{path}: {html}");
    }
}

#[test]
fn stylesheets_keep_the_order_they_were_added_in() {
    let _guard = reset();
    let client = signed_in_with(|ignite| {
        ignite
            .stylesheet("/css/first.css")
            .stylesheet("https://cdn.example.com/second.css")
            .stylesheet("/css/third.css")
    });
    let html = page(&client, "/admin");
    let head = head(&html);
    let first = at(head, r#"<link rel="stylesheet" href="/css/first.css">"#);
    let second = at(
        head,
        r#"<link rel="stylesheet" href="https://cdn.example.com/second.css">"#,
    );
    let third = at(head, r#"<link rel="stylesheet" href="/css/third.css">"#);
    assert!(first < second && second < third, "{head}");
}

#[test]
fn a_stylesheet_url_is_escaped() {
    let _guard = reset();
    let client =
        signed_in_with(|ignite| ignite.stylesheet(r#"/css/a.css"><script>alert(1)</script>"#));
    let html = page(&client, "/admin");
    assert!(
        head(&html).contains(
            r#"<link rel="stylesheet" href="/css/a.css&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;">"#
        ),
        "{html}"
    );
    assert!(!html.contains("<script>alert(1)"));
}

#[test]
fn no_stylesheet_is_linked_by_default() {
    let _guard = reset();
    let only_the_panels = |html: &str| {
        let head = head(html);
        assert_eq!(
            head.matches(r#"rel="stylesheet""#).count(),
            1,
            "only the panel's own CSS: {head}"
        );
    };
    let client = signed_in(None);
    only_the_panels(&page(&client, "/admin"));
    only_the_panels(&page(&client, "/admin/widgets"));

    let guest = client_with(|ignite| ignite);
    only_the_panels(&page(&guest, "/admin/login"));
}

#[test]
#[should_panic(expected = "is not a CSS length")]
fn a_logo_height_that_is_not_a_length_is_refused() {
    ensure_secret();
    std::env::set_var("DATABASE_URL", database_url());
    let _ = ignite_panel::Ignite::new().brand_logo_height("2rem;background:red");
}

// The resources of the navigation tests. The sidebar needs no rows, so their tables are never
// created: only the derive's navigation attributes matter.

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "nav_posts")]
#[ignite(
    table = nav_posts,
    title = "Posts",
    navigation_group = "Content",
    navigation_sort = 2,
    navigation_icon = "post"
)]
pub struct Post {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "nav_pages")]
#[ignite(table = nav_pages, title = "Pages", navigation_group = "Content", navigation_sort = 1)]
pub struct PageRow {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

/// The same sort as `Post`, so the label decides: "Labels" before "Posts".
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "nav_tags")]
#[ignite(
    table = nav_tags,
    title = "Tags",
    navigation_label = "Labels",
    navigation_group = "Content",
    navigation_sort = 2
)]
pub struct Tag {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "nav_orders")]
#[ignite(table = nav_orders, title = "Orders", navigation_group = "Shop")]
pub struct Order {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

/// A group the panel does not declare.
#[derive(Debug, Clone, OrbitModel, Serialize, IgniteResource)]
#[orbit(table = "nav_logs")]
#[ignite(table = nav_logs, title = "Logs", navigation_group = "Tools", navigation_sort = -1)]
pub struct Log {
    id: i32,
    #[orbit(fillable)]
    name: String,
}

#[test]
fn the_derive_sets_the_navigation_constants() {
    assert_eq!(<Post as IgniteResource>::NAVIGATION_GROUP, Some("Content"));
    assert_eq!(<Post as IgniteResource>::NAVIGATION_SORT, Some(2));
    assert_eq!(<Post as IgniteResource>::NAVIGATION_LABEL, None);
    assert_eq!(<Tag as IgniteResource>::NAVIGATION_LABEL, Some("Labels"));
    assert_eq!(<Log as IgniteResource>::NAVIGATION_SORT, Some(-1));
    assert_eq!(<Order as IgniteResource>::NAVIGATION_SORT, None);
    assert_eq!(<Post as IgniteResource>::NAVIGATION_ICON, Some("post"));
    assert_eq!(<Order as IgniteResource>::NAVIGATION_ICON, None);
}

/// The panel of the navigation tests: `Widgets` (no group) and the resources above.
fn with_groups(ignite: Ignite) -> Ignite {
    ignite
        .resource::<Post>()
        .resource::<Order>()
        .resource::<Tag>()
        .resource::<Log>()
        .resource::<PageRow>()
        .navigation_groups(["Shop"])
        .navigation_groups(["Content"])
}

/// The navigation of `html`: the `<nav>` element.
fn navigation(html: &str) -> &str {
    let start = html.find(r#"<nav class="#).expect("the navigation");
    let end = html[start..]
        .find("</nav>")
        .expect("the end of the navigation")
        + start;
    &html[start..end]
}

/// Where `needle` is in `haystack`, which must hold it.
fn at(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not in:\n{haystack}"))
}

/// The heading of the group `name`: the button that collapses it, as groups are collapsible
/// by default, as in Filament.
fn heading(name: &str) -> String {
    format!(r#"data-ig-nav-toggle>{name}<i class="ti ti-chevron-down"#)
}

/// A navigation link that ends with `label`, after its icon.
fn link(label: &str) -> String {
    format!("></i>{label}</a>")
}

/// A navigation link that ends with `label`, after the icon of the classes `classes`.
fn link_with_icon(classes: &str, label: &str) -> String {
    format!(r#"<i class="{classes} text-lg"></i>{label}</a>"#)
}

#[test]
fn navigation_groups_render_in_the_declared_order_with_headings() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let html = page(&client, "/admin");
    let nav = navigation(&html);
    let order = [
        at(nav, &link("Dashboard")),
        at(nav, &link("Widgets")),
        at(nav, &heading("Shop")),
        at(nav, &link("Orders")),
        at(nav, &heading("Content")),
        at(nav, &link("Pages")),
        // A group the panel does not declare comes after the declared ones.
        at(nav, &heading("Tools")),
        at(nav, &link("Logs")),
    ];
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "out of order: {order:?}\n{nav}"
    );
    assert_eq!(nav.matches("data-ig-nav-toggle>").count(), 3);
    assert!(!nav.contains(r#"<p class="ig-caption mt-5">"#), "{nav}");
}

#[test]
fn resources_sort_by_navigation_sort_then_label_inside_a_group() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let html = page(&client, "/admin");
    let nav = navigation(&html);
    let content = at(nav, &heading("Content"));
    let pages = at(nav, &link("Pages"));
    let labels = at(nav, &link("Labels"));
    let posts = at(nav, &link_with_icon("ti ti-pin", "Posts"));
    assert!(
        content < pages && pages < labels && labels < posts,
        "Pages (1), then Labels and Posts (2) by label:\n{nav}"
    );
    assert!(
        !nav.contains(&link("Tags")),
        "the navigation label replaces the title"
    );
    // The page keeps the resource's title.
    let html = page(&client, "/admin/nav_tags/new");
    assert!(html.contains("Tag") && navigation(&html).contains(&link("Labels")));
}

#[test]
fn a_resource_without_a_group_still_appears_before_the_groups() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let html = page(&client, "/admin/widgets");
    let nav = navigation(&html);
    let widgets = at(
        nav,
        r#"href="/admin/widgets" aria-current="page"><i class="ti ti-table text-lg"></i>Widgets</a>"#,
    );
    assert!(widgets < at(nav, &heading("Shop")));
    assert!(nav.contains(r#"href="/admin/widgets/new">Add new</a>"#));
}

/// A data source with no rows: the sidebar does not read any.
struct NoRows;

impl RuntimeData for NoRows {
    fn list<'a>(&'a self, _: &'a Database, _: &'a ListQuery) -> RuntimeFuture<'a, Page<Row>> {
        Box::pin(async {
            Ok(Page {
                rows: Vec::new(),
                total: 0,
            })
        })
    }
    fn find<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, Option<Row>> {
        Box::pin(async { Ok(None) })
    }
    fn create<'a>(&'a self, _: &'a Database, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Ok(input) })
    }
    fn update<'a>(&'a self, _: &'a Database, _: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async { Ok(input) })
    }
    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }
}

#[test]
fn a_runtime_resource_takes_its_place_in_a_group() {
    let _guard = reset();
    register_runtime(
        RuntimeResource::new("nav-notes", "Notes", "Note", NoRows)
            .navigation_group("Content")
            .navigation_sort(0)
            .navigation_label("Sticky notes")
            .navigation_icon("comment"),
    )
    .expect("register");
    let client = signed_in_with(with_groups);
    let html = page(&client, "/admin");
    unregister_runtime("nav-notes").expect("unregister");
    let nav = navigation(&html);
    let content = at(nav, &heading("Content"));
    let notes = at(nav, &link_with_icon("ti ti-message", "Sticky notes"));
    assert!(content < notes && notes < at(nav, &link("Pages")), "{nav}");
}

#[test]
fn the_ignite_theme_maps_each_icon_name_to_a_tabler_icon() {
    let _guard = reset();
    let client = signed_in_with(with_groups);
    let html = page(&client, "/admin/nav_posts/new");
    let nav = navigation(&html);
    // Posts names the icon `post`: the active entry shows the theme's icon of that name.
    assert!(
        nav.contains(r#"aria-current="page"><i class="ti ti-pin text-lg"></i>Posts</a>"#),
        "{nav}"
    );
    // The others name none: the theme's generic resource icon.
    assert!(nav.contains(&link_with_icon("ti ti-table", "Orders")));
    assert!(nav.contains(&link_with_icon("ti ti-layout-dashboard", "Dashboard")));
    // An icon is never a resource's raw classes any more.
    assert!(!nav.contains("<span "), "{nav}");
}

#[test]
fn an_icon_name_the_theme_does_not_know_shows_its_resource_icon() {
    let _guard = reset();
    register_runtime(
        RuntimeResource::new("nav-books", "Books", "Book", NoRows).navigation_icon("book"),
    )
    .expect("register");
    let client = signed_in_with(with_groups);
    let html = page(&client, "/admin");
    unregister_runtime("nav-books").expect("unregister");
    assert!(
        navigation(&html).contains(&link_with_icon("ti ti-table", "Books")),
        "{html}"
    );
}
