//! installation-groans' macros, re-exported by it: [`macro@cli`] makes an installer's command line
//! out of a struct, [`macro@location`] names where it installs. Use them through
//! `installation_groans::`, which is the crate their code is written against.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    Attribute, Expr, Fields, ItemStruct, LitStr, Token, Type, parse_macro_input,
    punctuated::Punctuated,
};

/// What a field of the installer's struct is.
enum Kind {
    /// `#[installation_groans::Release(INSTALLATION, "name")]`: a program the install puts in
    /// place, its payload.
    Release(Expr, LitStr),
    /// `#[installation_groans::Artifact(INSTALLATION)]`: something it downloads into the artifacts.
    Artifact(Expr),
    /// A plain `bool`: a choice the installer's own code acts on.
    Choice,
}

/// The line of doc comments on `attrs`, joined, for the help, the plan and the question.
fn doc(attrs: &[Attribute]) -> String {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(pair) => match &pair.value {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) => Some(text.value().trim().to_string()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `attr` is ours, `Release`/`Artifact` (any case, with or without `installation_groans::`).
fn ours(attr: &Attribute, name: &str) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|last| last.ident.to_string().eq_ignore_ascii_case(name))
}

/// The installer's command line, out of a struct of its parts, each with a doc comment (its help,
/// its line in the plan and its place in the question):
///
/// - `#[installation_groans::Release(INSTALLATION, "name")]` on a `()` (always installed) or a
///   `bool`: a program put in place as `name`, the payload;
/// - `#[installation_groans::Artifact(INSTALLATION)]` on a `bool` (or `()`): something downloaded
///   into the artifacts;
/// - a plain `bool`: a choice for the installer's own code.
///
/// Releases and artifacts are on unless `--no-<field>`, choices off unless `--<field>`. The struct
/// gets the flags every install has (`installation_groans::Flags`: `--accept`, `--mode`, `--dir`,
/// `--artifacts`, `--reinstall`, `--run`, the shortcuts, `--computer-speak`), read through it as
/// `cli.run`, a `Cli::new()` that reads the command line, and `<Struct>Releases`, a field per
/// release, which a location's `releases` is.
#[proc_macro_attribute]
pub fn cli(_args: TokenStream, item: TokenStream) -> TokenStream {
    let mut item = parse_macro_input!(item as ItemStruct);
    let Fields::Named(fields) = &mut item.fields else {
        return syn::Error::new_spanned(&item, "an installer's command line is a struct with named fields")
            .to_compile_error()
            .into();
    };
    let mut parts = Vec::new();
    for field in fields.named.iter_mut() {
        let mut kind = Kind::Choice;
        for attr in &field.attrs {
            if ours(attr, "release") {
                let args = match attr.parse_args_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
                    Ok(args) => args,
                    Err(error) => return error.to_compile_error().into(),
                };
                let mut args = args.into_iter();
                let installation = args.next();
                let name = match args.next() {
                    Some(Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(name),
                        ..
                    })) => name,
                    None => LitStr::new(
                        &field.ident.as_ref().unwrap().to_string(),
                        Span::call_site(),
                    ),
                    Some(other) => {
                        return syn::Error::new_spanned(other, "a release's name is a string")
                            .to_compile_error()
                            .into();
                    }
                };
                match installation {
                    Some(installation) => kind = Kind::Release(installation, name),
                    None => {
                        return syn::Error::new_spanned(attr, "Release(INSTALLATION, \"name\")")
                            .to_compile_error()
                            .into();
                    }
                }
            } else if ours(attr, "artifact") {
                match attr.parse_args::<Expr>() {
                    Ok(installation) => kind = Kind::Artifact(installation),
                    Err(error) => return error.to_compile_error().into(),
                }
            }
        }
        field
            .attrs
            .retain(|attr| !ours(attr, "release") && !ours(attr, "artifact"));
        let always = match &field.ty {
            Type::Tuple(tuple) if tuple.elems.is_empty() => true,
            Type::Path(path) if path.path.is_ident("bool") => false,
            other => {
                return syn::Error::new_spanned(other, "an installer's parts are bool, or () for one always installed")
                    .to_compile_error()
                    .into();
            }
        };
        if always && matches!(kind, Kind::Choice) {
            return syn::Error::new_spanned(&field.ty, "a choice is a bool")
                .to_compile_error()
                .into();
        }
        if always {
            // (a release always installed is only there to be described)
            field.attrs.push(syn::parse_quote!(#[allow(dead_code)]));
        }
        parts.push((field.ident.clone().unwrap(), doc(&field.attrs), kind, always));
    }
    // 🚩 the flags every install has, read through the struct (cli.run)
    fields.named.push(syn::parse_quote! {
        #[doc(hidden)]
        pub flags: ::installation_groans::Flags
    });

    let name = &item.ident;
    let vis = &item.vis;
    let releases = format_ident!("{name}Releases");
    let about = doc(&item.attrs);
    let program = parts
        .iter()
        .find_map(|(_, _, kind, _)| match kind {
            Kind::Release(_, name) => Some(name.value()),
            _ => None,
        })
        .unwrap_or_default();
    let mut defaults = Vec::new();
    let mut described = Vec::new();
    let mut setters = Vec::new();
    let mut release_fields = Vec::new();
    let mut release_values = Vec::new();
    for (field, doc, kind, always) in &parts {
        let flag = field.to_string().replace('_', "-");
        let on = if *always {
            quote!(::core::option::Option::None)
        } else {
            quote!(::core::option::Option::Some(self.#field))
        };
        defaults.push(match (kind, always) {
            (_, true) => quote!(#field: ()),
            (Kind::Choice, false) => quote!(#field: false),
            (_, false) => quote!(#field: true),
        });
        if !always {
            setters.push(quote!(#flag => self.#field = on,));
        }
        let kind = match kind {
            Kind::Release(installation, release) => {
                release_fields.push(quote!(pub #field: ::installation_groans::Release));
                release_values.push(quote! {
                    #field: ::installation_groans::Release::new(#release, &#installation, places, #on)
                });
                quote!(::installation_groans::Kind::Release(&#installation, #release))
            }
            Kind::Artifact(installation) => {
                quote!(::installation_groans::Kind::Artifact(&#installation))
            }
            Kind::Choice => quote!(::installation_groans::Kind::Choice),
        };
        described.push(quote! {
            ::installation_groans::Part { flag: #flag, doc: #doc, kind: #kind, on: #on }
        });
    }
    quote! {
        #item

        /// The releases the install puts in place, a field each: a location's `releases`.
        #vis struct #releases {
            #(#release_fields,)*
        }

        impl ::installation_groans::Installer for #name {
            type Releases = #releases;
            const NAME: &'static str = #program;
            const ABOUT: &'static str = #about;

            fn blank() -> Self {
                Self {
                    #(#defaults,)*
                    flags: ::core::default::Default::default(),
                }
            }

            fn parts(&self) -> ::std::vec::Vec<::installation_groans::Part> {
                ::std::vec![#(#described),*]
            }

            fn set(&mut self, flag: &str, on: bool) -> bool {
                match flag {
                    #(#setters)*
                    _ => return false,
                }
                true
            }

            fn releases(&self, places: &::installation_groans::Places) -> #releases {
                #releases { #(#release_values,)* }
            }
        }

        impl #name {
            /// The command line, read: exits with its help for `--help`, or with what is wrong.
            pub fn new() -> Self {
                ::installation_groans::parse()
            }
        }

        impl ::core::ops::Deref for #name {
            type Target = ::installation_groans::Flags;
            fn deref(&self) -> &Self::Target {
                &self.flags
            }
        }

        impl ::core::ops::DerefMut for #name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.flags
            }
        }
    }
    .into()
}

/// Names where the installer whose command line is the struct given (`#[location(Cli)]`) installs:
/// `installation_groans::Location<Cli>`, its mode, folders and releases.
#[proc_macro_attribute]
pub fn location(args: TokenStream, item: TokenStream) -> TokenStream {
    let cli = parse_macro_input!(args as Type);
    let item = parse_macro_input!(item as ItemStruct);
    let name = &item.ident;
    let vis = &item.vis;
    let attrs = &item.attrs;
    quote! {
        #(#attrs)*
        #vis type #name = ::installation_groans::Location<#cli>;
    }
    .into()
}
