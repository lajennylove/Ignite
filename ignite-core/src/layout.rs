//! The layout of a resource's form, as Filament's layout components: a grid of
//! [`columns`](FormLayout::columns), [`Section`]s that group fields under a heading,
//! [`Group`]s that stack sections in one column, each with a
//! [`column_span`](Section::column_span), and [`Tabs`] that show one [`Tab`] of fields at a
//! time.
//!
//! A WordPress-like post editor puts the content in a main column two thirds wide and the
//! publishing settings in a sidebar column one third wide, as a Filament form does with
//! `->columns(3)` and a `Group` of sections with `->columnSpan(2)` beside one with
//! `->columnSpan(1)`:
//!
//! ```
//! use ignite_core::{FormLayout, Group, Section};
//!
//! let layout = FormLayout::new().columns(3).schema([
//!     Group::make([Section::make("Content").schema(["title", "slug", "body"])]).column_span(2),
//!     Group::make([
//!         Section::make("Publish").schema(["status", "published_at"]),
//!         Section::make("Category").schema(["category"]),
//!     ])
//!     .column_span(1),
//! ]);
//! assert_eq!(layout.column_count(), Some(3));
//! ```
//!
//! As in Filament, `columns` and `column_span` apply from the `lg` breakpoint; on a smaller
//! screen every component takes the full width, one under the other. A form without a layout
//! (the default, [`FormLayout::new`]) renders as before: one card of fields. A field the
//! layout does not place is shown after the others, in the first column, so a form never
//! loses a field it saves.

/// The widest the page's content may be, as Filament's `getMaxContentWidth()` and its `Width`
/// enum. The panel's default is [`ScreenTwoExtraLarge`](Self::ScreenTwoExtraLarge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Width {
    /// 1536px, Tailwind's `max-w-screen-2xl`: the panel's default.
    ScreenTwoExtraLarge,
    /// The whole width of the page beside the sidebar, as Filament's `Width::Full`.
    Full,
}

/// How many grid columns a component takes, as Filament's `columnSpan()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnSpan {
    /// This many columns, from the `lg` breakpoint, as `->columnSpan(2)`.
    Columns(u8),
    /// Every column, on every screen, as `->columnSpanFull()`.
    Full,
}

/// One part of a form layout: a field by name, a [`Section`], a [`Group`] or [`Tabs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Component {
    /// The field of this name, on its own.
    Field(String),
    /// Fields under a heading.
    Section(Section),
    /// Sections and fields stacked in one column.
    Group(Group),
    /// Tabs, each with its own fields and sections, one shown at a time.
    Tabs(Tabs),
}

impl From<Tabs> for Component {
    fn from(tabs: Tabs) -> Self {
        Component::Tabs(tabs)
    }
}

impl From<&str> for Component {
    fn from(name: &str) -> Self {
        Component::Field(name.to_string())
    }
}

impl From<String> for Component {
    fn from(name: String) -> Self {
        Component::Field(name)
    }
}

impl From<Section> for Component {
    fn from(section: Section) -> Self {
        Component::Section(section)
    }
}

impl From<Group> for Component {
    fn from(group: Group) -> Self {
        Component::Group(group)
    }
}

/// Fields under a heading, shown as one card, as Filament's `Section`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    heading: String,
    description: Option<String>,
    fields: Vec<String>,
    column_span: ColumnSpan,
}

impl Section {
    /// A section with the heading `heading`, as `Section::make('Content')`. An empty heading
    /// shows none. It has no fields until [`schema`](Self::schema).
    pub fn make(heading: impl Into<String>) -> Self {
        Section {
            heading: heading.into(),
            description: None,
            fields: Vec::new(),
            column_span: ColumnSpan::Columns(1),
        }
    }

    /// A line under the heading, as `->description()`.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The section's fields by name, in display order, as `->schema([...])`. Each call adds
    /// after the fields already given.
    pub fn schema<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fields.extend(fields.into_iter().map(Into::into));
        self
    }

    /// The columns the section takes in the form's grid, as `->columnSpan(2)`.
    pub fn column_span(mut self, columns: u8) -> Self {
        self.column_span = ColumnSpan::Columns(columns.clamp(1, 12));
        self
    }

    /// Takes every column, as `->columnSpanFull()`.
    pub fn column_span_full(mut self) -> Self {
        self.column_span = ColumnSpan::Full;
        self
    }

    /// The heading, empty when the section has none.
    pub fn heading(&self) -> &str {
        &self.heading
    }

    /// The line under the heading, if any.
    pub fn get_description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The names of the section's fields, in display order.
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// The columns the section takes.
    pub fn get_column_span(&self) -> ColumnSpan {
        self.column_span
    }
}

