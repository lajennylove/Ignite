//! Derive macro for Ignite resources: `#[derive(IgniteResource)]`.
//!
//! The derive reads an Orbit model and writes the `ignite_core::IgniteResource` impl that
//! the panel needs: the form fields, the list columns, the query methods, and
//! the input struct that create and update take. The model must also derive
//! `OrbitModel` and `Serialize`, because the derive does not write them.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, GenericArgument, LitBool, LitStr, PathArguments,
    Token, Type,
};

/// Writes `impl ignite_core::IgniteResource` for a struct that models a table.
///
/// Struct attributes:
/// - `#[ignite(table = widgets)]`: the table, required. Its name is the URL slug, and it must be the
///   table of the model's `#[orbit(table = ...)]` (or the default name of the struct). A mismatch is a
///   compile error.
/// - `#[ignite(title = "Widgets")]`: the heading and nav label. Defaults to the table name, capitalized.
/// - `#[ignite(singular = "Widget")]`: the name of one record, for "New Widget". Defaults to the title without a trailing `s`.
/// - `#[ignite(navigation_label = "All widgets")]`: the label of the resource's navigation entry, as Filament's `$navigationLabel`. Defaults to the title.
/// - `#[ignite(navigation_group = "Shop")]`: the navigation group the entry is listed under, as Filament's `$navigationGroup`. Defaults to none: the entry is listed before the groups.
/// - `#[ignite(navigation_sort = 2)]`: the entry's place, as Filament's `$navigationSort`: lower first. Defaults to none, which comes before any number.
/// - `#[ignite(navigation_parent_item = "Posts")]`: the navigation entry the resource is listed under, as Filament's `$navigationParentItem`: the parent's label or slug, in the same navigation group. Defaults to none: the resource is an entry of its own.
/// - `#[ignite(navigation_icon = "post")]`: the name of the entry's icon, as Filament's `$navigationIcon`: what the resource is (`post`, `page`, `media`, `comment`, `users`, `settings`, or a name of the app's own), not a CSS class. The admin theme maps the name to its icon. Defaults to the theme's generic resource icon.
/// - `#[ignite(navigation_create_item = false)]`: leaves the create link out of the resource's navigation group. Defaults to `true`: the entry's group lists a link to the create page, for a user who may create a record.
/// - `#[ignite(navigation_create_label = "Add New Post")]`: the label of that create link. Defaults to `"Add new"`.
/// - `#[ignite(conditions = post_conditions)]`: the conditions that show or hide the form's fields, and the live fields, as Filament's `visible()`, `hidden()` and `live()`: the path of a `fn() -> ignite_core::FieldConditions`. Defaults to none: every field shows. A field that a condition may hide should be an `Option`, so it is NULL on create and keeps its stored value on update.
/// - `#[ignite(tabs = post_tabs)]`: the tabs of the list page, as Filament's `getTabs()`: the path of a `fn() -> Vec<ignite_core::Tab>`. The list query applies the active tab's conditions. Defaults to none: the list has no tabs.
/// - `#[ignite(default_active_tab = "drafts")]`: the key of the tab shown when the list has no `tab` parameter, as Filament's `getDefaultActiveTab()`. Defaults to the first tab.
/// - `#[ignite(tab_counts_follow_search = true)]`: each tab's count is for the current search. Defaults to `false`: a tab counts all its rows, as a Filament tab's `badge()` counts its own query.
/// - `#[ignite(hide_empty_tabs = false)]`: renders the tabs whose count is 0 too. Defaults to `true`: an empty tab is left out, except the default tab and the tab shown.
/// - `#[ignite(badges = post_badges)]`: the list columns shown as coloured badges, as Filament's `badge()` and `color()` on a text column: the path of a `fn() -> ignite_core::ColumnBadges`. Defaults to none: every cell is plain text.
/// - `#[ignite(form_layout = post_layout)]`: the layout of the create and edit forms, as Filament's `Grid`, `Section`, `Group`, `columns()` and `columnSpan()`: the path of a `fn() -> ignite_core::FormLayout`. Defaults to none: the form renders as one card of fields.
///
/// Field attributes:
/// - `primary_key`: the key column. Defaults to a field named `id`.
/// - `label = "..."`: the label. Defaults to the field name, capitalized and spaced.
/// - `form_type = "text" | "textarea" | "password" | "number" | "checkbox" | "date"`: the input. Defaults from the type.
/// - `form_type = "email" | "url" | "color" | "link" | "wysiwyg" | "image" | "file" | "gallery"`: the field types for custom fields (see `ignite_core::FieldKind`). The field must be a `String`; an `image` or `file` may also be an integer, the media item's id. A `link` and a `gallery` store JSON text.
/// - `form_type = "radio"` with `options = [...]`: the choices as radio buttons. `form_type = "post_object" | "taxonomy" | "user"` with `options = load_choices` or `belongs_to`: a lookup of that kind. `form_type = "relationship"` with `options = load_choices`: several choices, stored as a JSON list in a `String`.
/// - `form_type = "repeater" | "group" | "clone"` with `fields = SUB_FIELDS`, or `form_type = "flexible_content"` with `blocks = BLOCKS`: a structured field whose sub-fields are a `&'static [ignite_core::Field]` (or blocks a `&'static [ignite_core::Block]`) constant. The field must be a `String`, and stores JSON text.
/// - `options = [("draft", "Draft"), ...]`: a select with a fixed list of choices. The field must be a `String`.
/// - `options = load_choices`: a select whose choices come from an async function, `async fn(&Database) -> Result<Vec<(String, String)>, orbit_orm::Error>`, called each time the form is shown. The field must be a `String` or an integer.
/// - `belongs_to = Category, display = name`: a relation. `Category` is the related model (an `OrbitModel`). The field stores the related row's key, the select lists the related table's rows by `display`, and the list shows that name. The field must be an `i32` or `i64`.
/// - `step = "0.01"`: the browser's step for an `f32` or `f64` input. The default is `"any"`.
/// - `required` or `required = false`: whether a blank value is rejected. An `Option` field is optional by default and `required` makes it required. A field that is not an `Option` is always required: `required = false` on it is a compile error (make it an `Option` instead). A `bool` is the exception: it is a checkbox and is never blank.
/// - `searchable`, `sortable`: the list's search and sort columns.
/// - `hidden_on_index`: leaves the column out of the list.
///
/// Supported field types: `String`, `i16`, `i32`, `i64`, `f32`, `f64`, `bool`,
/// `chrono::NaiveDate`, `chrono::NaiveDateTime`, and `Option` of any of them. The primary
/// key must be `i32` or `i64`. A `String` field is a text input (or a textarea, a password
/// or a select, by attribute); an `f32` or `f64` is a decimal input; a `NaiveDateTime` is a
/// date-and-time input; a `bool` is a checkbox.
///
/// Saving: create writes every field; an `Option` field left blank is NULL. Update writes
/// every field the form sent: an `Option` field sent blank is written as NULL, so a user
/// clears an optional value by emptying it on the edit form (since 0.3.0; before, a blank
/// optional field was skipped on update and kept its value), and an `Option` field the form
/// did not send at all is left out, so its stored value is kept. A password field is the
/// exception: left blank on edit, it is not written, so the stored hash is kept. A checkbox
/// is never blank: one the form did not send is `false`. A field that is not an `Option` is
/// required. Errors are keyed by the field's name in `ignite_core::FieldErrors`, whose keys
/// are owned `String`s since 0.3.0.
///
/// The generated input struct (`<Model>Input`) has a hidden `__ignite_sent` field that
/// records which optional fields the form sent. `validate` and `validate_update` fill it.
/// Code that builds the input with a struct literal must set it (since 0.3.0):
/// `__ignite_sent: Vec::new()` keeps the stored value of every optional field that is `None`
/// on update, and listing a field's name, as in `vec!["note"]`, writes NULL for it. An update
/// that has nothing to write runs no query, and returns the row (or `NotFound`).
#[proc_macro_derive(IgniteResource, attributes(ignite))]
pub fn derive_ignite_resource(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// One field of the model, with its attributes resolved.
struct Field {
    ident: syn::Ident,
    ty: Type,
    /// The inner type, for `Option<T>`; otherwise the type itself.
    inner: Type,
    /// What the inner type parses from form text.
    base: Base,
    optional: bool,
    kind: Kind,
    label: String,
    required: bool,
    primary_key: bool,
    searchable: bool,
    sortable: bool,
    hidden_on_index: bool,
    /// A password: hashed on save, and blank on an edit keeps the stored hash.
    is_password: bool,
    /// The choices of a static select.
    choices: Vec<(String, String)>,
    /// Where a lookup field's choices come from.
    lookup: Option<Lookup>,
    /// The step of a decimal input, as written.
    step: Option<String>,
    /// The sub-fields of a repeater, group or clone, or the blocks of a flexible content
    /// field: the path of a constant.
    sub_fields: Option<syn::Path>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Textarea,
    Password,
    Number,
    Decimal,
    Checkbox,
    Select,
    Lookup,
    Date,
    DateTime,
    Email,
    Url,
    Radio,
    Color,
    Image,
    File,
    Link,
    Wysiwyg,
    PostObject,
    Relationship,
    Taxonomy,
    User,
    Repeater,
    Group,
    FlexibleContent,
    Gallery,
    Clone,
}

impl Kind {
    /// Whether `ignite_core::Field::check_value` checks the field's text: the kinds added
    /// for custom fields, except the lookups stored as an integer.
    fn checks_value(self) -> bool {
        !matches!(
            self,
            Kind::Text
                | Kind::Textarea
                | Kind::Password
                | Kind::Number
                | Kind::Decimal
                | Kind::Checkbox
                | Kind::Select
                | Kind::Lookup
                | Kind::Date
                | Kind::DateTime
        )
    }
}

/// The Rust base type the field parses from form text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Base {
    Text,
    Integer,
    Decimal,
    Bool,
    Date,
    DateTime,
}

