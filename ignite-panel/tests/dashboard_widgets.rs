//! Dashboard widgets, as Filament's stats overview widget: each card has a title (the stat's
//! label), a value and an optional description, computed by the panel (`Ignite::widget`) or by
//! a resource (`IgniteResource::stats`, `RuntimeData::stats`, or `widgets` as before), and the
//! dashboard shows them in its grid. An empty dashboard says "Nothing to show yet." Runs
//! against the test database.

mod common;

use common::*;
use ignite_core::{
    register_runtime, unregister_runtime, ActingUser, Database, ListQuery, Page, Row, RuntimeData,
    RuntimeFuture, RuntimeResource, Stat, StatsOverviewWidget,
};
use ignite_panel::Ignite;
use rocket::http::Status;
use rocket::local::blocking::Client;

fn dashboard(client: &Client) -> String {
    let response = client.get("/admin").dispatch();
    assert_eq!(response.status(), Status::Ok);
    response.into_string().unwrap()
}

fn card(label: &str, value: &str) -> String {
    format!(
        "<p class=\"m-0 font-semibold\">{label}</p>\n        <p class=\"m-0 text-2xl font-semibold\">{value}</p>"
    )
}

/// The panel's own widget: two cards, one with a description, both naming the user.
struct SiteStats;

impl StatsOverviewWidget for SiteStats {
    fn stats<'a>(&'a self, _: &'a Database, user: &'a ActingUser) -> RuntimeFuture<'a, Vec<Stat>> {
        let name = user.name().to_string();
        Box::pin(async move {
            Ok(vec![
                Stat::make("Comments", 7).description(format!("awaiting {name}")),
                Stat::make("<Visits> & more", "1.2k"),
            ])
        })
    }
}

/// A widget no one may view.
struct Hidden;

impl StatsOverviewWidget for Hidden {
    fn stats<'a>(&'a self, _: &'a Database, _: &'a ActingUser) -> RuntimeFuture<'a, Vec<Stat>> {
        Box::pin(async { Ok(vec![Stat::make("Secret", 1)]) })
    }

    fn can_view(&self, _: &ActingUser) -> bool {
        false
    }
}

#[test]
fn the_panels_widgets_show_first_with_their_descriptions() {
    let _guard = reset();
    let client = signed_in_with(|ignite: Ignite| ignite.widget(SiteStats).widget(Hidden));
    let html = dashboard(&client);
    let comments = html.find(&card("Comments", "7")).expect("the panel's card");
    assert!(
        html[comments..].starts_with(&format!(
            "{}\n        <p class=\"m-0 text-sm\">awaiting Admin</p>",
            card("Comments", "7")
        )),
        "{html}"
    );
    assert!(
        html.contains(&card("&lt;Visits&gt; &amp; more", "1.2k")),
        "{html}"
    );
    // The `Widgets` resource's card (its `widgets`, as before) comes after the panel's.
    let widgets = html
        .find(&card("Widgets", "3"))
        .expect("the resource's card");
    assert!(comments < widgets, "{html}");
    assert!(!html.contains("Secret"), "can_view hides a widget");
    assert!(!html.contains("Nothing to show yet."));
}

/// Posts whose data source gives the dashboard a card.
struct Posts;

impl RuntimeData for Posts {
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
        Box::pin(async move { Ok(input) })
    }

    fn update<'a>(&'a self, _: &'a Database, _: i64, input: Row) -> RuntimeFuture<'a, Row> {
        Box::pin(async move { Ok(input) })
    }

    fn delete<'a>(&'a self, _: &'a Database, _: i64) -> RuntimeFuture<'a, u64> {
        Box::pin(async { Ok(0) })
    }

    fn stats<'a>(&'a self, _: &'a Database, _: &'a ActingUser) -> RuntimeFuture<'a, Vec<Stat>> {
        Box::pin(async { Ok(vec![Stat::make("Posts", 12).description("3 drafts")]) })
    }
}

#[test]
fn a_runtime_resource_gives_its_cards() {
    let _guard = reset();
    let _ = unregister_runtime("dw-posts");
    register_runtime(RuntimeResource::new("dw-posts", "Posts", "Post", Posts)).expect("register");
    let client = signed_in(None);
    let html = dashboard(&client);
    unregister_runtime("dw-posts").expect("unregister");
    assert!(
        html.contains(&format!(
            "{}\n        <p class=\"m-0 text-sm\">3 drafts</p>",
            card("Posts", "12")
        )),
        "{html}"
    );
}

#[test]
fn a_stat_widget_becomes_a_stat() {
    let stat = Stat::from(ignite_core::StatWidget {
        label: "Users",
        value: 4,
    });
    assert_eq!(stat.get_label(), "Users");
    assert_eq!(stat.get_value(), "4");
    assert_eq!(stat.get_description(), None);
}

#[test]
fn an_empty_dashboard_says_nothing_to_show_yet() {
    let _guard = reset();
    // A user who may view no resource sees no card.
    let t = test_db();
    t.run(async {
        ignite_panel::users::create_user(&t.db, "Viewer", "viewer@example.test", PASSWORD)
            .await
            .expect("user");
    });
    let client = signed_in_as(None, "viewer@example.test", PASSWORD);
    let html = dashboard(&client);
    assert!(
        html.contains(r#"<p class="col-span-full">Nothing to show yet.</p>"#),
        "{html}"
    );
    assert!(!html.contains("No stats yet"));
    assert!(!html.contains("implementing"));
}
