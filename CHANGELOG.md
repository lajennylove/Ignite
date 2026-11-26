# Changelog

All notable changes to Ignite are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.4] - 2026-11-26

### Added

- List tabs, as Filament's `getTabs()` on a List page: `ignite_core::Tab` (`make`, `label`,
  `filter`, `where_eq`, `where_ne`, `where_in`, `where_null`, `where_not_null`), its conditions
  as `ListFilter` (with `apply` on an Orbit query and `matches` on a row) and
  `FilterCondition`, and `active_tab`. `IgniteResource::tabs()` and `default_active_tab()`
  (default: none), `RuntimeResource::tab` and `default_active_tab`, and the derive's
  `#[ignite(tabs = path)]` and `#[ignite(default_active_tab = "key")]`. The `tab` query
  parameter chooses the tab, as Filament's `?tab=`; the first tab is the default. Each tab shows
  the number of rows it matches for the current search, as "Drafts (3)", and its link keeps
  the search and sort. The tabs are plain links, so they work without JavaScript.
- Column badges, as Filament's `badge()` and `color()` on a text column: `ColumnBadges`
  (`badge`, `badge_color`) with `BadgeColor` (`Gray`, `Info`, `Success`, `Warning`, `Danger`,
  `Primary`, the default). `IgniteResource::column_badges()`, `RuntimeResource::badge` and
  `badge_color`, and the derive's `#[ignite(badges = path)]`. A badge cell renders
  `<span class="ig-badge ig-badge-<color>">`, with its value escaped.
- A list with tabs or badges loads the panel's `list.css`; a list without them renders exactly
  as before.
- Child navigation entries, as Filament's `$navigationParentItem`:
  `IgniteResource::NAVIGATION_PARENT_ITEM`, the derive's
  `#[ignite(navigation_parent_item = "Posts")]` and `RuntimeResource::navigation_parent_item`
  name the parent entry by label or slug. A child is listed under its parent, sorted by its
  `NAVIGATION_SORT`, and shows when the parent or one of its children is the current page. As
  in Filament, the parent must be in the same group and not a child itself; a child whose
  parent is not in the sidebar is not listed. The dashboard can be a parent.