/// The `options` attribute: fixed choices, or a function that loads them.
enum OptionsSpec {
    Static(Vec<(String, String)>),
    Dynamic(syn::Path),
}

/// Where a lookup field's choices come from.
enum Lookup {
    /// A function that returns the choices.
    Dynamic(syn::Path),
    /// The rows of a related table: `id` as the value, the `display` column as the label.
    Related {
        table: syn::Path,
        display: syn::Ident,
    },
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let vis = &input.vis;
    let fields_data = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "IgniteResource needs a struct with named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "IgniteResource can only be derived for a struct",
            ))
        }
    };

    let (table, title, singular, latest, navigation, conditions, form_layout, list_attrs) =
        struct_attrs(input)?;
    let table_name = table
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
    let title = title.unwrap_or_else(|| capitalize(&table_name));
    let singular =
        singular.unwrap_or_else(|| title.strip_suffix('s').unwrap_or(&title).to_string());

    let mut fields = Vec::new();
    for field in fields_data {
        fields.push(field_attrs(field)?);
    }

    let primary = fields
        .iter()
        .find(|f| f.primary_key)
        .or_else(|| fields.iter().find(|f| f.ident == "id"))
        .ok_or_else(|| syn::Error::new_spanned(name, "IgniteResource needs a primary key: a field named `id` or one marked `primary_key`"))?;
    if !is_key_type(&primary.inner) {
        return Err(syn::Error::new_spanned(
            &primary.ty,
            "the primary key must be an i32 or i64",
        ));
    }
    let pk_ident = primary.ident.clone();
    let pk_name = pk_ident.to_string();

    let latest_records = match &latest {
        Some(spec) => latest_records_tokens(spec, &fields, &title, name)?,
        None => quote! { ::core::option::Option::None },
    };

    let navigation_consts = navigation.consts();
    let conditions_method = conditions.map(|path| {
        quote! {
            fn conditions() -> ::ignite_core::FieldConditions {
                #path()
            }
        }
    });

    let form_layout_method = form_layout.map(|path| {
        quote! {
            fn form_layout() -> ::ignite_core::FormLayout {
                #path()
            }
        }
    });

    let list_methods = list_attrs.methods();

    let form_fields: Vec<&Field> = fields.iter().filter(|f| f.ident != pk_ident).collect();
    let input_name = format_ident!("{}Input", name);

    for f in &form_fields {
        if f.is_password && f.base != Base::Text {
            return Err(syn::Error::new_spanned(
                &f.ty,
                "a password field must be a String (or Option<String>), so the derive can hash it",
            ));
        }
        if !f.optional && !f.required && f.base != Base::Bool {
            return Err(syn::Error::new_spanned(
                &f.ty,
                "a field that is not an Option is required; remove `required = false` or make the field an Option",
            ));
        }
    }

    let input_fields = form_fields.iter().map(|f| {
        let ident = &f.ident;
        // A password is Option<String> on input, so a blank edit leaves the column as it is.
        if f.is_password {
            quote! { pub #ident: ::core::option::Option<::std::string::String> }
        } else {
            let ty = &f.ty;
            quote! { pub #ident: #ty }
        }
    });
    // The optional fields whose absence from an update keeps the stored value: every
    // `Option` that is not a password (a password has its own rule) or a checkbox (an
    // unticked checkbox sends nothing, so absent means false).
    let sent_names: Vec<String> = form_fields
        .iter()
        .filter(|f| tracks_sent(f))
        .map(|f| f.ident.to_string())
        .collect();
    let input_struct = quote! {
        #vis struct #input_name {
            #(#input_fields,)*
            /// The optional fields the form sent, blank or not. An update writes NULL for an
            /// optional field sent blank, and leaves out one that was not sent, so its stored
            /// value is kept. Filled by `validate` and `validate_update`.
            #[doc(hidden)]
            pub __ignite_sent: ::std::vec::Vec<&'static str>,
        }
    };

    // The columns an insert or update writes, from the input struct.
    let value_steps = form_fields.iter().map(|f| value_step(f));

    let lookup_fields: Vec<&Field> = form_fields
        .iter()
        .copied()
        .filter(|f| f.lookup.is_some())
        .collect();
    let options_fn = options_method(&lookup_fields);

    let field_defs = form_fields.iter().map(|f| {
        let name_lit = f.ident.to_string();
        let label = &f.label;
        let kind = kind_tokens(f);
        let required = f.required;
        quote! {
            ::ignite_core::Field {
                name: #name_lit,
                label: #label,
                kind: #kind,
                required: #required,
            }
        }
    });

    let column_defs = fields
        .iter()
        .filter(|f| !f.hidden_on_index && !f.is_password)
        .map(|f| {
            let key = f.ident.to_string();
            let label = &f.label;
            let sortable = f.sortable;
            let searchable = f.searchable;
            quote! {
                ::ignite_core::Column {
                    key: #key,
                    label: #label,
                    sortable: #sortable,
                    searchable: #searchable,
                }
            }
        });

    let searchable: Vec<String> = fields
        .iter()
        .filter(|f| f.searchable)
        .map(|f| f.ident.to_string())
        .collect();
    // One LIKE per searchable column, joined with OR inside a group, as the search always was.
    let search_filter = if searchable.is_empty() {
        quote! {}
    } else {
        let first = &searchable[0];
        let rest = &searchable[1..];
        quote! {
            if let ::core::option::Option::Some(pattern) = &pattern {
                let like = |q: ::orbit_orm::Query<#name>| {
                    q.where_group(|q| {
                        q.where_op(#first, ::orbit_orm::Operator::Like, pattern.clone())
                            #(.or_where_op(#rest, ::orbit_orm::Operator::Like, pattern.clone()))*
                    })
                };
                rows = like(rows);
                count = like(count);
            }
        }
    };

    let sort_arms = fields.iter().filter(|f| f.sortable).map(|f| {
        let key = f.ident.to_string();
        quote! { (::core::option::Option::Some(#key), true) => (#key, ::orbit_orm::Direction::Asc), (::core::option::Option::Some(#key), false) => (#key, ::orbit_orm::Direction::Desc), }
    });

    let validate_steps = form_fields.iter().map(|f| validate_expr(f));
    let validate_idents: Vec<&syn::Ident> = form_fields.iter().map(|f| &f.ident).collect();
    let validate_patterns = form_fields.iter().map(|f| {
        let ident = &f.ident;
        if f.optional || f.is_password {
            quote! { #ident }
        } else {
            quote! { ::core::option::Option::Some(#ident) }
        }
    });

    let table_check = quote! {
        const _: () = ::core::assert!(
            ::ignite_core::table_name_is(<#name as ::orbit_orm::Model>::TABLE, #table_name),
            "the #[ignite(table = ...)] attribute must name the table of the model's #[orbit(table = ...)]"
        );
    };

    Ok(quote! {
        #table_check

        #input_struct

        impl #name {
            /// The columns that an insert (`creating`) or an update write, from the input.
            /// An `Option` that is `None` is written as NULL on create. On update it is written
            /// as NULL when the form sent it blank, so a cleared optional field is cleared, and
            /// skipped when the form did not send it. A blank password is skipped on update.
            #[doc(hidden)]
            #[allow(unused_mut)]
            fn __ignite_values(
                input: &#input_name,
                creating: bool,
            ) -> ::std::vec::Vec<(&'static str, ::orbit_orm::Value)> {
                let _ = creating;
                let mut values = ::std::vec::Vec::new();
                #(#value_steps)*
                values
            }

            /// Shared by `validate` and `validate_update`. `creating` decides
            /// whether a blank password is an error.
            #[doc(hidden)]
            #[allow(clippy::nonminimal_bool, clippy::needless_bool)]
            fn __ignite_validate(
                form: &::ignite_core::FormInput,
                creating: bool,
            ) -> ::core::result::Result<#input_name, ::ignite_core::FieldErrors> {
                let _ = creating;
                let mut errors = ::ignite_core::FieldErrors::new();
                #(#validate_steps)*
                let __ignite_optional: &[&'static str] = &[#(#sent_names),*];
                let __ignite_sent: ::std::vec::Vec<&'static str> = __ignite_optional
                    .iter()
                    .copied()
                    .filter(|name| form.has(name))
                    .collect();
                match (#(#validate_idents,)*) {
                    (#(#validate_patterns,)*) if errors.is_empty() => {
                        ::core::result::Result::Ok(#input_name { #(#validate_idents,)* __ignite_sent })
                    }
                    _ => ::core::result::Result::Err(errors),
                }
            }
        }

        impl ::ignite_core::IgniteResource for #name {
            type Record = #name;
            type Input = #input_name;

            const SLUG: &'static str = #table_name;
            const TITLE: &'static str = #title;
            const SINGULAR: &'static str = #singular;
            #navigation_consts

            fn fields() -> &'static [::ignite_core::Field] {
                &[#(#field_defs),*]
            }

            fn columns() -> &'static [::ignite_core::Column] {
                &[#(#column_defs),*]
            }

            async fn list(
                db: &::ignite_core::Database,
                query: &::ignite_core::ListQuery,
            ) -> ::core::result::Result<::ignite_core::Page<#name>, ::orbit_orm::Error> {
                let pattern = query
                    .search
                    .as_ref()
                    .map(|s| format!("%{}%", ::ignite_core::escape_like(s)));
                let mut rows = query.filter.apply(<#name as ::orbit_orm::Model>::query());
                let mut count = query.filter.apply(<#name as ::orbit_orm::Model>::query());
                #search_filter

                let asc = !matches!(query.sort, ::core::option::Option::Some((_, ::ignite_core::Direction::Desc)));
                let (column, direction) = match (query.sort.as_ref().map(|(k, _)| k.as_str()), asc) {
                    #(#sort_arms)*
                    _ => (#pk_name, ::orbit_orm::Direction::Asc),
                };
                rows = rows.order_by(column, direction);

                let total = count.count(db).await?;
                let rows = rows
                    .limit(query.per_page.max(0) as u64)
                    .offset(query.offset().max(0) as u64)
                    .get(db)
                    .await?;
                Ok(::ignite_core::Page { rows, total })
            }

            async fn find(
                db: &::ignite_core::Database,
                id: i64,
            ) -> ::core::result::Result<::core::option::Option<#name>, ::orbit_orm::Error> {
                <#name as ::orbit_orm::Model>::query().find(db, id).await
            }

            fn latest_records() -> ::core::option::Option<::ignite_core::LatestRecords> {
                #latest_records
            }

            #conditions_method
            #form_layout_method
            #list_methods

            async fn latest(
                db: &::ignite_core::Database,
                limit: i64,
            ) -> ::core::result::Result<::std::vec::Vec<#name>, ::orbit_orm::Error> {
                <#name as ::orbit_orm::Model>::query()
                    .order_by(#pk_name, ::orbit_orm::Direction::Desc)
                    .limit(limit.max(0) as u64)
                    .get(db)
                    .await
            }

            async fn create(
                db: &::ignite_core::Database,
                input: #input_name,
            ) -> ::core::result::Result<#name, ::orbit_orm::Error> {
                let values = #name::__ignite_values(&input, true);
                let id = <#name as ::orbit_orm::Model>::query().insert(db, &values).await?;
                <#name as ::ignite_core::IgniteResource>::find(db, id as i64)
                    .await?
                    .ok_or(::orbit_orm::Error::NotFound { table: <#name as ::orbit_orm::Model>::TABLE })
            }

            async fn update(
                db: &::ignite_core::Database,
                id: i64,
                input: #input_name,
            ) -> ::core::result::Result<#name, ::orbit_orm::Error> {
                let values = #name::__ignite_values(&input, false);
                // An update that writes nothing (every field it has was not sent) succeeds
                // without a query, and returns the row when it exists.
                if !values.is_empty() {
                    let changed = <#name as ::orbit_orm::Model>::query()
                        .where_eq(#pk_name, id)
                        .update(db, &values)
                        .await?;
                    if changed == 0 {
                        return Err(::orbit_orm::Error::NotFound { table: <#name as ::orbit_orm::Model>::TABLE });
                    }
                }
                <#name as ::ignite_core::IgniteResource>::find(db, id)
                    .await?
                    .ok_or(::orbit_orm::Error::NotFound { table: <#name as ::orbit_orm::Model>::TABLE })
            }

            async fn delete(
                db: &::ignite_core::Database,
                id: i64,
            ) -> ::core::result::Result<u64, ::orbit_orm::Error> {
                <#name as ::orbit_orm::Model>::query()
                    .where_eq(#pk_name, id)
                    .delete(db)
                    .await
            }

            fn validate(
                form: &::ignite_core::FormInput,
            ) -> ::core::result::Result<#input_name, ::ignite_core::FieldErrors> {
                #name::__ignite_validate(form, true)
            }

            fn validate_update(
                form: &::ignite_core::FormInput,
            ) -> ::core::result::Result<#input_name, ::ignite_core::FieldErrors> {
                #name::__ignite_validate(form, false)
            }

            #options_fn
        }
    })
}

/// One block of the generated `__ignite_values`: pushes the column of `field`. An `Option`
/// that is `None` is written as NULL on create. On update (since 0.3.0) it is written as NULL
/// when the form sent it blank, so clearing an optional field on the edit form clears the
/// stored value, and skipped when the form did not send it, so the stored value is kept, as
/// Laravel's `update` with validated data leaves out a key the request did not have. A
/// password is the exception: a blank one is NULL on create and skipped on update, so the
/// stored hash is kept.
fn value_step(field: &Field) -> TokenStream2 {
    let ident = &field.ident;
    let column = ident.to_string();
    let convert = value_of(field.base, quote! { x });
    // A password's input is an `Option` too, so a blank edit leaves the column alone.
    if field.is_password {
        quote! {
            match &input.#ident {
                ::core::option::Option::Some(x) => values.push((#column, #convert)),
                ::core::option::Option::None if creating => values.push((#column, ::orbit_orm::Value::Null)),
                ::core::option::Option::None => {}
            }
        }
    } else if tracks_sent(field) {
        quote! {
            match &input.#ident {
                ::core::option::Option::Some(x) => values.push((#column, #convert)),
                ::core::option::Option::None if creating || input.__ignite_sent.contains(&#column) => {
                    values.push((#column, ::orbit_orm::Value::Null))
                }
                ::core::option::Option::None => {}
            }
        }
    } else if field.optional {
        // An optional checkbox: always Some, as an unticked box is false.
        quote! {
            match &input.#ident {
                ::core::option::Option::Some(x) => values.push((#column, #convert)),
                ::core::option::Option::None => values.push((#column, ::orbit_orm::Value::Null)),
            }
        }
    } else {
        quote! {
            {
                let x = &input.#ident;
                values.push((#column, #convert));
            }
        }
    }
}

/// Whether an update keeps the stored value of `field` when the form does not send it: an
/// `Option` that is neither a password nor a checkbox.
fn tracks_sent(field: &Field) -> bool {
    field.optional && !field.is_password && field.base != Base::Bool
}

/// The `Value` of a reference `x` to a field's base value.
fn value_of(base: Base, x: TokenStream2) -> TokenStream2 {
    match base {
        Base::Text => quote! { ::orbit_orm::Value::from(#x.as_str()) },
        Base::Integer => quote! { ::orbit_orm::Value::from(*#x as i64) },
        Base::Decimal => quote! { ::orbit_orm::Value::from(*#x as f64) },
        Base::Bool | Base::Date | Base::DateTime => quote! { ::orbit_orm::Value::from(*#x) },
    }
}

/// The `#[ignite(latest_records(...))]` attribute, as written.
struct LatestSpec {
    /// The table heading. Defaults to "Latest" and the title.
    title: Option<String>,
    /// The most rows the dashboard table shows.
    limit: i64,
    /// Each column key, with the span of the string it came from.
    columns: Vec<(String, Span)>,
}

/// The navigation attributes of the struct, as Filament's resource properties. Each one left
/// out keeps the trait's default.
#[derive(Default)]
struct Navigation {
    label: Option<String>,
    group: Option<String>,
    sort: Option<i32>,
    parent_item: Option<String>,
    icon: Option<String>,
    create_item: Option<bool>,
    create_label: Option<String>,
}

impl Navigation {
    /// The `NAVIGATION_*` constants of the attributes that were given.
    fn consts(&self) -> TokenStream2 {
        let label = self.label.as_ref().map(|label| {
            quote! { const NAVIGATION_LABEL: ::core::option::Option<&'static str> = ::core::option::Option::Some(#label); }
        });
        let group = self.group.as_ref().map(|group| {
            quote! { const NAVIGATION_GROUP: ::core::option::Option<&'static str> = ::core::option::Option::Some(#group); }
        });
        let sort = self.sort.map(|sort| {
            quote! { const NAVIGATION_SORT: ::core::option::Option<i32> = ::core::option::Option::Some(#sort); }
        });
        let parent_item = self.parent_item.as_ref().map(|parent| {
            quote! { const NAVIGATION_PARENT_ITEM: ::core::option::Option<&'static str> = ::core::option::Option::Some(#parent); }
        });
        let icon = self.icon.as_ref().map(|icon| {
            quote! { const NAVIGATION_ICON: ::core::option::Option<&'static str> = ::core::option::Option::Some(#icon); }
        });
        let create_item = self.create_item.map(|listed| {
            quote! { const NAVIGATION_CREATE_ITEM: bool = #listed; }
        });
        let create_label = self.create_label.as_ref().map(|label| {
            quote! { const NAVIGATION_CREATE_LABEL: &'static str = #label; }
        });
        quote! { #label #group #sort #parent_item #icon #create_item #create_label }
    }
}

/// What the struct-level attributes give: the table, the title, the singular name, the
/// dashboard table, the navigation, the paths of the functions that give the conditions
/// and the form layout, and the list page's tabs and badges.
type StructAttrs = (
    syn::Path,
    Option<String>,
    Option<String>,
    Option<LatestSpec>,
    Navigation,
    Option<syn::Path>,
    Option<syn::Path>,
    ListAttrs,
);

/// The list page's attributes: the tabs, the default tab and the badge columns.
#[derive(Default)]
struct ListAttrs {
    tabs: Option<syn::Path>,
    default_active_tab: Option<String>,
    badges: Option<syn::Path>,
    tab_counts_follow_search: Option<bool>,
    hide_empty_tabs: Option<bool>,
}

impl ListAttrs {
    /// The `tabs`, `default_active_tab` and `column_badges` methods of the attributes given.
    fn methods(&self) -> TokenStream2 {
        let tabs = self.tabs.as_ref().map(|path| {
            quote! {
                fn tabs() -> ::std::vec::Vec<::ignite_core::Tab> {
                    #path()
                }
            }
        });
        let default = self.default_active_tab.as_ref().map(|key| {
            quote! {
                fn default_active_tab() -> ::core::option::Option<&'static str> {
                    ::core::option::Option::Some(#key)
                }
            }
        });
        let badges = self.badges.as_ref().map(|path| {
            quote! {
                fn column_badges() -> ::ignite_core::ColumnBadges {
                    #path()
                }
            }
        });
        let follow = self.tab_counts_follow_search.map(|follow| {
            quote! {
                fn tab_counts_follow_search() -> bool {
                    #follow
                }
            }
        });
        let hide = self.hide_empty_tabs.map(|hide| {
            quote! {
                fn hide_empty_tabs() -> bool {
                    #hide
                }
            }
        });
        quote! { #tabs #default #badges #follow #hide }
    }
}

/// Reads `#[ignite(...)]` on the struct: the table, the title, the singular name, the
/// dashboard table, the navigation, the conditions and the form layout.
fn struct_attrs(input: &DeriveInput) -> syn::Result<StructAttrs> {
    let mut table = None;
    let mut title = None;
    let mut singular = None;
    let mut latest = None;
    let mut navigation = Navigation::default();
    let mut conditions = None;
    let mut form_layout = None;
    let mut list = ListAttrs::default();
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("ignite")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                table = Some(meta.value()?.parse::<syn::Path>()?);
                Ok(())
            } else if meta.path.is_ident("title") {
                title = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("singular") {
                singular = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("latest_records") {
                latest = Some(latest_spec(&meta)?);
                Ok(())
            } else if meta.path.is_ident("navigation_label") {
                navigation.label = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("navigation_group") {
                navigation.group = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("navigation_parent_item") {
                navigation.parent_item = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("navigation_create_item") {
                navigation.create_item = Some(meta.value()?.parse::<syn::LitBool>()?.value);
                Ok(())
            } else if meta.path.is_ident("navigation_create_label") {
                navigation.create_label = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("navigation_icon") {
                let icon = meta.value()?.parse::<LitStr>()?;
                let name = icon.value();
                // Before 0.6.2 the value was CSS classes, such as `dashicons dashicons-admin-post`.
                if name.trim().is_empty() || name.chars().any(char::is_whitespace) {
                    return Err(syn::Error::new(
                        icon.span(),
                        "`navigation_icon` takes an icon name such as \"post\", \"media\" or \"settings\", which the admin theme maps to its icon, not CSS classes",
                    ));
                }
                navigation.icon = Some(name);
                Ok(())
            } else if meta.path.is_ident("navigation_sort") {
                navigation.sort = Some(sort_value(&meta.value()?.parse::<syn::Expr>()?)?);
                Ok(())
            } else if meta.path.is_ident("conditions") {
                conditions = Some(meta.value()?.parse::<syn::Path>()?);
                Ok(())
            } else if meta.path.is_ident("form_layout") {
                form_layout = Some(meta.value()?.parse::<syn::Path>()?);
                Ok(())
            } else if meta.path.is_ident("tabs") {
                list.tabs = Some(meta.value()?.parse::<syn::Path>()?);
                Ok(())
            } else if meta.path.is_ident("default_active_tab") {
                list.default_active_tab = Some(meta.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else if meta.path.is_ident("badges") {
                list.badges = Some(meta.value()?.parse::<syn::Path>()?);
                Ok(())
            } else if meta.path.is_ident("tab_counts_follow_search") {
                list.tab_counts_follow_search = Some(meta.value()?.parse::<syn::LitBool>()?.value);
                Ok(())
            } else if meta.path.is_ident("hide_empty_tabs") {
                list.hide_empty_tabs = Some(meta.value()?.parse::<syn::LitBool>()?.value);
                Ok(())
            } else {
                Err(meta.error(
                    "unknown ignite attribute on the struct; expected `table`, `title`, `singular`, `latest_records`, `navigation_label`, `navigation_group`, `navigation_sort`, `navigation_parent_item`, `navigation_icon`, `navigation_create_item`, `navigation_create_label`, `conditions`, `form_layout`, `tabs`, `default_active_tab`, `badges`, `tab_counts_follow_search` or `hide_empty_tabs`",
                ))
            }
        })?;
    }
    let table = table.ok_or_else(|| {
        syn::Error::new_spanned(
            &input.ident,
            "IgniteResource needs #[ignite(table = <table name>)]",
        )
    })?;
    Ok((
        table,
        title,
        singular,
        latest,
        navigation,
        conditions,
        form_layout,
        list,
    ))
}

/// The `i32` of a `navigation_sort` value: an integer literal, or a negative one such as `-1`.
fn sort_value(expr: &syn::Expr) -> syn::Result<i32> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(int),
            ..
        }) => int.base10_parse::<i32>(),
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr: inner,
            ..
        }) => match inner.as_ref() {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(int),
                ..
            }) => format!("-{}", int.base10_digits())
                .parse::<i32>()
                .map_err(|_| syn::Error::new_spanned(expr, "navigation_sort must fit in an i32")),
            _ => Err(syn::Error::new_spanned(
                expr,
                "navigation_sort takes a whole number",
            )),
        },
        _ => Err(syn::Error::new_spanned(
            expr,
            "navigation_sort takes a whole number",
        )),
    }
}

/// Reads `latest_records(limit = 5, columns = "name,email", title = "...")`.
fn latest_spec(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<LatestSpec> {
    let mut title = None;
    let mut limit = 5i64;
    let mut columns = None;
    meta.parse_nested_meta(|inner| {
        if inner.path.is_ident("limit") {
            let value: syn::LitInt = inner.value()?.parse()?;
            limit = value.base10_parse::<i64>()?;
            if limit < 1 {
                return Err(syn::Error::new(value.span(), "limit must be at least 1"));
            }
            Ok(())
        } else if inner.path.is_ident("columns") {
            let list: LitStr = inner.value()?.parse()?;
            let span = list.span();
            let names: Vec<(String, Span)> = list
                .value()
                .split(',')
                .map(|name| (name.trim().to_string(), span))
                .collect();
            columns = Some(names);
            Ok(())
        } else if inner.path.is_ident("title") {
            title = Some(inner.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else {
            Err(inner.error("latest_records takes limit, columns and title"))
        }
    })?;
    let columns = columns.ok_or_else(|| {
        meta.error(
            "latest_records needs columns = \"name,email\": the columns the dashboard table shows",
        )
    })?;
    Ok(LatestSpec {
        title,
        limit,
        columns,
    })
}

/// Writes the body of `latest_records()` from the attribute, after checking each
/// column: it must be a field of the model, and it must not be a password.
fn latest_records_tokens(
    spec: &LatestSpec,
    fields: &[Field],
    title: &str,
    name: &syn::Ident,
) -> syn::Result<TokenStream2> {
    let mut keys = Vec::new();
    for (key, span) in &spec.columns {
        if key.is_empty() {
            return Err(syn::Error::new(
                *span,
                "latest_records has an empty column name",
            ));
        }
        let field = fields
            .iter()
            .find(|f| f.ident == key.as_str())
            .ok_or_else(|| syn::Error::new(*span, format!("`{key}` is not a field of {name}")))?;
        if field.is_password {
            return Err(syn::Error::new(
                *span,
                format!("`{key}` is a password and cannot be shown on the dashboard"),
            ));
        }
        keys.push(key.clone());
    }
    let table_title = spec
        .title
        .clone()
        .unwrap_or_else(|| format!("Latest {title}"));
    let limit = spec.limit;
    Ok(quote! {
        ::core::option::Option::Some(::ignite_core::LatestRecords {
            title: #table_title,
            columns: &[#(#keys),*],
            limit: #limit,
        })
    })
}

/// Reads the `#[ignite(...)]` attributes of a field and resolves its defaults.
fn field_attrs(field: &syn::Field) -> syn::Result<Field> {
    let ident = field
        .ident
        .clone()
        .ok_or_else(|| syn::Error::new_spanned(field, "IgniteResource needs named fields"))?;
    let ty = field.ty.clone();
    let (optional, inner) = match option_inner(&ty) {
        Some(inner) => (true, inner.clone()),
        None => (false, ty.clone()),
    };
    let base = base_of(&inner).ok_or_else(|| {
        syn::Error::new_spanned(
            &ty,
            "unsupported field type: use String, i16, i32, i64, f32, f64, bool, chrono::NaiveDate, chrono::NaiveDateTime, or Option of one",
        )
    })?;

    let mut label = None;
    let mut form_type = None;
    let mut required_attr = None;
    let mut primary_key = false;
    let mut searchable = false;
    let mut sortable = false;
    let mut hidden_on_index = false;
    let mut options = None;
    let mut belongs_to = None;
    let mut display = None;
    let mut step = None;
    let mut sub_fields = None;
    let mut blocks = None;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("ignite")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                label = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident("form_type") {
                form_type = Some(meta.value()?.parse::<LitStr>()?);
            } else if meta.path.is_ident("required") {
                let value = if meta.input.peek(Token![=]) {
                    meta.value()?.parse::<LitBool>()?.value
                } else {
                    true
                };
                required_attr = Some(value);
            } else if meta.path.is_ident("primary_key") {
                primary_key = true;
            } else if meta.path.is_ident("searchable") {
                searchable = true;
            } else if meta.path.is_ident("sortable") {
                sortable = true;
            } else if meta.path.is_ident("hidden_on_index") {
                hidden_on_index = true;
            } else if meta.path.is_ident("options") {
                let expr: syn::Expr = meta.value()?.parse()?;
                options = Some(options_of(&expr)?);
            } else if meta.path.is_ident("belongs_to") {
                belongs_to = Some(meta.value()?.parse::<syn::Path>()?);
            } else if meta.path.is_ident("display") {
                display = Some(meta.value()?.parse::<syn::Ident>()?);
            } else if meta.path.is_ident("step") {
                step = Some(meta.value()?.parse::<LitStr>()?);
            } else if meta.path.is_ident("fields") {
                sub_fields = Some(meta.value()?.parse::<syn::Path>()?);
            } else if meta.path.is_ident("blocks") {
                blocks = Some(meta.value()?.parse::<syn::Path>()?);
            } else {
                return Err(meta.error(
                    "unknown ignite attribute; expected label, form_type, required, primary_key, searchable, sortable, hidden_on_index, options, belongs_to, display, step, fields or blocks",
                ));
            }
            Ok(())
        })?;
    }

    // Options and a relation both make a select, so neither takes a form_type of its own,
    // except the kinds that are a select by another name: radio buttons for fixed options,
    // and a post object, taxonomy, user or relationship for loaded ones.
    let lookup_attr = options.is_some() || belongs_to.is_some();
    let choice_kind = match form_type.as_ref().map(|s| s.value()).as_deref() {
        Some("radio") if matches!(options, Some(OptionsSpec::Static(_))) => Some(Kind::Radio),
        Some("post_object") if lookup_attr && !matches!(options, Some(OptionsSpec::Static(_))) => {
            Some(Kind::PostObject)
        }
        Some("taxonomy") if lookup_attr && !matches!(options, Some(OptionsSpec::Static(_))) => {
            Some(Kind::Taxonomy)
        }
        Some("user") if lookup_attr && !matches!(options, Some(OptionsSpec::Static(_))) => {
            Some(Kind::User)
        }
        Some("relationship") if matches!(options, Some(OptionsSpec::Dynamic(_))) => {
            Some(Kind::Relationship)
        }
        _ => None,
    };
    if lookup_attr && form_type.is_some() && choice_kind.is_none() {
        return Err(syn::Error::new_spanned(
            &ty,
            "a field with options or belongs_to is a select: remove form_type",
        ));
    }
    if belongs_to.is_some() != display.is_some() {
        return Err(syn::Error::new_spanned(
            &ty,
            "belongs_to and display go together: belongs_to = <model type>, display = <column>",
        ));
    }
    if options.is_some() && belongs_to.is_some() {
        return Err(syn::Error::new_spanned(
            &ty,
            "a field takes options or belongs_to, not both",
        ));
    }

    let mut choices = Vec::new();
    let mut lookup = None;
    let kind = if let Some(spec) = options {
        match spec {
            OptionsSpec::Static(pairs) => {
                if base != Base::Text {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "a field with a list of options must be a String (or Option<String>)",
                    ));
                }
                choices = pairs;
                choice_kind.unwrap_or(Kind::Select)
            }
            OptionsSpec::Dynamic(path) => {
                if !matches!(base, Base::Text | Base::Integer) {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "a field with options from a function must be a String or an integer",
                    ));
                }
                if choice_kind == Some(Kind::Relationship) && base != Base::Text {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "a relationship stores a JSON list, so it must be a String (or Option<String>)",
                    ));
                }
                lookup = Some(Lookup::Dynamic(path));
                choice_kind.unwrap_or(Kind::Lookup)
            }
        }
    } else if let Some(table) = belongs_to {
        if base != Base::Integer {
            return Err(syn::Error::new_spanned(
                &ty,
                "a belongs_to field stores the related row's id, so it must be an i32 or i64",
            ));
        }
        lookup = Some(Lookup::Related {
            table,
            display: display
                .clone()
                .expect("checked: display is set with belongs_to"),
        });
        choice_kind.unwrap_or(Kind::Lookup)
    } else {
        match form_type.as_ref().map(|s| s.value()) {
            None => match base {
                Base::Bool => Kind::Checkbox,
                Base::Integer => Kind::Number,
                Base::Decimal => Kind::Decimal,
                Base::Date => Kind::Date,
                Base::DateTime => Kind::DateTime,
                Base::Text => Kind::Text,
            },
            Some(value) => match value.as_str() {
                "text" => Kind::Text,
                "textarea" => Kind::Textarea,
                "password" => Kind::Password,
                "number" if base == Base::Decimal => Kind::Decimal,
                "number" => Kind::Number,
                "checkbox" => Kind::Checkbox,
                "date" => Kind::Date,
                "email" => Kind::Email,
                "url" => Kind::Url,
                "color" => Kind::Color,
                "image" => Kind::Image,
                "file" => Kind::File,
                "link" => Kind::Link,
                "wysiwyg" => Kind::Wysiwyg,
                "gallery" => Kind::Gallery,
                "repeater" => Kind::Repeater,
                "group" => Kind::Group,
                "clone" => Kind::Clone,
                "flexible_content" => Kind::FlexibleContent,
                "radio" | "post_object" | "taxonomy" | "user" | "relationship" => {
                    return Err(syn::Error::new(
                        form_type.map(|s| s.span()).unwrap_or_else(Span::call_site),
                        "form_type radio takes options = [...]; post_object, taxonomy and user take options = <function> or belongs_to; relationship takes options = <function>",
                    ))
                }
                _ => return Err(syn::Error::new(
                    form_type.map(|s| s.span()).unwrap_or_else(Span::call_site),
                    "form_type must be one of text, textarea, password, number, checkbox, date, email, url, color, image, file, link, wysiwyg, gallery, radio, post_object, taxonomy, user, relationship, repeater, group, clone or flexible_content",
                )),
            },
        }
    };

    // A structured field names its sub-fields (or blocks), and only it does.
    let sub_fields = match kind {
        Kind::Repeater | Kind::Group | Kind::Clone => match (sub_fields, blocks) {
            (Some(path), None) => Some(path),
            _ => {
                return Err(syn::Error::new_spanned(
                    &ty,
                    "a repeater, group or clone takes fields = <a &'static [ignite_core::Field] constant>",
                ))
            }
        },
        Kind::FlexibleContent => match (blocks, sub_fields) {
            (Some(path), None) => Some(path),
            _ => {
                return Err(syn::Error::new_spanned(
                    &ty,
                    "a flexible_content field takes blocks = <a &'static [ignite_core::Block] constant>",
                ))
            }
        },
        _ if sub_fields.is_some() || blocks.is_some() => {
            return Err(syn::Error::new_spanned(
                &ty,
                "fields and blocks apply to a repeater, group, clone or flexible_content field",
            ))
        }
        _ => None,
    };
    // The kinds for custom fields store text, except a media id, which may be an integer.
    let text_kind =
        kind.checks_value() && !matches!(kind, Kind::PostObject | Kind::Taxonomy | Kind::User);
    if text_kind
        && base != Base::Text
        && !(matches!(kind, Kind::Image | Kind::File) && base == Base::Integer)
    {
        return Err(syn::Error::new_spanned(
            &ty,
            "this form_type stores text, so the field must be a String (or Option<String>); an image or file may also be an integer",
        ));
    }

    if step.is_some() && base != Base::Decimal {
        return Err(syn::Error::new_spanned(
            &ty,
            "step applies to an f32 or f64 field",
        ));
    }

    // A checkbox is never blank, so it is not required unless asked.
    let required = required_attr.unwrap_or(!optional && base != Base::Bool);
    let label = label.unwrap_or_else(|| capitalize(&ident.to_string().replace('_', " ")));

    Ok(Field {
        ident,
        ty,
        inner,
        base,
        optional,
        kind,
        label,
        required,
        primary_key,
        searchable,
        sortable,
        hidden_on_index,
        is_password: kind == Kind::Password,
        choices,
        lookup,
        step: step.map(|s| s.value()),
        sub_fields,
    })
}

/// The `options = ...` attribute: a list of `("value", "Label")` pairs, or the path of a
/// function that returns the choices from the database.
fn options_of(expr: &syn::Expr) -> syn::Result<OptionsSpec> {
    match expr {
        syn::Expr::Array(array) => {
            let mut pairs = Vec::new();
            for element in &array.elems {
                let syn::Expr::Tuple(tuple) = element else {
                    return Err(syn::Error::new_spanned(
                        element,
                        "each option is a pair, such as (\"draft\", \"Draft\")",
                    ));
                };
                let [value, label] = tuple.elems.iter().collect::<Vec<_>>()[..] else {
                    return Err(syn::Error::new_spanned(
                        tuple,
                        "each option is a pair, such as (\"draft\", \"Draft\")",
                    ));
                };
                pairs.push((string_literal(value)?, string_literal(label)?));
            }
            Ok(OptionsSpec::Static(pairs))
        }
        syn::Expr::Path(path) => Ok(OptionsSpec::Dynamic(path.path.clone())),
        _ => Err(syn::Error::new_spanned(
            expr,
            "options is a list of (\"value\", \"Label\") pairs, or the path of a function that returns them",
        )),
    }
}

/// The text of a string literal, or an error at the expression.
fn string_literal(expr: &syn::Expr) -> syn::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(text),
            ..
        }) => Ok(text.value()),
        _ => Err(syn::Error::new_spanned(expr, "expected a string literal")),
    }
}

/// The type inside `Option<T>`, if the type is one.
fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    match args.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

/// What a supported base type parses from form text, or `None` if it is unsupported.
fn base_of(ty: &Type) -> Option<Base> {
    let Type::Path(path) = ty else { return None };
    let ident = path.path.segments.last()?.ident.to_string();
    match ident.as_str() {
        "String" => Some(Base::Text),
        "i16" | "i32" | "i64" => Some(Base::Integer),
        "f32" | "f64" => Some(Base::Decimal),
        "bool" => Some(Base::Bool),
        "NaiveDate" => Some(Base::Date),
        "NaiveDateTime" => Some(Base::DateTime),
        _ => None,
    }
}

/// Whether a primary key has a type the query methods accept: `i32` or `i64`.
fn is_key_type(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    path.path
        .segments
        .last()
        .is_some_and(|s| matches!(s.ident.to_string().as_str(), "i32" | "i64"))
}

/// The `FieldKind` a field's form input has, as tokens.
fn kind_tokens(field: &Field) -> TokenStream2 {
    match field.kind {
        Kind::Text => quote! { ::ignite_core::FieldKind::Text },
        Kind::Textarea => quote! { ::ignite_core::FieldKind::Textarea },
        Kind::Password => quote! { ::ignite_core::FieldKind::Password },
        Kind::Number => quote! { ::ignite_core::FieldKind::Number },
        Kind::Decimal => {
            let step = field.step.as_deref().unwrap_or("any");
            quote! { ::ignite_core::FieldKind::Decimal(#step) }
        }
        Kind::Checkbox => quote! { ::ignite_core::FieldKind::Checkbox },
        Kind::Select => {
            let pairs = choice_pairs(&field.choices);
            quote! { ::ignite_core::FieldKind::Select(&[#(#pairs),*]) }
        }
        Kind::Lookup => quote! { ::ignite_core::FieldKind::Lookup },
        Kind::Date => quote! { ::ignite_core::FieldKind::Date },
        Kind::DateTime => quote! { ::ignite_core::FieldKind::DateTime },
        Kind::Email => quote! { ::ignite_core::FieldKind::Email },
        Kind::Url => quote! { ::ignite_core::FieldKind::Url },
        Kind::Radio => {
            let pairs = choice_pairs(&field.choices);
            quote! { ::ignite_core::FieldKind::Radio(&[#(#pairs),*]) }
        }
        Kind::Color => quote! { ::ignite_core::FieldKind::Color },
        Kind::Image => quote! { ::ignite_core::FieldKind::Image },
        Kind::File => quote! { ::ignite_core::FieldKind::File },
        Kind::Link => quote! { ::ignite_core::FieldKind::Link },
        Kind::Wysiwyg => quote! { ::ignite_core::FieldKind::Wysiwyg },
        Kind::PostObject => quote! { ::ignite_core::FieldKind::PostObject },
        Kind::Relationship => quote! { ::ignite_core::FieldKind::Relationship },
        Kind::Taxonomy => quote! { ::ignite_core::FieldKind::Taxonomy },
        Kind::User => quote! { ::ignite_core::FieldKind::User },
        Kind::Gallery => quote! { ::ignite_core::FieldKind::Gallery },
        Kind::Repeater | Kind::Group | Kind::Clone | Kind::FlexibleContent => {
            let path = field
                .sub_fields
                .as_ref()
                .expect("checked: a structured field has its sub-fields");
            match field.kind {
                Kind::Repeater => quote! { ::ignite_core::FieldKind::Repeater(#path) },
                Kind::Group => quote! { ::ignite_core::FieldKind::Group(#path) },
                Kind::Clone => quote! { ::ignite_core::FieldKind::Clone(#path) },
                _ => quote! { ::ignite_core::FieldKind::FlexibleContent(#path) },
            }
        }
    }
}

/// The `(value, label)` tuples of a static list of options.
fn choice_pairs(choices: &[(String, String)]) -> Vec<TokenStream2> {
    choices
        .iter()
        .map(|(value, label)| quote! { (#value, #label) })
        .collect()
}

/// One `let` in `validate`: reads the field from the form into an `Option` of its base value,
/// and records an error when the value is blank (required) or does not parse.
fn validate_expr(field: &Field) -> TokenStream2 {
    let ident = &field.ident;
    let name = ident.to_string();
    let label = &field.label;
    let kind = kind_tokens(field);
    let required = field.required;
    let base = field.base;
    let is_password = field.is_password;
    let ty = &field.inner;

    let convert = match base {
        Base::Text if is_password => {
            let message = "Could not save this password.".to_string();
            quote! {
                match ::ignite_core::hash_password(text) {
                    Ok(hash) => ::core::option::Option::Some(hash),
                    Err(_) => {
                        errors.insert(#name.to_string(), #message.to_string());
                        ::core::option::Option::None
                    }
                }
            }
        }
        Base::Text if field.kind == Kind::Select => {
            let pairs = choice_pairs(&field.choices);
            quote! {
                {
                    let text = text.trim();
                    let choices: &[(&str, &str)] = &[#(#pairs),*];
                    if field.check_choice(text, choices, &mut errors) {
                        ::core::option::Option::Some(text.to_string())
                    } else {
                        ::core::option::Option::None
                    }
                }
            }
        }
        // The kinds for custom fields: the text their check returns.
        Base::Text if field.kind.checks_value() => quote! { field.check_value(text, &mut errors) },
        Base::Text => quote! { ::core::option::Option::Some(text.to_string()) },
        Base::Bool => quote! { ::core::option::Option::Some(form.checkbox(#name)) },
        // A media id stored as an integer: checked as a media id, then parsed.
        Base::Integer if matches!(field.kind, Kind::Image | Kind::File) => quote! {
            field
                .check_value(text, &mut errors)
                .and_then(|id| id.parse::<#ty>().ok())
        },
        Base::Integer => quote! { field.check_whole_number::<#ty>(text, &mut errors) },
        Base::Decimal => quote! { field.check_decimal::<#ty>(text, &mut errors) },
        Base::Date => quote! { field.check_date(text, &mut errors) },
        Base::DateTime => quote! { field.check_datetime(text, &mut errors) },
    };

    // Booleans are never blank: an unticked checkbox is false.
    if base == Base::Bool {
        return quote! { let #ident = #convert; };
    }

    // A password is required only when creating. On an edit a blank password
    // keeps the stored hash.
    quote! {
        let #ident = {
            let field = ::ignite_core::Field { name: #name, label: #label, kind: #kind, required: #required };
            let present = if #required && (creating || !#is_password) {
                field.check_required(form, &mut errors)
            } else {
                form.get(#name)
            };
            match present {
                ::core::option::Option::Some(text) => { let text: &str = text; #convert }
                ::core::option::Option::None => ::core::option::Option::None,
            }
        };
    }
}

/// The `options` method, which the panel calls for each lookup field. It is written
/// only when the model has one, so the trait's default serves the rest.
fn options_method(lookups: &[&Field]) -> TokenStream2 {
    if lookups.is_empty() {
        return quote! {};
    }
    let arms = lookups.iter().map(|f| {
        let name_lit = f.ident.to_string();
        match f.lookup.as_ref().expect("checked: a lookup field") {
            Lookup::Dynamic(path) => quote! { #name_lit => #path(db).await, },
            Lookup::Related { table, display } => {
                let key_ty = &f.inner;
                let display_lit = display.to_string();
                quote! {
                    #name_lit => {
                        let rows: ::std::vec::Vec<(#key_ty, ::std::string::String)> =
                            <#table as ::orbit_orm::Model>::query()
                                .select(&[<#table as ::orbit_orm::Model>::PRIMARY_KEY, #display_lit])
                                .order_by(#display_lit, ::orbit_orm::Direction::Asc)
                                .get_as(db)
                                .await?;
                        ::core::result::Result::Ok(
                            rows.into_iter().map(|(id, label)| (id.to_string(), label)).collect(),
                        )
                    }
                }
            }
        }
    });
    quote! {
        async fn options(
            db: &::ignite_core::Database,
            field: &str,
        ) -> ::core::result::Result<::std::vec::Vec<(::std::string::String, ::std::string::String)>, ::orbit_orm::Error> {
            match field {
                #(#arms)*
                _ => ::core::result::Result::Ok(::std::vec::Vec::new()),
            }
        }
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
