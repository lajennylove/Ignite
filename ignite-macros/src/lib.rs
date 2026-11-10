//! Derive macro for Ignite resources: `#[derive(IgniteResource)]`.
//!
//! The derive reads a Diesel model and writes the [`IgniteResource`] impl that
//! the panel needs: the form fields, the list columns, the query methods, and
//! the input struct that create and update take. The model must also derive
//! `Queryable`, `Selectable` and `Serialize`, because the derive does not write
//! them.
//!
//! [`IgniteResource`]: https://docs.rs/ignite-core/latest/ignite_core/trait.IgniteResource.html

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
/// - `#[ignite(table = widgets)]`: the Diesel table module, required. Its name is the URL slug.
/// - `#[ignite(title = "Widgets")]`: the heading and nav label. Defaults to the table name, capitalized.
/// - `#[ignite(singular = "Widget")]`: the name of one record, for "New Widget". Defaults to the title without a trailing `s`.
///
/// Field attributes:
/// - `primary_key`: the key column. Defaults to a field named `id`.
/// - `label = "..."`: the label. Defaults to the field name, capitalized and spaced.
/// - `form_type = "text" | "textarea" | "password" | "number" | "checkbox" | "date"`: the input. Defaults from the type.
/// - `options = [("draft", "Draft"), ...]`: a select with a fixed list of choices. The field must be a `String`.
/// - `options = load_choices`: a select whose choices come from a function, `fn(&mut MysqlConnection) -> QueryResult<Vec<(String, String)>>`, called each time the form is shown. The field must be a `String` or an integer.
/// - `belongs_to = schema::categories, display = name`: a relation. The field stores the related row's `id`, the select lists the related table's rows by `display`, and the list shows that name. The field must be an `i32` or `i64`, and the related table's key must be `id`.
/// - `step = "0.01"`: the browser's step for an `f32` or `f64` input. The default is `"any"`.
/// - `required` or `required = false`: whether a blank value is rejected. Non-`Option` fields are required by default.
/// - `searchable`, `sortable`: the list's search and sort columns.
/// - `hidden_on_index`: leaves the column out of the list.
///
/// Supported field types: `String`, `i16`, `i32`, `i64`, `f32`, `f64`, `bool`,
/// `chrono::NaiveDate`, `chrono::NaiveDateTime`, and `Option` of any of them. The primary
/// key must be `i32` or `i64`. A `String` field is a text input (or a textarea, a password
/// or a select, by attribute); an `f32` or `f64` is a decimal input; a `NaiveDateTime` is a
/// date-and-time input; a `bool` is a checkbox.
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
    let pk_ty = primary.inner.clone();

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

    // The table module, aliased so the generated code can refer to it.
    let table_path = &table;

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
    let input_struct = quote! {
        #[derive(::diesel::Insertable, ::diesel::AsChangeset)]
        #[diesel(table_name = #table_path)]
        #vis struct #input_name {
            #(#input_fields,)*
        }
    };

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

    let searchable: Vec<&Field> = fields.iter().filter(|f| f.searchable).collect();
    // One LIKE per searchable column, joined with OR. Each query gets its own
    // copy of the condition, because a boxed filter takes its expression by value.
    let search_condition = || {
        let mut conditions = searchable.iter().map(|f| {
            let col = &f.ident;
            quote! { __table::#col.like(pattern.clone()) }
        });
        let first = conditions
            .next()
            .expect("checked: at least one searchable column");
        conditions.fold(first, |acc, next| quote! { #acc.or(#next) })
    };
    let search_filter = if searchable.is_empty() {
        quote! {}
    } else {
        let rows_condition = search_condition();
        let count_condition = search_condition();
        quote! {
            if let Some(pattern) = &pattern {
                rows = rows.filter(#rows_condition);
                count = count.filter(#count_condition);
            }
        }
    };

    let sort_arms = fields.iter().filter(|f| f.sortable).flat_map(|f| {
        let key = f.ident.to_string();
        let col = &f.ident;
        vec![
            quote! { (::core::option::Option::Some(#key), true) => rows.order(__table::#col.asc()), },
            quote! { (::core::option::Option::Some(#key), false) => rows.order(__table::#col.desc()), },
        ]
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

    Ok(quote! {
        #input_struct

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

            fn list(
                conn: &mut ::diesel::mysql::MysqlConnection,
                query: &::ignite_core::ListQuery,
            ) -> ::diesel::result::QueryResult<::ignite_core::Page<#name>> {
                #[allow(unused_imports)]
                use ::diesel::prelude::*;
                #[allow(unused_imports)]
                use #table_path as __table;

                let pattern = query
                    .search
                    .as_ref()
                    .map(|s| format!("%{}%", ::ignite_core::escape_like(s)));
                let mut rows = __table::table.into_boxed();
                let mut count = __table::table.into_boxed();
                let _ = &pattern;
                #search_filter

                let asc = !matches!(query.sort, ::core::option::Option::Some((_, ::ignite_core::Direction::Desc)));
                rows = match (query.sort.as_ref().map(|(k, _)| k.as_str()), asc) {
                    #(#sort_arms)*
                    _ => rows.order(__table::#pk_ident.asc()),
                };

                let total: i64 = count.count().get_result(conn)?;
                let rows = rows
                    .limit(query.per_page)
                    .offset(query.offset())
                    .select(<#name as ::diesel::SelectableHelper<::diesel::mysql::Mysql>>::as_select())
                    .load(conn)?;
                Ok(::ignite_core::Page { rows, total })
            }

            fn find(
                conn: &mut ::diesel::mysql::MysqlConnection,
                id: i64,
            ) -> ::diesel::result::QueryResult<::core::option::Option<#name>> {
                #[allow(unused_imports)]
                use ::diesel::prelude::*;
                #[allow(unused_imports)]
                use #table_path as __table;

                let Ok(key) = <#pk_ty as ::core::convert::TryFrom<i64>>::try_from(id) else {
                    return Ok(None);
                };
                __table::table
                    .find(key)
                    .select(<#name as ::diesel::SelectableHelper<::diesel::mysql::Mysql>>::as_select())
                    .first(conn)
                    .optional()
            }

            fn latest_records() -> ::core::option::Option<::ignite_core::LatestRecords> {
                #latest_records
            }

            fn latest(
                conn: &mut ::diesel::mysql::MysqlConnection,
                limit: i64,
            ) -> ::diesel::result::QueryResult<::std::vec::Vec<#name>> {
                #[allow(unused_imports)]
                use ::diesel::prelude::*;
                #[allow(unused_imports)]
                use #table_path as __table;

                __table::table
                    .order(__table::#pk_ident.desc())
                    .limit(limit)
                    .select(<#name as ::diesel::SelectableHelper<::diesel::mysql::Mysql>>::as_select())
                    .load(conn)
            }

            fn create(
                conn: &mut ::diesel::mysql::MysqlConnection,
                input: #input_name,
            ) -> ::diesel::result::QueryResult<#name> {
                #[allow(unused_imports)]
                use ::diesel::prelude::*;
                #[allow(unused_imports)]
                use #table_path as __table;

                ::diesel::insert_into(__table::table).values(&input).execute(conn)?;
                let id: i64 = ::diesel::select(::diesel::dsl::sql::<::diesel::sql_types::BigInt>("LAST_INSERT_ID()"))
                    .get_result(conn)?;
                <#name as ::ignite_core::IgniteResource>::find(conn, id)?
                    .ok_or(::diesel::result::Error::NotFound)
            }

            fn update(
                conn: &mut ::diesel::mysql::MysqlConnection,
                id: i64,
                input: #input_name,
            ) -> ::diesel::result::QueryResult<#name> {
                #[allow(unused_imports)]
                use ::diesel::prelude::*;
                #[allow(unused_imports)]
                use #table_path as __table;

                let Ok(key) = <#pk_ty as ::core::convert::TryFrom<i64>>::try_from(id) else {
                    return Err(::diesel::result::Error::NotFound);
                };
                let changed = ::diesel::update(__table::table.find(key)).set(&input).execute(conn)?;
                if changed == 0 {
                    return Err(::diesel::result::Error::NotFound);
                }
                <#name as ::ignite_core::IgniteResource>::find(conn, id)?
                    .ok_or(::diesel::result::Error::NotFound)
            }

            fn delete(
                conn: &mut ::diesel::mysql::MysqlConnection,
                id: i64,
            ) -> ::diesel::result::QueryResult<usize> {
                #[allow(unused_imports)]
                use ::diesel::prelude::*;
                #[allow(unused_imports)]
                use #table_path as __table;

                let Ok(key) = <#pk_ty as ::core::convert::TryFrom<i64>>::try_from(id) else {
                    return Ok(0);
                };
                ::diesel::delete(__table::table.find(key)).execute(conn)
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

        impl #name {
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
                match (#(#validate_idents,)*) {
                    (#(#validate_patterns,)*) if errors.is_empty() => {
                        ::core::result::Result::Ok(#input_name { #(#validate_idents,)* })
                    }
                    _ => ::core::result::Result::Err(errors),
                }
            }
        }

    })
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
            "IgniteResource needs #[ignite(table = <diesel table module>)]",
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
            "belongs_to and display go together: belongs_to = <table module>, display = <column>",
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
                        errors.insert(#name, #message.to_string());
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
            Lookup::Dynamic(path) => quote! { #name_lit => #path(conn), },
            Lookup::Related { table, display } => {
                let key_ty = &f.inner;
                quote! {
                    #name_lit => {
                        #[allow(unused_imports)]
                        use #table as __related;
                        let rows: ::std::vec::Vec<(#key_ty, ::std::string::String)> = __related::table
                            .select((__related::id, __related::#display))
                            .order(__related::#display.asc())
                            .load(conn)?;
                        ::core::result::Result::Ok(
                            rows.into_iter().map(|(id, label)| (id.to_string(), label)).collect(),
                        )
                    }
                }
            }
        }
    });
    quote! {
        fn options(
            conn: &mut ::diesel::mysql::MysqlConnection,
            field: &str,
        ) -> ::diesel::result::QueryResult<::std::vec::Vec<(::std::string::String, ::std::string::String)>> {
            #[allow(unused_imports)]
            use ::diesel::prelude::*;
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