/// Sections and fields stacked one under the other in one column of the form, as Filament's
/// `Group`: a sidebar is a group of sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    components: Vec<Component>,
    column_span: ColumnSpan,
}

impl Group {
    /// A group of `components`, sections or field names, as `Group::make([...])`. A group in
    /// a group adds its components in its place.
    pub fn make<I, C>(components: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Component>,
    {
        Group {
            components: components.into_iter().map(Into::into).collect(),
            column_span: ColumnSpan::Columns(1),
        }
    }

    /// The columns the group takes in the form's grid, as `->columnSpan(2)`.
    pub fn column_span(mut self, columns: u8) -> Self {
        self.column_span = ColumnSpan::Columns(columns.clamp(1, 12));
        self
    }

    /// Takes every column, as `->columnSpanFull()`.
    pub fn column_span_full(mut self) -> Self {
        self.column_span = ColumnSpan::Full;
        self
    }

    /// The group's components, in display order.
    pub fn components(&self) -> &[Component] {
        &self.components
    }

    /// The columns the group takes.
    pub fn get_column_span(&self) -> ColumnSpan {
        self.column_span
    }
}

/// Tabs, as Filament's `Tabs` schema component: one card whose tab list shows one [`Tab`] of
/// fields at a time. The first tab is open when the form shows, as in Filament, unless
/// [`active_tab`](Self::active_tab) names another. Every tab's fields are in the form, so a
/// save sends them all, whichever tab is open.
///
/// ```
/// use ignite_core::layout::{Tab, Tabs};
/// use ignite_core::{FormLayout, Section};
///
/// let layout = FormLayout::new().schema([Tabs::make("Settings").tabs([
///     Tab::make("General").schema(["description", "active"]),
///     Tab::make("Labels").schema([Section::make("Menu").schema(["menu_name"])]),
/// ])]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabs {
    label: String,
    tabs: Vec<Tab>,
    active_tab: usize,
    column_span: ColumnSpan,
}

impl Tabs {
    /// Tabs with the label `label`, which names the tab list for a screen reader, as
    /// `Tabs::make('Settings')`. They have no tabs until [`tabs`](Self::tabs).
    pub fn make(label: impl Into<String>) -> Self {
        Tabs {
            label: label.into(),
            tabs: Vec::new(),
            active_tab: 1,
            column_span: ColumnSpan::Columns(1),
        }
    }

    /// The tabs, in display order, as `->tabs([...])`. Each call adds after the tabs given.
    pub fn tabs(mut self, tabs: impl IntoIterator<Item = Tab>) -> Self {
        self.tabs.extend(tabs);
        self
    }

    /// The tab open when the form shows, counted from 1, as Filament's `->activeTab(2)`. A
    /// number past the last tab opens the first.
    pub fn active_tab(mut self, tab: usize) -> Self {
        self.active_tab = tab;
        self
    }

    /// The columns the tabs take in the form's grid, as `->columnSpan(2)`.
    pub fn column_span(mut self, columns: u8) -> Self {
        self.column_span = ColumnSpan::Columns(columns.clamp(1, 12));
        self
    }

    /// Takes every column, as `->columnSpanFull()`.
    pub fn column_span_full(mut self) -> Self {
        self.column_span = ColumnSpan::Full;
        self
    }

    /// The label of the tab list.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The tabs, in display order.
    pub fn get_tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// The index (from 0) of the tab open when the form shows.
    pub fn active_index(&self) -> usize {
        match self.active_tab {
            n if n >= 1 && n <= self.tabs.len() => n - 1,
            _ => 0,
        }
    }

    /// The columns the tabs take.
    pub fn get_column_span(&self) -> ColumnSpan {
        self.column_span
    }
}

