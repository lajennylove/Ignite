# Ignite

An admin panel for [Rocket.rs](https://rocket.rs). Ignite builds a back office for a MariaDB or MySQL table: a sign-in page, a dashboard, a profile page, and screens to list, search, sort, create, edit and delete its rows. It renders with [Propel](https://github.com/lajennylove/Propel) views and talks to the database through [Orbit](https://github.com/lajennylove/Orbit), an ORM for Rocket.

Documentation: [ignite.rocketboost.rs](https://ignite.rocketboost.rs). Changes between releases are in [CHANGELOG.md](CHANGELOG.md). Ignite is released under the [MIT License](LICENSE).

## Install

Ignite is on crates.io. Orbit, Rocket and Propel come from crates.io too, so the app builds one copy of each crate:

```toml
[dependencies]
ignite-panel = "0.6.6"
ignite-core = "0.6.6"
orbit-orm = "0.4.0"
rocket = "0.5.2"
propel_rocket = "0.3.0"
propel_runtime = "0.3.0"
serde = { version = "1", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
dotenvy = "0.15"

[build-dependencies]
propel_compiler = "0.3.0"
```

- **Ignite** 0.6.6: `ignite-panel`, `ignite-core` and `ignite-macros` are released together, at the same version. Upgrading from 0.6 has breaking changes (icon names, the admin theme trait, the list query's `filter`, collapsible navigation groups by default): see the migration notes in [CHANGELOG.md](CHANGELOG.md).
- **Orbit** (`orbit-orm`) 0.4.0. The models derive its `OrbitModel`.
- **Rocket** 0.5.2 and **Propel** 0.3.0. Use the same versions as the panel, or Cargo builds a second copy and the panel's views would not match the app's.
- **propel_compiler** 0.3.0 is a build dependency. An app with its own views compiles them in its `build.rs`.
- **chrono** is needed for the date fields of a resource, and **dotenvy** for the `ignite` command.

### Build requirements

- Rust (stable).
- Node.js is needed only to change the panel's own interface (its CSS and JavaScript under `ignite-panel/ui`). The built files are in the repository, so building and running an app does not need Node.

## Configure

Ignite reads its settings from the environment. When a `.env` file is in the working directory, it is loaded first. Variables already set in the environment win over the file, as in Laravel. A missing `.env` is fine. A `.env` that exists but cannot be parsed stops the app with an error that names the file. Keep `.env` files and secrets out of git.

| Variable | Required | What it does |
| --- | --- | --- |
| `DATABASE_URL` | Yes | `mysql://user:password@host:port/database`. |
| `IGNITE_SECRET_KEY` | Yes in a release build | Signs the sign-in session and the flash messages. Generate one with `openssl rand -base64 32`. A debug build makes a random key for each run if it is unset, so every restart signs everyone out. A release build refuses to start without a valid key. |
| `IGNITE_TRUSTED_PROXIES` | No | Comma-separated IP addresses and CIDR ranges of the proxies in front of the app, such as `10.0.0.0/8, 192.0.2.10`. The panel believes their `X-Forwarded-For` and `X-Forwarded-Proto` headers. See below. |
| `IGNITE_VIEWS_DIR` | No | Debug builds only: reads the panel's views from this directory, so they can be edited without a rebuild. |
| `IGNITE_APP_URL` | No | The app's own URL, such as `https://app.example.test`. Password reset links are built from it, as Laravel builds them from `APP_URL`. Without it, the links use the scheme and host of the request, which a client can set, so set it in production. |
| `EMAILIT_API_KEY` | Only for Emailit | The Emailit API key, read by `EmailitMailer::from_env`. It is never logged. |
| `IGNITE_MAIL_FROM` | Only for Emailit | The sender, such as `Ignite <hello@mail.example.test>`. The domain must be verified in Emailit. |

### Client addresses and proxies

The login throttle counts failed sign-ins per email and per client address. The client address is the socket address of the connection. Forwarded headers are believed only when the socket address is listed in `IGNITE_TRUSTED_PROXIES`. Then the panel uses the right-most `X-Forwarded-For` entry that is not itself a trusted proxy. `X-Real-IP` is never read. If the socket address is unknown, the panel counts those requests together under the key `unknown`.

The same rule decides whether a request is secure. The panel's cookies get the `Secure` flag when the request was made over HTTPS: when every listener of the server is TLS, or when the socket is a trusted proxy whose right-most `X-Forwarded-Proto` is `https`. A plain HTTP request, such as a local development server, gets cookies without the flag, so the browser keeps them. `X-Forwarded-Host` is not read.

Rocket does not report, for a single request, which listener it arrived on. A server with both an HTTP and an HTTPS listener is therefore treated as not direct TLS. Behind a proxy, that case is handled by `X-Forwarded-Proto`.

## Deploy and start

Migrations are a deploy step. The panel does not migrate when the app starts, so two instances never race on the schema. Run the migrations before the first start, and again after each upgrade:

```sh
ignite migrate
```

The command prints each migration it applies, or `Nothing to migrate.`. It exits with status 1 on failure. `ignite user create` refuses to run while migrations are pending, and says to run `ignite migrate` first.

To see where the database is:

```sh
ignite migrate:status
```

lists each migration as `Applied` or `Pending`.

To undo the last batch, run `ignite migrate:rollback`. A batch is the set of migrations that one `ignite migrate` applied, so a run reverts all of them, last first, each with its down migration. After a `migrate` that applied one migration, a rollback reverts that one. It is not one step per run: when a batch has several migrations, one run reverts all of them. It says so in its output, and changes nothing else.

**Batch 0.** The panel records its migrations in the `__diesel_schema_migrations` table. A database that was migrated before the move to Orbit has its earlier migrations there as batch 0. They are not run again. A rollback that reaches batch 0 reverts those migrations too, which drops the panel's tables. Back up the database before you roll back that far.

**If a migration fails.** MySQL and MariaDB commit each schema statement as it runs, and a failed migration does not roll back the statements before it. A failed `ignite migrate` can therefore leave the database part way through a migration. To recover:

1. Read the error. Fix its cause, such as a duplicate value or a missing column the migration needs.
2. Run `ignite migrate:status` to see which migrations are recorded as applied.
3. Run `ignite migrate` again. A migration that was only partly applied can need its partial change removed first. Check the database by hand, and use `ignite migrate:rollback` when the last migration is the one that failed and its down migration undoes what it did.

Take a database backup before each deploy that migrates.

To get the `ignite` command in your app, add a small binary that loads `.env` and calls the panel's command line:

```rust
// src/bin/ignite.rs
fn main() {
    let _ = dotenvy::dotenv();
    ignite_panel::cli::main();
}
```

An app that registers user callbacks (see [User lifecycle callbacks](#user-lifecycle-callbacks)) runs the command through its builder instead, so `ignite user create` and `ignite user delete` run them too. `Ignite::new` loads `.env` itself:

```rust
// src/bin/ignite.rs
fn main() {
    Ignite::new().on_user_created(create_profile).run_cli();
}
```

Create the first admin with the command. It asks for a password twice without echoing it, or reads it from standard input with `--password-stdin`:

```sh
cargo run --bin ignite -- user create --email you@example.com --name "You"
```

Attach the panel to your Rocket app, with a resource for each table you want to manage:

```rust
rocket::build().attach(
    Ignite::new()
        .resource::<Users>()
        .fairing(),
)
```

The panel is served under `/admin`. Its health check is `GET /up`, at the application root. It answers with a minimal HTML page, "Application up", and needs no sign-in. The documentation explains how to define a resource, its fields and columns, the profile fields an app adds, and how to extend the layout.

### Storage directory

The panel keeps its files on disk, in a storage directory: `storage` in the working directory, as Laravel's `storage/`, or the path given to `Ignite::storage_dir`. The users' profile pictures are in its `avatars` folder, one file per user named from the user's id, such as `avatars/42.png`; the `user_avatars` table keeps only that path, relative to the storage directory, and `users.avatar_mime` keeps the picture's type, as WordPress keeps its uploads. The panel serves a picture at `/admin/profile/avatar`. Replacing a picture with one of another type, removing it, and deleting the user remove the old file. The folders are created when the first picture is saved.

Keep the storage directory out of git, give the app's user write access to it, and keep it across deploys (include it in backups). An app that sets `storage_dir` and runs its command through its builder (`run_cli`) has `ignite user delete` remove pictures from the same directory. The setting is process-wide, as the user callbacks are: the most recently configured `Ignite` sets it. The stock `ignite` command uses `storage` in its working directory.

## Resources

A resource is an Orbit model with `#[derive(OrbitModel)]` and `#[derive(IgniteResource)]`. The model also derives `Serialize`. The derive writes the form, the list and the queries. Its `#[ignite(table = ...)]` must name the table of the model's `#[orbit(table = ...)]`, which is checked at compile time. Each writable field needs `#[orbit(fillable)]`, because Orbit refuses to insert or update a column that is not fillable. Each field's type sets its input and its check:

| Rust type | Input | Checked as |
| --- | --- | --- |
| `String` | text, or `textarea`, `password` by attribute | any text, required unless `Option` |
| `i16`, `i32`, `i64` | number | a whole number in the type's range |
| `f32`, `f64` | number, with `step = "0.01"` (default `any`) | a finite number |
| `bool` | checkbox | ticked or not |
| `chrono::NaiveDate` | date | `YYYY-MM-DD` |
| `chrono::NaiveDateTime` | date and time | `YYYY-MM-DD HH:MM`, with seconds if given |

A select is a `String` field with a list of choices, or a field whose choices come from the database:

```rust
#[ignite(options = [("draft", "Draft"), ("published", "Published")])]
status: String,

#[ignite(options = load_categories)]
tag: Option<String>,

#[ignite(belongs_to = Category, display = name)]
category_id: Option<i32>,
```

`load_categories` is an async function `async fn(&Database) -> Result<Vec<(String, String)>, orbit_orm::Error>`, called each time a form is shown. A `belongs_to` field stores the related row's `id`. Its select lists the rows of the related table by their `display` column, and the list shows that name. A submitted value must be one of the choices, so a value with no row is refused. The derive does not write a foreign key: add one in the migration if the database should enforce it.

### Saving a form

Create and update write the fields of the form. A blank `Option` field is saved as NULL, on edit as on create, so a user clears an optional value by emptying it and saving (since 0.3.0; before, a blank optional field was skipped on edit and kept its old value). An `Option` field that the request does not send at all is left out of an update, so its stored value is kept; `FormInput::has` tells a field sent blank from one not sent. A password is the exception: left blank on edit, it keeps the stored hash. A field that is not an `Option` is required, and a blank one is refused with "<label> is required." on create and on edit. A checkbox is never blank: unticked, or not sent, is `false`.

The errors of a form are a `FieldErrors`, a `HashMap<String, String>` from the field name to its message, as Laravel's `MessageBag` keys messages by field. A resource that implements `IgniteResource` by hand writes literal keys with `.to_string()`:

```rust
fn validate(form: &FormInput) -> Result<WidgetInput, FieldErrors> {
    let mut errors = FieldErrors::new();
    let price = match form.get("price").map(str::parse::<i32>) {
        Some(Ok(price)) => price,
        _ => {
            errors.insert("price".to_string(), "Price must be a whole number.".to_string());
            0
        }
    };
    // ...
}
```

An app that builds a derived input (`<Model>Input`) itself, with a struct literal, sets its hidden `__ignite_sent` field to the optional fields to write: `__ignite_sent: Vec::new()` writes none of the `None` ones on update, so their stored values are kept, and `vec!["note"]` writes NULL for `note`. `validate` and `validate_update` fill it from the form.

The checks on `Field` (`check_required`, `check_whole_number`, `check_decimal`, `check_date` and the others) record their message under the field's name, so most resources never write a key by hand.

### Runtime resources

A resource can also be defined by data while the app runs, such as a custom post type that a site owner creates in the admin. Build a `RuntimeResource` from its slug, titles, `FieldDef`s and `ColumnDef`s (owned twins of `Field` and `Column`; `FieldDef::from(field)` converts a static one), give it a `RuntimeData` that reads and writes its rows as JSON maps, and add it with `register_runtime`. Definitions stay owned: editing and registering a resource again keeps no copy of the old one, and removing it frees everything, field names included (its form errors are keyed by owned `String`s). A field name must be a lowercase identifier: lowercase letters, digits and `_`, starting with a letter, at most 64 bytes, as a column name is. `register_runtime` refuses any other name with `RuntimeError::InvalidFieldName`, and two fields with one name with `RuntimeError::DuplicateField`.

```rust
use ignite_core::{register_runtime, unregister_runtime, ColumnDef, FieldDef, FieldKindDef, RuntimeResource};

register_runtime(
    RuntimeResource::new("books", "Books", "Book", BookData)
        .field(FieldDef { name: "title".into(), label: "Title".into(), kind: FieldKindDef::Text, required: true })
        .column(ColumnDef { key: "title".into(), label: "Title".into(), sortable: true, searchable: true }),
)?;
unregister_runtime("books")?;
```

The panel serves it at `/admin/<slug>` with the same list, forms, checks and delete action as a static resource, and shows it in the navigation, from the next request on. Ignite assumes no table layout: `RuntimeData` decides where the rows live. A row needs a numeric `id`. A slug that is not valid is refused with `RuntimeError::InvalidSlug`, and a slug that a static resource, another runtime resource or a panel page (such as `login`) uses with `RuntimeError::SlugTaken`; `unregister_runtime` of a slug that is not registered returns `RuntimeError::NotRegistered`. On edit, an optional field sent blank comes to `RuntimeData::update` as `null`, and one the form did not send and a blank password are left out of the row, as for a derived resource. Call `ignite_panel::register_runtime_permissions(&db, "books").await?` right after `register_runtime` (it refuses a slug that is not registered), so its five permissions exist at once and a role can be granted them; without the call, the panel creates them on the next signed-in request. They stay when the resource is removed. A unique-index violation shows as "<label> is already taken." only when the index is named `{slug}_{field}_unique`, as for a static resource; a `RuntimeData` that stores values elsewhere should check uniqueness itself. A runtime resource has no dashboard cards or table yet.

### Conditional fields

A field can show only when a condition holds, as Filament's `visible()`, `hidden()`, `visibleOn()`, `hiddenOn()` and `live()`. A condition is a closure the server runs when it builds and when it saves the form, given a `FormContext`: `operation()` (`Operation::Create` or `Operation::Edit`), `record()` (the stored row, `None` on create), `user()` (the `ActingUser`) and `get(name)` (a field's current value, as Filament's `$get`). A hidden field is not rendered, not checked (a required one included) and not written: a request that sends it anyway leaves the stored value as it is. A field marked `live` sends the form to the server when it changes in the browser, and the panel's script shows the fields the server renders for the new values.

```rust
RuntimeResource::new("posts", "Posts", "Post", PostData)
    .field(kind)
    .field(company)
    .live("kind")
    .visible("company", |form| form.get("kind") == Some("business"))
    .hidden_on("slug", &[Operation::Create])
    .visible("featured", |form| form.user().can("posts.feature"));
```

A static resource returns the same `FieldConditions` from `IgniteResource::conditions`, and the derive takes `#[ignite(conditions = post_conditions)]`, a `fn() -> FieldConditions`. The panel removes a hidden field from the form before `validate`, so a field that may be hidden must be one `validate` accepts as missing, such as an `Option` field of the derive.

### Form layout

A form can be laid out in columns, as Filament's `Grid`, `Section`, `Group`, `columns()` and `columnSpan()`. A WordPress-like editor puts the content in a main column two thirds wide and stacks its settings in a sidebar:

```rust
use ignite_core::{FormLayout, Group, Section, Width};

RuntimeResource::new("posts", "Posts", "Post", PostData)
    // ...fields...
    .slug_from("slug", "title")
    .form_layout(FormLayout::new().columns(3).schema([
        Group::make([Section::make("Content").schema(["title", "slug", "body"])]).column_span(2),
        Group::make([
            Section::make("Publish").schema(["status", "published_at"]),
            Section::make("Category").schema(["category"]),
        ])
        .column_span(1),
    ]));
```

Each section is a card; `columns` and `column_span` apply from the `lg` breakpoint, and below it the cards stack in one column. `column_span_full()` takes every column. `max_content_width(Width::Full)` lets a settings form use the whole width beside the sidebar instead of the panel's 1536px. A field the layout does not place shows after the others in the first column. A form without a layout renders as before, one card of fields. A static resource returns the layout from `IgniteResource::form_layout`, and the derive takes `#[ignite(form_layout = post_layout)]`.

`slug_from("slug", "title")` (on `RuntimeResource` and `FieldConditions`) fills the slug with the title's slug as the user types, until the user edits the slug by hand, as Filament's documented slug example (`afterStateUpdated` with `Str::slug`). The server still checks the value in `validate`.

`slug_locked_when("slug", rule)` stops the slug from following the title on the edit form of a record that has a stored slug, when `rule` (given the `FormContext`) holds, such as once the stored post is published, so a published URL does not change with its title. On create, on drafts and for a record without a slug yet, the slug follows the title as before. Filament's example has no such rule.

```rust
.slug_from("slug", "title")
.slug_locked_when("slug", |form| {
    form.record().and_then(|r| r.get("status")).and_then(|s| s.as_str()) == Some("publish")
})
```

### List tabs and badges

A list can have tabs, as Filament's `getTabs()` (`RuntimeResource::tab(Tab::make("drafts").where_eq("status", "draft"))`, or `IgniteResource::tabs`), and badge columns, as Filament's `badge()` and `color()` (`badge`, `badge_color`). Each tab shows the number of all its rows, whatever the search, as a Filament tab's badge counts its own query; `tab_counts_follow_search(true)` counts the rows that match the search instead. A tab whose count is 0 is left out, except the default tab and the tab being shown; `hide_empty_tabs(false)` renders every tab. A badge can show inside another column's cell, and only on some tabs, decided on the server:

```rust
RuntimeResource::new("posts", "Posts", "Post", PostData)
    .tab(Tab::make("all"))
    .tab(Tab::make("drafts").where_eq("status", "draft"))
    .badge_color("status", |state| if state == "draft" { BadgeColor::Gray } else { BadgeColor::Success })
    .badge_inside("status", "title") // after the title, with no Status column
    .badge_on_tabs("status", ["all"]); // on All only
```

The derive takes `#[ignite(tab_counts_follow_search = true)]` and `#[ignite(hide_empty_tabs = false)]`, and `ColumnBadges` has `badge_inside` and `badge_on_tabs`.

`Ignite::unsaved_changes_alerts()`, as Filament's panel option of the same name, asks "Are you sure you want to leave? Without saving, everything is going to be lost." when the user leaves a create or edit form they changed and did not save. It is off by default, as in Filament.

## Brand and navigation

The panel's brand and its sidebar are options of the panel and of each resource, as Filament's panel and resources set them. They are compiled into the panel's views, so they work in a release build too; an app does not copy the views to change them.

```rust
Ignite::new()
    .brand_name("Mission Control")
    .brand_logo("/images/logo.webp")
    .brand_logo_height("2rem")
    .favicon("/images/favicon.png")
    .navigation_groups(["Content", "Settings"])
    .resource::<Posts>()
```

- `brand_name` (Filament's `brandName`) is the text at the top of the sidebar and in the sign-in bar, and the end of each page title (`Posts · Mission Control`). The default is `Ignite` beside the Ignite mark; a name set here shows without the mark.
- `brand_logo` (`brandLogo`) is the path or URL of an image, shown in the sidebar and the sign-in bar in place of the name, with the name as its alternative text, as in Filament. `brand_logo_height` (`brandLogoHeight`) is a CSS length; the default is Filament's `1.5rem`.
- `favicon` (`favicon`) is the path or URL of the icon. It replaces Ignite's favicon and touch icon.
- The app serves the image files. Without these calls the pages look exactly as before.

A resource places itself in the sidebar with struct attributes, as Filament's `$navigationGroup`, `$navigationSort`, `$navigationLabel` and `$navigationIcon`:

```rust
#[ignite(
    table = posts,
    navigation_group = "Content",
    navigation_sort = 1,
    navigation_label = "All posts",
    navigation_icon = "post"
)]
```

A resource implemented by hand sets `NAVIGATION_GROUP`, `NAVIGATION_SORT`, `NAVIGATION_LABEL` and `NAVIGATION_ICON`, and a `RuntimeResource` the methods of the same names. The sidebar lists the dashboard and the resources without a group first, as they were listed before, then each group under its name as a heading: the groups in the order of `navigation_groups` (each call adds to the list, as in Filament), then any group not listed there. Inside a group, resources are sorted by `navigation_sort`, lower first, then by label; a resource without a sort comes before those with one. `navigation_label` changes the sidebar's text only; the pages keep the resource's title.

A resource is listed under another entry with `navigation_parent_item` (Filament's `$navigationParentItem`), by the parent's label or slug, in the same group. A parent that is only a label, with no screen of its own, is declared on the panel with `navigation_items` (Filament's `navigationItems()`); clicking it opens its first child, as a Filament cluster's navigation item does:

```rust
Ignite::new()
    .navigation_groups(["Content", "Settings"])
    .navigation_items([NavigationItem::make("Settings").group("Settings").sort(1).icon("settings")])
// General, Writing, Reading...: navigation_group "Settings", navigation_parent_item "Settings",
// navigation_sort 1, 2, 3...: "Settings" links to General and lists the others under it.
```

A `NavigationItem` has `group`, `sort`, `icon` and an optional `url`: with one it is a plain link, as Filament's custom navigation items; without one it links to its first child by sort, and is not listed when the user may see none of its children.

`navigation_icon` is an icon name that says what the resource is, not a CSS class: `post`, `page`, `media`, `comment`, `appearance`, `plugins`, `users`, `tools`, `settings`, or a name of the app's own, such as a custom post type's `book`. The active admin theme maps the name to its own icon (see [Admin themes](#admin-themes)): the `ignite` theme to a Tabler icon from the font it ships, a WordPress-like theme to a Dashicon. A name the theme does not know, and a resource without a name, show the theme's generic resource icon. Filament's navigation items also take an icon name, which the panel resolves through its icon set; in Ignite the admin theme resolves it.

### Stylesheets

`stylesheet` adds an app stylesheet to every page of the panel, signed-in and sign-in pages alike, as Filament registers an app's CSS with `FilamentAsset::register`. Each call adds one, in order. The layout links them at the end of the head, after the panel's own CSS, so their rules can change the panel's look. The links are compiled into the panel's views, so they work in a release build.

```rust
Ignite::new()
    .stylesheet("/mc-admin/css/admin.css")
```

The value is the path or URL of the file, printed escaped in the link's `href`; the app serves the file. Without the call the pages link only the panel's CSS, as before. An admin theme's layout that has its own head yields the `ignite_stylesheets` section to show them. These stylesheets load in every admin theme; a stylesheet that only one theme needs, such as its icon font, belongs in the theme's `stylesheets` (see [Admin themes](#admin-themes)).

### Admin themes

An admin theme decides how the panel looks: the layout, the navigation, the top bar, the flash message, the colour tokens and the icons. Every theme renders the same screens with the same forms and links, as a Filament panel theme replaces the CSS and the views but not the pages' behaviour. The panel ships the `ignite` theme. An app implements `ignite_panel::theme::AdminTheme`, registers the theme and chooses it by name:

```rust
ignite_panel::register_admin_theme(WpTheme)?;
ignite_panel::set_admin_theme("wp")?;
```

`set_admin_theme` sets the site's default theme. Each signed-in user can choose their own admin theme on the profile page (the card shows when more than one theme is registered); the choice is stored in the user's `admin_theme` column and applies to that user's requests only. A user without a choice, a stored name that no registered theme has, and the sign-in pages get the site default. The admin theme is separate from the light or dark mode (the top bar's switch and the `theme` column): changing one keeps the other. An app reads the theme of a user with `AdminUser::admin_theme()` (the stored choice is `admin_theme_choice()`), and renders its own view that extends `ignite::layouts.app` in it with `ignite_panel::with_admin_theme(&user.admin_theme(), || PropelView::render(...))`; without that, an app's view renders in the site default.

Every registered theme renders in debug and release builds. A theme's own views come from `AdminTheme::view`, so it can ship them in the binary. In a release build the `ignite` theme renders from the views compiled into the panel, and another theme through Propel's runtime renderer, from the panel's views embedded in the binary.

A theme's own stylesheets (`stylesheets()`, the URLs of files the app serves) load only on the pages that render in that theme, after the panel's CSS and before the app-wide stylesheets of `Ignite::stylesheet`, so the Dashicons font of a WordPress-like theme is not loaded in the `ignite` theme. A layout that has its own head yields the `ignite_stylesheets` section to show them.

Icons are names (see `navigation_icon` above). A theme maps each name to its classes with `icon(name)`, which returns `None` for a name it does not know, and gives its generic resource icon with `resource_icon()`. The panel names the dashboard's entry `dashboard`. `ignite_panel::theme::icons` lists the names the panel and Mission Control use, and `IGNITE_ICONS` the `ignite` theme's Tabler icons:

| Name | `ignite` theme |
| --- | --- |
| `dashboard` | `ti ti-layout-dashboard` |
| `post` | `ti ti-pin` |
| `page` | `ti ti-file-text` |
| `media` | `ti ti-photo` |
| `comment` | `ti ti-message` |
| `appearance` | `ti ti-brush` |
| `plugins` | `ti ti-plug` |
| `users` | `ti ti-users` |
| `tools` | `ti ti-tool` |
| `settings` | `ti ti-settings` |
| any other name, or none | `ti ti-table` |

```rust
impl AdminTheme for WpTheme {
    fn name(&self) -> &str { "wp" }
    fn layout(&self) -> &str { "themes.wp.layout" }
    fn icon(&self, name: &str) -> Option<String> {
        let icon = match name {
            "dashboard" => "dashboard",
            "post" => "admin-post",
            "media" => "admin-media",
            "page" => "admin-page",
            _ => return None,
        };
        Some(format!("dashicons dashicons-{icon}"))
    }
    fn resource_icon(&self) -> String { "dashicons dashicons-admin-generic".into() }
    fn stylesheets(&self) -> Vec<String> {
        vec!["/mc-includes/css/dashicons.min.css".into(), "/mc-admin/css/wp.css".into()]
    }
    fn view(&self, name: &str) -> Option<String> { /* the theme's layout and partials */ None }
}
```


The panel follows Filament and its standard plugin, Filament Shield (built on Spatie laravel-permission). Each resource has five abilities, named after its slug:

| Ability | Lets the user |
| --- | --- |
| `<slug>.viewAny` | See the list, and the resource in the navigation and on the dashboard. |
| `<slug>.view` | Reserved for a detail page. The panel has none yet, so nothing checks it. |
| `<slug>.create` | Open the new form and save a new record. |
| `<slug>.update` | Open the edit form and save a change. |
| `<slug>.delete` | Delete a record. |

A resource with the slug `users` has `users.viewAny`, `users.view`, `users.create`, `users.update` and `users.delete`. The panel registers them in the `permissions` table when the app starts, so declare the resources before the first start, and start the app once after you add one.

A user holds the abilities of their roles. Manage them with the `ignite` command:

```sh
ignite role create editors
ignite role grant editors users.viewAny
ignite role grant editors users.update
ignite user role add editor@example.com editors
ignite user roles editor@example.com
ignite user role remove editor@example.com editors
```

- `super-admin` is a role that passes every check, as Filament Shield's super admin does.
- The migration that adds roles gives `super-admin` to every user who exists when it runs, so an upgrade locks nobody out. A user created later has no roles until `ignite user role add` gives one. The last super-admin cannot lose the role.
- `role grant` refuses a permission that the app has not registered yet. It also refuses an unknown role.
- The server answers 403 Forbidden to a request the user may not make, and an unknown resource stays 404. The navigation lists only the resources the user may view, the "Add new" link and the New button need `create`, the Edit button needs `update`, and the Delete button needs `delete`. The dashboard shows a resource's stat cards and table only when the user may view it.
- An app's own route can check an ability: `user.can("reports.view")`, where `user` is an `AdminUser` request guard.

The panel does not edit roles yet. Use the command.

### Policies and the acting user

An ability opens a resource to a role. Which records a user sees, and what they may do with each one, is decided per user and per record, as a Filament resource does with `getEloquentQuery()` and a Laravel policy:

- The panel passes the signed-in user, an `ActingUser` (id, name, email, `can(ability)`, `is_super_admin()`), to the resource's queries: the `list_for`, `find_for`, `create_for`, `update_for`, `delete_for` and `options_for` methods of `RuntimeData` and `IgniteResource`. Override `list_for` and `find_for` to restrict the records to the user, such as an author who sees only their own posts. A record that `find_for` does not return is 404 on the edit form, its save and the delete. `options_for` gives the user's choices of a select, so a choice left out cannot be saved. By default each one calls the method without the user, so a resource that does not override them behaves as before.
- A `Policy` decides `view_any`, `view`, `create`, `update` and `delete` for the user and, for the last three, the record. Register it for the resource's slug with `register_policy("posts", PostPolicy)`, as `Gate::policy` does; `unregister_policy` removes it. Each method allows by default. An action needs the ability and the policy, as a policy that Filament Shield generates checks the permission first; a super-admin passes every policy. A refusal is 403, and the navigation entry, the New button and each record's Edit and Delete buttons show only what the policy allows. The panel has no detail page yet, so nothing asks `view`.

```rust
use ignite_core::{register_policy, ActingUser, Database, Policy, Row, RuntimeFuture};

struct PostPolicy;

impl Policy for PostPolicy {
    fn update<'a>(&'a self, _: &'a Database, user: &'a ActingUser, post: &'a Row) -> RuntimeFuture<'a, bool> {
        let own = post.get("author_id").and_then(|id| id.as_i64()) == Some(user.id());
        Box::pin(async move { Ok(own) })
    }
}

register_policy("posts", PostPolicy);
```

An app route gets the same user from the guard: `admin_user.acting_user()`.

## Account lifecycle

Manage a user's account with the `ignite` command:

```sh
ignite user disable editor@example.com
ignite user enable editor@example.com
ignite user password editor@example.com
ignite user delete editor@example.com
```

- A disabled user cannot sign in. They get the same "The email or password is incorrect." message as a wrong password, which does not reveal that the account exists, and the attempt counts against the sign-in throttle. Disabling ends the user's sessions at once, so a signed-in browser is sent to the login page on its next request. Enabling restores sign-in, and the old sessions stay ended.
- The last enabled super-admin cannot be disabled, and a disabled super-admin does not count as one who keeps the panel open, so the role cannot be removed from the last enabled super-admin either.
- `ignite user password` reads the new password the way `ignite user create` does: asked for twice in a terminal, or from standard input with `--password-stdin`. It ends the user's sessions, as the panel's change-password form does for other devices.
- `ignite user delete` deletes a user. The last enabled super-admin cannot be deleted. An app's resource deletes a user with `users::delete_user(&db, id)`.
- Deleting a user deletes their session rows in the same transaction, before the user's row, so they go on every database, SQLite included, where the sessions table has no foreign key to the users. Their picture row and role rows go through the foreign keys, and their picture file is removed from the storage directory after the delete commits. The sign-in throttle rows are keyed by email and address rather than by user, and they expire after one minute, so they need no cleanup.

### User lifecycle callbacks

An app that keeps its own rows for each user, such as a profile, registers callbacks on the builder. They run each time Ignite creates or deletes a user: from the panel (an app resource that calls `users::create_user` or `users::delete_user`), from `ignite user create` and `ignite user delete` run through `Ignite::run_cli`, and from a direct call to those functions. The password reset does not create users.

```rust
use orbit_orm::raw;

Ignite::new()
    .on_user_created(|user_id, tx| {
        Box::pin(async move {
            raw::execute(&mut *tx, "INSERT INTO profiles (user_id) VALUES (?)", &[user_id.into()])
                .await?;
            Ok(())
        })
    })
    .on_user_deleting(|user_id, tx| {
        Box::pin(async move {
            raw::execute(&mut *tx, "DELETE FROM profiles WHERE user_id = ?", &[user_id.into()])
                .await?;
            Ok(())
        })
    })
```

- A callback is given the user's id and the open transaction that writes the user, so the app's rows are written or removed atomically with the user's row. It returns `Result<(), orbit_orm::Error>`.
- `on_user_created` callbacks run after the user's row is inserted. A delete runs, in one transaction, the `on_user_deleting` callbacks while the row still exists (they can read the user and remove or reassign rows whose foreign key would refuse the delete, as WordPress reassigns posts), then the delete, then the `on_user_deleted` callbacks, when the row and the rows whose foreign key cascades are gone. They follow Laravel's `deleting` and `deleted` events and WordPress's `delete_user` and `deleted_user` actions.
- Several callbacks run in the order they were registered. When one returns an error, the later ones do not run, the whole change is rolled back (a failed create leaves no user row, a failed delete keeps the user), and the error goes to the caller: the panel answers with its error page and logs the error, and the command exits with status 1 and prints it.
- The callbacks are process-wide, as Laravel's model events are. They take effect when `fairing()` or `run_cli()` is called on the builder, and replace the ones an earlier builder set, so the most recently configured `Ignite`'s callbacks are the ones that run. `users::flush_callbacks()` removes them, as `Model::flushEventListeners` does; a test suite calls it before creating its own users. The stock `ignite` binary, and `ignite_panel::cli::main`, run none.

## Mail and password reset

The panel sends email through a mailer that the app chooses. Until it does, the panel has no mailer, and a send is refused and logged, so nothing is sent by accident. To send with Emailit, read the key and the sender from the environment and give the mailer to the builder:

```rust
rocket::build().attach(
    Ignite::new()
        .mailer(EmailitMailer::from_env())
        .resource::<Users>()
        .fairing(),
)
```

Set `EMAILIT_API_KEY` and `IGNITE_MAIL_FROM` in the `.env` file or the environment. `from_env` stops the app at start when either is missing. Emailit answers 401 or 403 for a key or a domain it does not accept, 402 when the account has no credits, 422 for a message it refuses, 429 with `retry_after` when it limits sends, and 404 for an unknown template. Each of these is a `MailError`. Another provider is added as one more implementation of the `Mailer` trait, and the password reset does not change.

Password reset follows Laravel's:

- `/admin/password/forgot` asks for an email. Every request gets the same answer, so the form does not show which emails have an account. A request counts under a throttle key of its own, with the same limit as sign-in: five requests in 60 seconds for each email and address.
- A link is sent only to an account that is enabled. It carries a random token. The table keeps only a hash of it, and the link works for 60 minutes and once.
- A link is not sent again within 60 seconds of the last one to the same email, as Laravel's `throttle` setting does. A double submit sends one message.
- The reset form asks for the new password twice, with the same rules as the change-password form. A reset ends every session of the user, as a password change does.
- The sign-in page links to the forgot page.

A failed send is logged with the error and does not change the answer.

The tests never send mail: they use `RecordingMailer`. One opt-in test, `tests/mail_live.rs`, sends a real message through Emailit. It runs only when `EMAILIT_API_KEY`, `IGNITE_MAIL_FROM` and `IGNITE_TEST_MAIL_TO` are all set, and prints a notice and passes otherwise. The email text follows Laravel's reset notification.

## Tests

The test suite needs a MariaDB or MySQL database that it may drop and rewrite tables in. Point `DATABASE_URL` at a database set aside for tests, such as `ignite_test`, and never at a database with data you keep:

```sh
DATABASE_URL=mysql://user@localhost/ignite_test cargo test --workspace
```

Continuous integration runs the same checks on every push and pull request. It uses a MariaDB 11.4 service, and runs format, lint, the tests and a release build of the panel. The workflow is in `.github/workflows/ci.yml`. Locally, the suite has run against MariaDB 13.0.2.

## Requirements

- Rust (stable).
- MariaDB or MySQL. No MariaDB client library is needed to build.
