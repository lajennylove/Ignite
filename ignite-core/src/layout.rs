//! The layout of a resource's form, as Filament's layout components: a grid of
//! [`columns`](FormLayout::columns), [`Section`]s that group fields under a heading, and
//! [`Group`]s that stack sections in one column, each with a
//! [`column_span`](Section::column_span).
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

/// One part of a form layout: a field by name, a [`Section`] or a [`Group`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Component {
    /// The field of this name, on its own.
    Field(String),
    /// Fields under a heading.
    Section(Section),
    /// Sections and fields stacked in one column.
    Group(Group),
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