/// One tab of [`Tabs`], as Filament's `Tabs\Tab`: a label and its fields and sections. It is
/// not re-exported at the crate root, where [`Tab`](crate::Tab) is a list page's tab; write
/// `ignite_core::layout::Tab`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    label: String,
    components: Vec<Component>,
}

impl Tab {
    /// A tab labelled `label`, as `Tab::make('General')`, the label as the app gives it. It
    /// has no fields until [`schema`](Self::schema).
    pub fn make(label: impl Into<String>) -> Self {
        Tab {
            label: label.into(),
            components: Vec::new(),
        }
    }

    /// The tab's fields and sections, in display order, as `->schema([...])`. A group in a tab
    /// adds its components in its place. Each call adds after the components given.
    pub fn schema<I, C>(mut self, components: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Component>,
    {
        self.components
            .extend(components.into_iter().map(Into::into));
        self
    }

    /// The tab's label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The tab's components, in display order.
    pub fn components(&self) -> &[Component] {
        &self.components
    }
}

/// The layout of a resource's create and edit forms: the grid's columns, the components in
/// it, and the page's content width. See the [module documentation](self).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormLayout {
    columns: Option<u8>,
    components: Vec<Component>,
    max_content_width: Option<Width>,
}

impl FormLayout {
    /// No layout: the form renders as one card of fields, as before.
    pub fn new() -> Self {
        FormLayout::default()
    }

    /// The number of columns of the form's grid, from the `lg` breakpoint, as Filament's
    /// `->columns(3)`. Between 1 and 12. Without it, the grid has one column.
    pub fn columns(mut self, columns: u8) -> Self {
        self.columns = Some(columns.clamp(1, 12));
        self
    }

    /// The form's components, in display order, as Filament's `->schema([...])`: sections,
    /// groups and field names. Each call adds after the components already given.
    pub fn schema<I, C>(mut self, components: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Component>,
    {
        self.components
            .extend(components.into_iter().map(Into::into));
        self
    }

    /// The widest the form's page may be, as a Filament page's `getMaxContentWidth()`.
    /// [`Width::Full`] lets a settings form use the whole width beside the sidebar.
    pub fn max_content_width(mut self, width: Width) -> Self {
        self.max_content_width = Some(width);
        self
    }

    /// Whether the form has no layout, and renders as before.
    pub fn is_empty(&self) -> bool {
        self.columns.is_none() && self.components.is_empty() && self.max_content_width.is_none()
    }

    /// The number of columns, if set.
    pub fn column_count(&self) -> Option<u8> {
        self.columns
    }

    /// The form's components, in display order.
    pub fn components(&self) -> &[Component] {
        &self.components
    }

    /// The page's content width, if set.
    pub fn content_width(&self) -> Option<Width> {
        self.max_content_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_layout_is_empty() {
        assert!(FormLayout::new().is_empty());
        assert!(!FormLayout::new().columns(2).is_empty());
        assert!(!FormLayout::new().max_content_width(Width::Full).is_empty());
    }

    #[test]
    fn columns_and_spans_stay_between_1_and_12() {
        assert_eq!(FormLayout::new().columns(0).column_count(), Some(1));
        assert_eq!(FormLayout::new().columns(20).column_count(), Some(12));
        assert_eq!(
            Section::make("A").column_span(0).get_column_span(),
            ColumnSpan::Columns(1)
        );
        assert_eq!(
            Group::make(["a"]).column_span_full().get_column_span(),
            ColumnSpan::Full
        );
    }

    #[test]
    fn the_first_tab_is_active_unless_another_is_named() {
        let tabs = Tabs::make("Settings").tabs([Tab::make("A"), Tab::make("B")]);
        assert_eq!(tabs.active_index(), 0);
        assert_eq!(tabs.clone().active_tab(2).active_index(), 1);
        assert_eq!(tabs.active_tab(9).active_index(), 0);
    }

    #[test]
    fn schema_adds_in_order() {
        let section = Section::make("Content").schema(["title"]).schema(["body"]);
        assert_eq!(section.fields(), ["title", "body"]);
        let layout = FormLayout::new().schema(["a"]).schema([section.clone()]);
        assert_eq!(
            layout.components(),
            [Component::Field("a".into()), Component::Section(section)]
        );
    }
}