- Collapsible navigation groups, as Filament's: `NavigationGroup::make(label)` with
  `collapsible(bool)` and `collapsed(bool)`, passed to `Ignite::navigation_groups` beside
  plain labels, and `Ignite::collapsible_navigation_groups(bool)` for every group. The
  browser remembers the collapsed groups (localStorage `ignite:sidebar:collapsed_groups`, as
  Filament's), the group of the current page shows expanded, and without JavaScript every
  group is expanded. As in Filament, groups are collapsible by default;
  `collapsible_navigation_groups(false)` (or `collapsible(false)` on a group) keeps the plain
  headings of 0.6.2.
- The navigation view values `children`, `has_children`, `has_active_child` and `expanded` on
  each entry, `collapsible`, `collapsed`, `active` and `items_id` on each section, and
  `nav_collapsible`, for themes.
- Form layouts, as Filament's `Grid`, `Section`, `Group`, `columns()` and `columnSpan()`:
  `ignite_core::FormLayout` (`columns`, `schema`, `max_content_width`), `Section` (`make`,
  `description`, `schema`, `column_span`, `column_span_full`), `Group` (`make`, `column_span`,
  `column_span_full`), `Component`, `ColumnSpan` and `Width` (`ScreenTwoExtraLarge`, `Full`).
  `IgniteResource::form_layout()` (default: none), `RuntimeResource::form_layout` and its
  `form_layout` field, `RegisteredResource::form_layout`, and the derive's
  `#[ignite(form_layout = path)]`. A form with a layout loads the panel's `form.css`; a form
  without one renders exactly as before.
- `Width::Full` removes the 1536px cap of the page's content: the layout's `<main>` takes its
  width class from the `ignite_content_width` section, `max-w-[1536px]` by default.
- Slug fields, as Filament's slug example: `FieldConditions::slug_from(field, source)` and
  `RuntimeResource::slug_from`. The input is marked `data-ignite-slug-from="<source>"`.
- Unsaved changes alerts, as Filament's `unsavedChangesAlerts()`:
  `Ignite::unsaved_changes_alerts()`, off by default. The form is marked
  `data-ignite-unsaved-changes-alert`, and `data-ignite-unsaved` when it is shown again after a
  failed save. The behaviour is in the panel's new `form.js`, which a form page loads only when
  it needs it.

### Changed

- **Breaking:** `ListQuery` has a new public field, `filter`, the active tab's conditions, so
  code that builds a `ListQuery` with a struct literal must set it (`ListFilter::new()` for
  none). A hand-written `list` (or `RuntimeData::list`) of a resource with tabs must apply it;
  the panel counts a tab's rows by asking for `per_page` 0.
- **Breaking:** navigation groups are collapsible by default, as in Filament:
  `Ignite::collapsible_navigation_groups` defaults to `true`. A sidebar that should keep the
  plain group headings of 0.6.3 calls `collapsible_navigation_groups(false)` (or
  `collapsible(false)` on a `NavigationGroup`), which renders the same HTML as before.
- **Breaking:** the sidebar HTML changed. In the `ignite` theme's navigation partial, a
  collapsible group is a `<div data-ig-nav-group>` whose heading is a
  `<button data-ig-nav-toggle>` (in place of `<p class="ig-caption">`) that controls the
  group's entries (`id="{items_id}"`); a child entry is printed under its parent; and the
  collapse script follows the navigation. A theme, a stylesheet or a test that reads the
  sidebar's markup must follow it.
- `RuntimeResource` has the new public fields `navigation_parent_item`, `form_layout`, `tabs`,
  `default_active_tab` and `column_badges`.
- The `ignite` theme's layout takes the width class of its `<main>` from the
  `ignite_content_width` section; the default, `max-w-[1536px]`, renders as before.

## [0.6.3] - 2026-11-23

### Added

- "Remember me" on the sign-in page, as Laravel's: a checkbox, unticked by default. Ticked, the
  user also gets the `ignite_remember` cookie (signed, HttpOnly, 400 days, Laravel's remember
  duration of 576000 minutes) with their id and remember token. When the session has ended,
  the cookie signs the user in again with a new session. Unticked, the sign-in is unchanged.
- A new nullable `users.remember_token` column (migration
  `20261117000000_add_user_remember_token`), as Laravel's `rememberToken()`. Existing users
  keep NULL until they sign in with the box ticked.
- Signing out, signing out everywhere and a password change (the profile, the reset link and
  `ignite user password`) set a new remember token, so every remember cookie issued before
  stops working, as Laravel's `cycleRememberToken`. A disabled user is not signed in by it.
- Run `ignite migrate` (or `ignite_panel::migrate`) after upgrading, to add
  `users.remember_token`; `ignite migrate:rollback` drops it.

### Changed

- The sign-in page sends a user who is already signed in, by the session or by the remember
  cookie, to the panel's dashboard, as Laravel's `guest` middleware
  (`RedirectIfAuthenticated`). A signed-out user sees the page as before, and a sign-in attempt
  (the `POST`) is unchanged.

## [0.6.2] - 2026-11-22

### Added

- Conditional fields, as Filament's `visible()`, `hidden()`, `visibleOn()`, `hiddenOn()` and
  `live()`: `ignite_core::FieldConditions`, with the closure context `FormContext`
  (`operation()`, `record()`, `user()`, `get(name)`, `checked(name)`) and `Operation`
  (`Create`, `Edit`). `RuntimeResource` has the builder methods `visible`, `hidden`,
  `visible_on`, `hidden_on` and `live` and a `conditions` field; `IgniteResource` has
  `conditions()` (default: none), and the derive takes `#[ignite(conditions = path)]`. As in
  Filament, a field has one `hidden` and one `visible` condition, a later call replaces the
  earlier one, and the field shows when both allow it.
- A hidden field is not rendered, not checked (a required one included) and not written, on
  create and on update, so a request that sends it anyway does not change the stored value. A
  condition that reads a hidden field with `get` gets its stored value, not the one sent.
  `RegisteredResource::validate_shown` and `validate_update_shown` take the hidden fields;
  `FormInput::without` drops fields from a form.
- Live fields: the form marks each field `data-ignite-field="<name>"`, a live one also
  `data-ignite-live`, and the form `data-ignite-live-url`. When a live field changes, the
  panel's script posts the form to `POST /<slug>/new/live` or `POST /<slug>/<id>/edit/live`
  (CSRF-checked, authorized as the form), and replaces the fields with the ones the server
  renders for the new values. Nothing is saved. A field a change reveals on the edit form shows
  its stored value. The script is part of the panel's built `app.js`; no CDN.
- Release-mode admin themes: a release build renders every registered admin theme. A theme
  other than `ignite` renders through Propel's runtime renderer (`PropelView::render`, which
  renders in release builds since Propel 0.3.0), with the panel's views embedded in the binary
  and the theme's own views from `AdminTheme::view`, so the binary needs no view files on
  disk. The `ignite` theme still renders from the compiled views in a release build, with the
  same HTML as before. Propel stays at 0.3.0, the version ignite-panel already depended on: it
  is the first release whose runtime renderer works in release builds.
- Semantic icons: `ignite_panel::theme::icons` (the names `dashboard`, `post`, `page`,
  `media`, `comment`, `appearance`, `plugins`, `users`, `tools`, `settings`),
  `IGNITE_ICONS` and `ignite_icon(name)` (the `ignite` theme's Tabler icon of each name),
  `IGNITE_RESOURCE_ICON` (`ti ti-table`), and `icon_classes(theme, name)`, which resolves a
  name through a theme and falls back to its generic resource icon. Filament's navigation
  items take an icon name that the panel resolves through its icon set; in Ignite the admin
  theme resolves it.
- `AdminTheme::resource_icon()`: the theme's generic resource icon, for a resource that names
  no icon and for a name the theme does not know. Defaults to `ti ti-table`.
- A navigation entry has `icon_name` (the name, or empty) beside `icon` (the resolved classes).
- Theme stylesheets: `AdminTheme::stylesheets() -> Vec<String>` (default none), the URLs of a
  theme's own stylesheets. They load only on the pages that render in that theme, after the
  panel's CSS and before the app-wide stylesheets of `Ignite::stylesheet` (unchanged since
  0.6.1), in the `ignite_stylesheets` section, escaped in the `href`. A WordPress-like theme
  loads Dashicons this way, and the `ignite` theme does not. The pages have a new
  `admin_theme` value (`name`, `stylesheets`).
