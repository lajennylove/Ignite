//! The panel's 404 page, as Laravel's `resources/views/errors/404.blade.php`: a request under
//! the panel's mount path that finds nothing (an unknown path, a resource that does not exist,
//! or a record that is not there) answers 404 with a page inside the panel's layout, in the
//! admin theme of the request. A signed-in user sees the navigation and the top bar around the
//! message; a signed-out visitor sees the page without them, as on the sign-in page. The page's
//! message, its link and its view are set with [`Ignite::not_found`](crate::Ignite::not_found).
//!
//! The panel registers its catcher at its mount path only, so a 404 elsewhere, such as on the
//! app's public site, keeps the app's own 404 page.

use propel_rocket::PropelView;
use rocket::http::Status;
use rocket::request::{Outcome, Request};

use crate::ignite::Registry;
use crate::session::{csrf_token, AdminUser};
use crate::views::{compiled, render_page, Layout, NotFoundView};

/// The title of the panel's 404 page, as Laravel's default 404 page says "Not Found".
pub const DEFAULT_NOT_FOUND_TITLE: &str = "Not Found";

/// The message of the panel's 404 page when the app sets none.
pub const DEFAULT_NOT_FOUND_MESSAGE: &str = "The page you are looking for could not be found.";

/// The label of the 404 page's link to the dashboard when the app sets none.
pub const DEFAULT_NOT_FOUND_LINK: &str = "Go to the dashboard";

/// The panel's 404 page: its title, its message, the label of its link to the dashboard and,
/// optionally, the view that renders it. Pass it to [`Ignite::not_found`](crate::Ignite::not_found).
///
/// ```ignore
/// Ignite::new().not_found(
///     NotFound::make()
///         .message("Sorry, you are not allowed to access this page.")
///         .link_label("Back to the Dashboard"),
/// )
/// ```
#[derive(Debug, Clone)]
pub struct NotFound {
    title: String,
    message: String,
    link_label: String,
    view: Option<String>,
}

impl Default for NotFound {
    fn default() -> Self {
        NotFound {
            title: DEFAULT_NOT_FOUND_TITLE.to_string(),
            message: DEFAULT_NOT_FOUND_MESSAGE.to_string(),
            link_label: DEFAULT_NOT_FOUND_LINK.to_string(),
            view: None,
        }
    }
}

impl NotFound {
    /// The page with Ignite's title, message and link.
    pub fn make() -> Self {
        NotFound::default()
    }

    /// The page's heading and `<title>`. The default is [`DEFAULT_NOT_FOUND_TITLE`].
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The line under the heading. The default is [`DEFAULT_NOT_FOUND_MESSAGE`]. It is
    /// escaped, as every value of the views is.
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// The text of the link to the panel's dashboard. The default is
    /// [`DEFAULT_NOT_FOUND_LINK`].
    pub fn link_label(mut self, label: impl Into<String>) -> Self {
        self.link_label = label.into();
        self
    }

    /// Renders the page with the app's view, such as `app::errors.404`, instead of the panel's
    /// `ignite::not_found`, as Laravel renders `errors/404.blade.php`. The view is rendered
    /// with Propel's runtime renderer in the request's admin theme and is given the values of
    /// the panel's page: the layout's values (it can extend `ignite::layouts.app` and include
    /// `ignite::partials.nav` and `ignite::partials.topbar` when `signed_in`), `title`,
    /// `message`, `link_label` and `dashboard_url`.
    pub fn view(mut self, view: impl Into<String>) -> Self {
        self.view = Some(view.into());
        self
    }

    /// The page's title.
    pub fn get_title(&self) -> &str {
        &self.title
    }

    /// The page's message.
    pub fn get_message(&self) -> &str {
        &self.message
    }

    /// The label of the link to the dashboard.
    pub fn get_link_label(&self) -> &str {
        &self.link_label
    }

    /// The app's view, if it set one.
    pub fn get_view(&self) -> Option<&str> {
        self.view.as_deref()
    }
}

/// Answers a 404 under the panel's mount with the panel's 404 page. The catcher is registered
/// at the mount path, so Rocket calls it only for the panel's requests.
#[rocket::catch(404)]
pub(crate) async fn not_found(request: &Request<'_>) -> Result<(Status, PropelView), Status> {
    let registry = request
        .rocket()
        .state::<Registry>()
        .ok_or(Status::NotFound)?;
    // The page is in the user's admin theme with the navigation when they are signed in; a
    // visitor, or a session the database cannot read, gets the page without them.
    let user = match request.guard::<AdminUser>().await {
        Outcome::Success(user) => Some(user),
        _ => None,
    };
    let token = csrf_token(request.cookies(), &registry.secret);
    // The flash message stays for the next page: the 404 does not show it.
    let layout = Layout::new(registry, user.as_ref(), "\u{0}not-found", None, token);
    let page = NotFoundView::new(layout, &registry.not_found, &registry.mount);
    let view = match registry.not_found.get_view() {
        Some(name) => {
            let entry = crate::theme::entry_or_default(&page.admin_theme.name);
            crate::theme::rendering(entry, || PropelView::render(name, &page))
        }
        None => render_page("ignite::not_found", &page, compiled::not_found::render),
    };
    Ok((Status::NotFound, view))
}
