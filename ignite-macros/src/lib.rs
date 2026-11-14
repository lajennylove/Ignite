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
///
/// Field attributes:
/// - `primary_key`: the key column. Defaults to a field named `id`.
/// - `label = "..."`: the label. Defaults to the field name, capitalized and spaced.
/// - `form_type = "text" | "textarea" | "password" | "number" | "checkbox" | "date"`: the input. Defaults from the type.
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
/// records which optional fields the form sent. `validate` and `validate_update` fill it;
/// code that builds the input by hand sets it to the names of the optional fields to write
/// (or to every optional field name, to write NULL for each `None`).
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

    let (table, title, singular, latest) = struct_attrs(input)?;
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
                let mut rows = <#name as ::orbit_orm::Model>::query();
                let mut count = <#name as ::orbit_orm::Model>::query();
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
                let changed = <#name as ::orbit_orm::Model>::query()
                    .where_eq(#pk_name, id)
                    .update(db, &values)
                    .await?;
                if changed == 0 {
                    return Err(::orbit_orm::Error::NotFound { table: <#name as ::orbit_orm::Model>::TABLE });
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

/// What the struct-level attributes give: the table, the title, the singular name, and
/// the dashboard table.
type StructAttrs = (
    syn::Path,
    Option<String>,
    Option<String>,
    Option<LatestSpec>,
);

/// Reads `#[ignite(...)]` on the struct: the table, the title, the singular name, and
/// the dashboard table.
fn struct_attrs(input: &DeriveInput) -> syn::Result<StructAttrs> {
    let mut table = None;
    let mut title = None;
    let mut singular = None;
    let mut latest = None;
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
            } else {
                Err(meta.error(
                    "unknown ignite attribute on the struct; expected `table`, `title`, `singular` or `latest_records`",
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
    Ok((table, title, singular, latest))
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
            } else {
                return Err(meta.error(
                    "unknown ignite attribute; expected label, form_type, required, primary_key, searchable, sortable, hidden_on_index, options, belongs_to, display or step",
                ));
            }
            Ok(())
        })?;
    }

    // Options and a relation both make a select, so neither takes a form_type of its own.
    let lookup_attr = options.is_some() || belongs_to.is_some();
    if lookup_attr && form_type.is_some() {
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
                Kind::Select
            }
            OptionsSpec::Dynamic(path) => {
                if !matches!(base, Base::Text | Base::Integer) {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "a field with options from a function must be a String or an integer",
                    ));
                }
                lookup = Some(Lookup::Dynamic(path));
                Kind::Lookup
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
        Kind::Lookup
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
                _ => return Err(syn::Error::new(
                    form_type.map(|s| s.span()).unwrap_or_else(Span::call_site),
                    "form_type must be one of text, textarea, password, number, checkbox or date",
                )),
            },
        }
    };

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
        Base::Text => quote! { ::core::option::Option::Some(text.to_string()) },
        Base::Bool => quote! { ::core::option::Option::Some(form.checkbox(#name)) },
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