- Each user's admin theme: a signed-in user chooses their own admin theme on the profile page
  (an "Admin theme" card, shown when more than one theme is registered, with "Site default"
  first), saved by `POST {mount}/profile/admin-theme` (field `admin_theme`: a registered
  theme's name, or empty for the site default; CSRF-checked; an unknown name is refused with
  400, as the light or dark toggle refuses an unknown mode). The choice is stored in a new
  nullable `users.admin_theme` column (`VARCHAR(64)`) and applies to that user's requests only.
  The panel resolves the theme of each request from the signed-in user's choice, and falls back
  to the site default for a user with no choice, for a stored name no registered theme has, and
  on the sign-in pages. It is separate from the light or dark mode (`users.theme`, the top
  bar's toggle): changing one never changes the other.
- The migration `20261116000000_add_user_admin_theme` adds the column (and its rollback drops
  it). Run `ignite migrate` after upgrading; it is one batch with any other pending migration,
  and `ignite migrate:rollback` reverses that batch, as before.
- `AdminTheme::label()` (default: the name; `Ignite` for the `ignite` theme), shown in the
  profile's choice.
- `ignite_panel::theme`: `registered_admin_themes()`, `is_registered_admin_theme(name)`,
  `resolve_admin_theme(choice)` (the theme a stored choice renders in) and
  `with_admin_theme(name, render)` (renders an app's own view that extends
  `ignite::layouts.app` in a given theme; re-exported from the crate root with
  `registered_admin_themes` and `resolve_admin_theme`). `AdminUser::admin_theme()` (the theme
  the user's requests render in) and `AdminUser::admin_theme_choice()` (the stored name).
  `users::set_admin_theme(db, id, Option<&str>)` and `User::admin_theme`.

### Changed

- `set_admin_theme` sets the site's default admin theme, and `active_admin_theme` returns it: a
  user's own choice overrides it for that user's requests.
- **Breaking:** `users::User` has a new public field, `admin_theme`, so code that builds a
  `User` with a struct literal must set it.
- **Breaking:** a resource's icon is a name, not CSS classes. `NAVIGATION_ICON`, the derive's
  `navigation_icon` attribute and `RuntimeResource::navigation_icon` take a name that says what
  the resource is, such as `"post"`, or the app's own name for a custom post type, such as
  `"book"`. The active admin theme maps the name to its icon. The set of names is open.
- **Breaking:** `AdminTheme::icon(&self, icon: Icon) -> String` is now
  `AdminTheme::icon(&self, name: &str) -> Option<String>`: the theme returns the whole `class`
  value of the icon named `name` (such as `ti ti-pin` or `dashicons dashicons-admin-post`), or
  `None` for a name it does not know. The default maps the names to the `ignite` theme's Tabler
  icons.
- **Breaking:** the `ignite` theme's navigation partial prints `<i class="{icon} text-lg">` for
  every entry; the `<span class="..." aria-hidden="true">` of a resource's own classes is gone,
  and so are the entry's `has_icon_class` and `icon_class` values. The pages of the `ignite`
  theme render the same HTML as before for resources without an icon.

### Migration from 0.6

- **`navigation_icon` with CSS classes.** Replace the classes with the name of what the
  resource is: `navigation_icon = "dashicons dashicons-admin-post"` becomes
  `navigation_icon = "post"`, and `.navigation_icon("dashicons dashicons-sticky")` becomes,
  for example, `.navigation_icon("note")`. The derive refuses a value with a space at compile
  time and says so; a hand-written `NAVIGATION_ICON` or a runtime resource with classes shows
  the theme's generic resource icon instead. To keep a particular icon, map its name in your
  admin theme's `icon`, such as `"note" => Some("dashicons dashicons-sticky".into())`. A
  stylesheet that defined the classes (`Ignite::stylesheet`) is needed only by the theme that
  uses them: move it to that theme's `stylesheets()` (see Theme stylesheets above).
- **A theme that implemented `icon(Icon)`.** Implement `icon(&self, name: &str) ->
  Option<String>` instead: `Icon::Dashboard` is the name `dashboard`, and `Icon::Resource` is
  `resource_icon()`. Return the full class list, as the panel no longer adds `ti`.
- **`ignite_panel::theme::Icon` is removed**, with no replacement type: the names are strings.
- **The database.** Run `ignite migrate` (or `ignite_panel::migrate`) to add
  `users.admin_theme`. Nothing else changes for existing users: they get the site default.

### Removed

- **Breaking:** `ignite_panel::theme::Icon` (see the migration above).
- **Breaking:** `ThemeError::NotCompiled`. `set_admin_theme` no longer refuses a theme other
  than `ignite` in a release build. Code that matched the variant can drop that arm.

### Known limitations

- A live field sends its change on the input's `change` event (at once for a select, a
  checkbox or a radio, on leaving a text input), as Filament's `live(onBlur: true)`; there is
  no per-keystroke `live()` or `debounce`.
- Filament's `hiddenJs()` / `visibleJs()` (conditions written in JavaScript) are not provided:
  every condition runs on the server.
- The live re-render replaces the fields' markup, so an app's editor or field script must
  enhance the new inputs again; the form fires a bubbling `ignite:fields` event for that.
- A static resource whose `validate` refuses a missing field that a condition hides is a
  server error (500): make such a field optional.
- An app's own view that extends `ignite::layouts.app` renders in the site default theme unless
  the app wraps the render in `with_admin_theme(&user.admin_theme(), ...)`; the panel cannot see
  the signed-in user from an app's handler.

## [0.6.1] - 2026-11-17

### Added

- App stylesheets: `Ignite::stylesheet(url)` adds a `<link rel="stylesheet">` at the end of
  the head of every panel page, signed-in and sign-in pages, after the panel's own CSS, as
  Filament registers an app's CSS with `FilamentAsset::register`. Each call adds one, in
  order. The URL is escaped in the `href`. The links are in the compiled views, so a release
  build loads the font of a `navigation_icon` (such as Dashicons). The layout yields them in
  a new `ignite_stylesheets` section. By default there is none, and the pages are unchanged.

### Fixed

- The 0.6.0 limitation that a release build had no place for an app's own stylesheet.

## [0.6.0] - 2026-11-16

### Known limitations

- The admin panel's runtime queries (user create, update and delete, sessions and sign-in) still run on MariaDB and MySQL only, so the panel's policy tests run on MariaDB. Migrations and `migrate:status` work on SQLite, MySQL, MariaDB and PostgreSQL.
- A release build renders the compiled `ignite` layout, which has no place for an app's own
  stylesheet, so the classes of `navigation_icon` render without their font there until the
  panel has an option to add one.
- One `brand_logo` serves the sidebar and the sign-in bar, as Filament's `brandLogo` does;
  there is no dark-mode logo (`darkModeBrandLogo`) yet, and the navigation groups are
  headings, not collapsible, and have no icons or labels of their own (Filament's
  `NavigationGroup` objects).

### Added

- The acting user: `ignite_core::ActingUser`, the signed-in user (id, name, email, `can`,
  `is_super_admin`) that the panel passes to a resource's queries, as Laravel's
  `auth()->user()`. `AdminUser::acting_user` returns it to an app's own routes.
- User-aware queries, with defaults: `RuntimeData` and `IgniteResource` have `list_for`,
  `find_for`, `create_for`, `update_for`, `delete_for` and `options_for`, given the acting
  user. The panel calls them; by default each calls the method without the user. A resource
  restricts its list and records to the user in `list_for` and `find_for`, as a Filament
  resource does in `getEloquentQuery()`: a record that `find_for` does not return is 404 on
  the edit form, its save and the delete. `options_for` gives the user's choices of a select,
  and a value outside them is refused.
- Policies: the `Policy` trait (`view_any`, `view`, `create`, `update`, `delete`, given the
  database, the acting user and, for `view`, `update` and `delete`, the record), with
  `register_policy`, `unregister_policy` and `policy`, keyed by the resource's slug, for static
  and runtime resources, as Laravel's `Gate::policy`. Each method allows by default. An action
  needs the user's ability and the policy's answer, as a Filament Shield policy checks the
  permission first; a super-admin passes every policy, as Shield's super admin does. A refusal
  answers 403. The navigation, the dashboard and the New button follow `view_any` and `create`,
  and each record's Edit and Delete buttons follow `update` and `delete` for that record. A
  policy that fails answers 500. The panel has no detail page, so nothing asks `view` yet.
- Brand options on the panel, as Filament's panel: `Ignite::brand_name`, `brand_logo`,
  `brand_logo_height` and `favicon`. The name shows at the top of the sidebar and in the
  sign-in bar, and ends each page title (`Posts · <name>`); a logo shows there in place of the
  name, with the name as its alternative text, at `brand_logo_height` (default `1.5rem`, as
  Filament's); the favicon replaces Ignite's favicon and touch icon. Without them the pages
  render exactly as before: the Ignite mark beside "Ignite", and Ignite's icons.
- Navigation options, as Filament's resources and panel: `IgniteResource` has
  `NAVIGATION_GROUP`, `NAVIGATION_SORT`, `NAVIGATION_LABEL` and `NAVIGATION_ICON` (defaults
  `None`), the derive takes `#[ignite(navigation_group = "...", navigation_sort = 1,
  navigation_label = "...", navigation_icon = "...")]`, `RuntimeResource` has the fields and
  builder methods of the same names (and `RegisteredResource` the getters), and
  `Ignite::navigation_groups` orders the groups. The sidebar lists the dashboard and the
  resources without a group first, then each group under its name as a heading: the listed
  groups in order, then the others. Inside a group, resources sort by `navigation_sort`
  (lower first; none before any number), then by label. Ungrouped resources keep the order
  they were added in unless they set a sort.
- `navigation_icon` is one or more CSS classes, rendered as
  `<span class="..." aria-hidden="true"></span>` before the label in place of the admin
  theme's icon. Ignite ships no icon font: the app supplies the stylesheet.
- These options are compiled into the panel's views (every page has `brand` and
  `nav_sections`; `nav` stays, in the new order), so they work in a release build, where an
  app's copy of the views is not read.

### Changed

- The edit form's save and the delete action find the record with `find_for` before they
  write, so a record the user's query does not find, or one that does not exist, is 404 before
  the write is tried. A resource without a policy or user-aware queries behaves as before, with
  one more read on those two routes.
- The list's Edit and Delete buttons are decided per record (`row.can_update` and
  `row.can_delete` in `views/ignite/index.propel`). Without a policy they show as before.
- `RuntimeResource` has four more public fields (`navigation_label`, `navigation_group`,
  `navigation_sort`, `navigation_icon`). Code that builds it with `RuntimeResource::new` is
  unchanged; code that builds it with a struct literal adds them (`None`).
- The views read `brand` and `nav_sections`: the sidebar's and sign-in bar's brand and menu
  (`themes/ignite/partials/nav.propel` and `topbar.propel`), and each panel page fills two
  new sections of the layout, `ignite_brand_name` (the end of the title) and `ignite_favicon`
  (the new `partials/favicon.propel`). The layout itself still reads no page values: without
  these sections it prints `Ignite` and Ignite's icons, so an app's own view that extends it
  renders as before. A copy of the views made before this release (`views_dir` or
  `IGNITE_VIEWS_DIR`) still renders, without the new options.

## [0.5.0] - 2026-11-15

### Known limitations

- The admin panel's runtime queries (user create, update and delete, sessions and sign-in) run on MariaDB and MySQL only. Migrations and `migrate:status` work on SQLite, MySQL, MariaDB and PostgreSQL. PostgreSQL and SQLite support for the runtime queries follows in a later release.

### Added

- Ignite builds on `orbit-orm` 0.4.0, so an app's own migrations can use its new
  `long_text` and `medium_text` columns beside the panel's.

### Changed

- **Breaking:** Ignite depends on `orbit-orm` 0.4.0 instead of 0.3.0. The `Database` that
  `ignite-core` re-exports, and that `IgniteResource`, the migration functions, the user
  functions and the lifecycle callbacks take, is now `orbit_orm` 0.4's, as are the
  `Transaction` the callbacks are given and the `orbit_orm::Error` these functions return. An
  app moves its own `orbit-orm` dependency to 0.4 with it, since the two versions' types do
  not mix. Ignite's own API is otherwise unchanged.
- The documentation of `rollback` is corrected: it reverts the latest batch of the
  database's migrations table, which an app's own migrations can share, and one rollback
  reverses the whole run, as Laravel's does. When that batch holds a migration that is not
  the panel's, it reverts nothing and returns `orbit_orm::Error::UnknownMigration`. It said
  before that other migrations in the same database are not touched.

## [0.4.0] - 2026-11-15

### Known limitations

- The admin panel's runtime queries (user create, update and delete, sessions and sign-in) run on MariaDB and MySQL only. Migrations and `migrate:status` work on SQLite, MySQL, MariaDB and PostgreSQL. PostgreSQL and SQLite support for the runtime queries follows in a later release.


### Added

- Field types for custom fields, in `FieldKind` and its owned twin `FieldKindDef`, for
  static (derived) and runtime resources: `Email`, `Url`, `Radio`, `Color`, `Image`,
  `File`, `Link`, `Wysiwyg`, `PostObject`, `Relationship`, `Taxonomy`, `User`, `Repeater`,
  `Group`, `FlexibleContent` (with `Block` / `BlockDef`), `Gallery` and `Clone`. Each one
  checks its input (`Field::check_value`, and `check_email`, `check_url`, `check_color`,
  `check_link`): an email must be a valid address, a URL must parse (absolute, or
  protocol-relative `//host` as ACF accepts), a link's URL may also be a root-relative path,
  an anchor or a query as ACF's link picker stores, script schemes are refused, and a color
  is `#rrggbb`.
  `Link`, `Relationship`, `Gallery`, `Repeater`, `Group`, `FlexibleContent` and `Clone`
  store JSON text that round-trips; the nested values are checked against their
  sub-fields. `PostObject`, `Relationship`, `Taxonomy` and `User` load their choices as
  `Lookup` does (`FieldKind::loads_options`).
- `register_editor(script_url)`: the editor script a `Wysiwyg` field loads. Ignite bundles
  no editor.
- `MediaLibrary`, `MediaItem` and `register_media_library`: the app's media library, which
  `Image`, `File` and `Gallery` fields show and check their ids against.
- The derive takes the new kinds as `form_type = "email"` and so on, `form_type = "radio"`
  with `options`, `post_object`, `taxonomy`, `user` and `relationship` with loaded options,
  and `fields = ...` / `blocks = ...` for a repeater, group, clone or flexible content.
- The panel renders each new kind through a partial under
  `views/ignite/partials/fields/`, and reads form keys `name[]` and `name[key]` as a JSON
  list or object, as Laravel reads them.
- Admin themes (`ignite_panel::theme`): the `AdminTheme` trait (layout, navigation, top bar,
  flash, colour tokens with `ColorToken`, icons with `Icon`, and the theme's own views),
  `register_admin_theme`, `set_admin_theme` and `active_admin_theme`. The built-in theme,
  `ignite`, is registered and active from the start and renders the same HTML as before;
  its views are in `views/ignite/themes/ignite`. Switching theme changes the look, not the
  screens or the flows. A release build renders the compiled `ignite` views and refuses
  another theme (`ThemeError::NotCompiled`).

- User lifecycle callbacks: `Ignite::on_user_created`, `Ignite::on_user_deleting` and
  `Ignite::on_user_deleted`, async callbacks given the user's id and the open transaction
  (`users::UserCallbackFuture`), so an app's own rows are written or removed atomically with
  the user's. Created runs after the insert; deleting runs before the delete and deleted
  after it, as Laravel's `deleting`/`deleted` events and WordPress's
  `delete_user`/`deleted_user` actions do. They run, in the order they were registered, for
  every user Ignite creates or deletes: from the panel, from the command line, and from
  `users::create_user` and `users::delete_user`. A callback that returns an error rolls the
  whole change back and returns the error. The callbacks are process-wide: the most recently
  configured `Ignite` sets them, and `users::flush_callbacks` removes them.
- `Ignite::run_cli` and `Ignite::call_cli`: an app runs the `ignite` command line through its
  builder, from its own binary, so the commands run its user callbacks, as Laravel's
  `artisan` is the app's own console. The stock `ignite` binary runs none.
- `users::delete_user` and `ignite user delete <email>`. The command refuses to delete the last
  enabled super-admin (`AuthzError::CannotDeleteLastSuperAdmin`); `authz::delete_user` is its
  library form.

### Changed

- **Breaking:** profile pictures are files on disk, not database blobs, as WordPress keeps
  its uploads. They are written to `avatars/<user id>.<png|jpg|webp>` in the panel's storage
  directory: `storage` in the working directory, or the path given to the new
  `Ignite::storage_dir`. The `user_avatars` table of the migration `20261111000000_profile`
  now has a `path` column (relative to the storage directory) instead of the MySQL-only
  `data MEDIUMBLOB`, so the migration has no database-specific SQL;
  `users.avatar_mime` is the only record of the picture's type, and the panel serves the
  picture with it. Replacing a picture with one of another type, removing it, and deleting a
  user (`users::delete_user`, `ignite user delete`) remove the old file.
  `users::flush_storage_dir` restores the default directory.
- The panel's nine migrations are written with Orbit's schema builder, in
  `ignite-panel/src/migrations/`, instead of the MySQL `up.sql` and `down.sql` files in
  `ignite-panel/migrations/`, which are gone. `ignite migrate` and `migrate:rollback` now run
  them on SQLite and PostgreSQL as well as MySQL and MariaDB (`ignite-panel` features `sqlite`
  and `postgres`). The names, and the names of their indexes, uniques and foreign keys, are
  unchanged, so an existing install's history matches and nothing runs twice; on MySQL and
  MariaDB they build the same tables as before. On SQLite the `sessions_user_fk` key of
  `20261114000000_account_lifecycle` is not created, because SQLite cannot add a foreign key
  to an existing table; `users::delete_user` and `ignite user delete` delete the user's
  sessions themselves, so they go with the user there too. Both run on SQLite: their row
  locks use Orbit's `for_update`, which leaves the lock out on SQLite, as Laravel does.
  `ignite migrate:status`, and the pending-migrations check of the other commands, read the
  migrations table on all three databases too. The panel's other runtime queries still need
  MySQL or MariaDB.
- **Breaking:** `FieldKind` and `FieldKindDef` have new variants, so an exhaustive `match`
  on them needs new arms.
- **Breaking:** `AuthzError` has a new variant, `CannotDeleteLastSuperAdmin`.
- `users::create_user` inserts the user in a transaction, with the app's created callbacks.
- The panel's layout and its navigation, top bar and flash partials moved to
  `views/ignite/themes/ignite`. `ignite::layouts.app` and `ignite::partials.nav`, `.topbar`
  and `.flash` still exist and resolve to the active theme's views, so an app view that
  extends the layout is unchanged. A views directory (`views_dir`, `IGNITE_VIEWS_DIR`)
  must now hold `themes/ignite` too: copy `views/ignite` again. The app stops at startup
  when the theme's views are missing, and says where the layout moved when the directory
  has a customised `layouts/app.propel`. Edit the files under `themes/ignite` to change
  the look.

## [0.3.0] - 2026-11-14

0.3.0 is a breaking release. Read the migration notes first.

### MIGRATION from 0.2.x

1. **`FieldErrors` keys are `String`.** `FieldErrors` is now `HashMap<String, String>`
   (it was `HashMap<&'static str, String>`), as Laravel's `MessageBag` keys messages by
   field name. A hand-written `IgniteResource` adds `.to_string()` to each literal key:
   `errors.insert("price".to_string(), "Price must be a whole number.".to_string())`.
   Reading is unchanged: `errors.get("price")` still takes a `&str`. The `Field::check_*`
   methods keep their signatures. A model with `#[derive(IgniteResource)]` needs no change,
   except where the app builds the derived input (`<Model>Input`) itself with a struct
   literal: that literal no longer compiles (E0063, missing `__ignite_sent`), because the
   input now records which optional fields the form sent. Add one line to it:
   `__ignite_sent: Vec::new()`. An empty list means no optional field was sent, so an
   update keeps the stored value of each optional field that is `None`; list a field's name
   (`vec!["note"]`) to write NULL for it instead. On create, `None` is NULL either way.
2. **The derive writes NULL when an optional field is cleared on edit.** An `Option` field
   that the edit form sends blank is now saved as NULL, as on create. Before, it was
   skipped and kept its stored value, so an optional field could never be cleared.
   Existing data changes when a user submits such a field blank: check forms whose
   optional fields were left blank on purpose to keep a value. A field the request does
   not send at all is not written, so its stored value is kept, as before: an app that
   builds a `FormInput` itself, or posts a partial form, sees no change for the fields it
   leaves out (`FormInput::has` tells the two apart). A password field is not affected:
   left blank on edit, it still keeps the stored hash. A checkbox that is not sent is
   `false`, as before, because an unticked HTML checkbox sends nothing. Fields that are not
   `Option` are unchanged and still required.

Everything else under Changed below has been on the main branch since 0.2.0 and is also
breaking for an app on 0.2.0: the move from Diesel to Orbit (`orbit-orm`), the async
`IgniteResource`, and the new migrations (run `ignite migrate`). The README's install
section covers the Orbit setup.

### Changed

- `FieldErrors` is `HashMap<String, String>`. See the migration notes.
- `#[derive(IgniteResource)]` writes NULL for an optional field sent blank on update, and
  leaves out one that the form did not send, as a runtime resource does, so the two kinds
  of resource save a cleared field alike. A blank password on update is still skipped. The
  generated input struct has a hidden `__ignite_sent` field for this.
- Migrations run on Orbit (`orbit-orm`) instead of Diesel. `ignite migrate`, `ignite migrate:status`
  and `ignite migrate:rollback` keep their names. The migrations are recorded in Diesel's
  `__diesel_schema_migrations` table. The rows Diesel wrote are batch 0 and are not run again.
- `ignite migrate:rollback` reverts the last batch, not one migration. A batch is the set of
  migrations one `ignite migrate` applied. The rows Diesel wrote are batch 0, so the
  rollback that reaches them reverts every panel migration.
- The migration functions `migrate`, `run_migrations`, `pending_migrations`, `migration_status`
  and `rollback` are async and take an `orbit_orm::Database`, not a Diesel connection. `rollback`
  returns the names it reverted.
- The database layer is Orbit (`orbit-orm`) instead of Diesel. Diesel and r2d2 are gone from
  Ignite and from the build, so no MariaDB client library is needed to build.
- `IgniteResource` is async and takes an `orbit_orm::Database`. Its methods (`list`, `find`,
  `create`, `update`, `delete`, `latest`, `widgets`, `options`) return futures. `Pool`, `Conn` and
  `build_pool` are removed from `ignite-core`.
- `#[derive(IgniteResource)]` takes `#[ignite(table = <name>)]`, which must match the model's
  `#[orbit(table = ...)]`, checked at compile time. `belongs_to` takes the related model type, and
  `options = <fn>` takes an async `fn(&Database)`. The model derives `OrbitModel` and marks its
  writable columns `#[orbit(fillable)]`.
- Connection failure at start stops the launch with an error. `Ignite::new()` reads the environment
  and no longer connects.
- The derive's message for an unsupported field type lists the types it now supports.
- A resource's number input still accepts any value in the browser, as before. The
  server checks an integer as a whole number and a decimal as a number.
- The last-super-admin rule counts only enabled users. A disabled super-admin cannot lose
  the role while no enabled super-admin is left, and the last enabled one cannot be disabled.
- An upgrade gives `super-admin` to every user who exists when the migration runs, so they
  keep full access. A user created after the upgrade has no roles and sees no resource
  until `ignite user role add` gives them one.

### Added

- Runtime resources: a `RuntimeResource` is defined by data (`FieldDef`, `FieldKindDef`,
  `ColumnDef`, owned twins of `Field`, `FieldKind` and `Column`) and reads its rows through a
  `RuntimeData` the app implements. `register_runtime` and `unregister_runtime` add and remove
  one while the panel serves; the panel serves it at `/admin/<slug>` with the same list, forms
  and delete action as a static resource. A slug that is already used is refused with a
  `RuntimeError` (`InvalidSlug`, `SlugTaken`, `InvalidFieldName`, `DuplicateField`,
  `NotRegistered`). `ignite_panel::register_runtime_permissions` creates its five permissions
  at registration; without it, the panel creates them on the next signed-in request.
  A field name must be a lowercase identifier: lowercase ASCII letters, digits and `_`,
  starting with a letter, at most 64 bytes (`MAX_FIELD_NAME`); `register_runtime` refuses
  any other with `RuntimeError::InvalidFieldName`. Field names are owned by their resource
  and freed with it: no name is kept for the life of the process. On update, an optional
  field sent blank is `null` in the row and one not sent is left out.
- `FormInput::has`, which tells a field sent blank from one the form did not send.
- Roles and permissions, modelled on Filament and Filament Shield. Each resource has the
  abilities `<slug>.viewAny`, `<slug>.view`, `<slug>.create`, `<slug>.update` and
  `<slug>.delete`, which the panel registers when the app starts. The routes answer 403
  Forbidden without the ability. The navigation, the dashboard and the buttons show only
  what the user may do. The role `super-admin` passes every check.
- The `ignite role create`, `ignite role grant`, `ignite user role add`,
  `ignite user role remove` and `ignite user roles` commands.
- `AdminUser::can`, which checks an ability in an app's own route.
- The migration `20261113000000_roles_permissions`, which adds the tables `roles`,
  `permissions`, `role_permission` and `role_user`.
- `ignite user disable <email>` and `ignite user enable <email>`. A disabled user gets the
  normal wrong-password message when they sign in, and their sessions end at once.
- `ignite user password <email>`, which sets a new password for an existing user and ends
  their sessions. It reads the password the way `ignite user create` does.
- The migration `20261114000000_account_lifecycle`, which adds `users.disabled_at` and a
  foreign key from `sessions.user_id` to `users.id` with `ON DELETE CASCADE`, so deleting a
  user deletes their sessions.
- Field types for `#[derive(IgniteResource)]`: `i16`, `f32` and `f64` (a decimal input,
  with `step = "0.01"` for its browser step), and `chrono::NaiveDateTime` (a date-and-time
  input). Each is checked as its type says: a decimal refuses text and values that are not
  finite, and a date-and-time refuses what is not one. The profile's decimal and date-time
  fields are checked the same way.
- Static selects: `#[ignite(options = [("draft", "Draft"), ("published", "Published")])]` on
  a `String` field. A submitted value must be one of the choices.
- Dynamic selects: `#[ignite(options = load_choices)]` on a `String` or integer field, where
  `load_choices` is an async function
  `async fn(&Database) -> Result<Vec<(String, String)>, orbit_orm::Error>` that the panel
  calls each time it shows the form. A new `IgniteResource::options` method
  returns the choices of a field by name, and the panel checks a submitted value against them.
- Relation selects: `#[ignite(belongs_to = Category, display = name)]`, where `Category` is
  the related Orbit model, on an `i32` or `i64` field stores the related row's `id`. The select lists the related rows by their
  `display` column, and the list and the dashboard show that name in place of the id.
- A select starts with a "Select an option" placeholder, so a submitted form never sends the
  first choice by accident. Profile selects show it too.
- Sending email, behind the `Mailer` trait, with `Ignite::mailer` to choose the provider. The
  default, `NotConfigured`, refuses to send. `RecordingMailer` keeps messages in memory for
  tests. `EmailitMailer` sends through the Emailit API, and reads `EMAILIT_API_KEY` and
  `IGNITE_MAIL_FROM` with `EmailitMailer::from_env`. Failures come back as a typed `MailError`.
- Password reset by email, modelled on Laravel's password reset: `/admin/password/forgot`,
  and the link `/admin/password/reset/<token>?email=`. A link works for 60 minutes and once.
  The forgot page gives the same answer whether or not the email has an account. A disabled
  user gets no link. A reset ends every session of the user. A link is not sent again within
  60 seconds of the last one. The forgot requests are throttled like sign-in.
- The `IGNITE_APP_URL` variable, which the reset links are built from.
- The migration `20261115000000_password_reset_tokens`, which adds the table
  `password_reset_tokens`. The token is stored as an argon2 hash.
- A "Forgot password?" link on the sign-in page.


## [0.2.0] - 2026-11-09

### Added

- A new sidebar and top bar in the panel layout.
- A profile page at `/admin/profile`, with the name and email, the app's own profile
  fields, a password form, and a profile picture (PNG, JPEG or WebP, up to 1 MB).
- A light and dark theme for each user, saved with the user's account.
- `ignite migrate`, a command that runs the panel's pending migrations as a deploy step.
  It prints what it applied, or "Nothing to migrate.", and exits 1 on failure.
- `ignite migrate:status`, which lists each migration as Applied or Pending.
- `ignite migrate:rollback`, which reverts the most recently applied migration with its
  `down.sql`. It reverts one step per run and says so in its output.
- `.env` loading. `Ignite::new()` reads a `.env` file from the working directory.
  Variables already set in the environment take precedence over the file.
- `IGNITE_TRUSTED_PROXIES`, a comma-separated list of IP addresses and CIDR ranges
  whose forwarded headers the panel trusts.
- `GET /up`, a health check that answers with status 200 and a minimal HTML page, "Application
  up", at the application root. It needs no sign-in, as Laravel's `/up` does.
- The `MIT` license (`LICENSE`), and `license = "MIT"` in each crate.
- Continuous integration in `.github/workflows/ci.yml`.

### Changed

- The login throttle counts failed sign-ins per email and address together, as
  Laravel's login throttle does: five failures in 60 seconds. Before, failures were
  counted per email (five) and per address (twenty) separately. Now a failed attempt for
  one email does not count against another email from the same address, and the
  address limit of twenty is gone.
- `ignite user create` no longer runs migrations. When migrations are pending it stops
  before the password prompt and says to run `ignite migrate` first.
- The panel's fairing no longer runs migrations at startup. Run `ignite migrate` before
  the first start. `run_migrations` stays public for apps that want to call it.
- The profile's "is required" message matches the resource form ("Name is required.").
- Profile number fields accept decimals, as Laravel's `numeric` rule and Filament's numeric
  inputs do. Resource integer fields still accept whole numbers only. Number inputs carry
  `step="any"`, so browsers accept decimals.
- Profile date fields are checked with the same rule as resource date fields.
- Cookies of the panel get the `Secure` flag when the request was made over HTTPS: directly,
  or through a proxy listed in `IGNITE_TRUSTED_PROXIES` that sends `X-Forwarded-Proto: https`.
  Before, they got `Secure` in release builds only, whatever the scheme. A plain HTTP request,
  such as a local development server, now gets cookies without the flag.
- A `.env` file that exists but cannot be parsed stops the app with a clear message. A missing
  `.env` is still fine.

### Fixed

- Password-type profile fields are stored as argon2 hashes, not as plain text. The form
  never shows the stored value, and a blank password keeps the stored hash.
- The client address for the login throttle is the socket address. `X-Real-IP` and
  `X-Forwarded-For` are read only from a proxy listed in `IGNITE_TRUSTED_PROXIES`, so a
  client can no longer choose its own bucket by sending a header.
- The login throttle table deletes rows whose window has ended on each failure. It no
  longer grows with every email that was tried.
- A password change is throttled under its own key: the email and address, with the same limit
  as sign-in. A wrong current password counts against it, and a successful change clears only
  that key. It never clears the sign-in count.
- A resource that declares zero rows per page no longer divides by zero on the list view.

### Migrations added since 0.1.0

- `20261110000000_add_user_theme`: the user's colour theme.
- `20261111000000_profile`: the profile picture and the app's profile values.
- `20261112000000_login_throttles_window_index`: an index on `login_throttles(window_start)`.

## [0.1.0]

The first release. It was tagged `v0.1.0` and is not described here in detail.
