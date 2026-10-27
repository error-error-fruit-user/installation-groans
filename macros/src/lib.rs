//! installation-groans' macros, re-exported by it: [`macro@cli`] makes an installer's command line
//! out of a struct, [`macro@location`] names where it installs. Use them through
//! `installation_groans::`, which is the crate their code is written against.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    Attribute, Expr, Fields, ItemStruct, LitStr, Path, Token, Type, parse::Parser,
    parse_macro_input, punctuated::Punctuated,
};

/// What a field of the installer's struct is.
enum Kind {
    /// `#[installation_groans::Release(INSTALLATION, "name")]`: a program the install puts in
    /// place, its payload.
    Release(Expr, LitStr),
    /// `#[installation_groans::Artifact(INSTALLATION)]`: something it downloads into the artifacts.
    Artifact(Expr),
    /// A plain `bool` (on with `#[installation_groans::On]`): a choice for the installer's own code.
    Choice { on: bool },
    /// An `Option<PathBuf>`: a path the installer's own code is given (`--waw DIR`).
    Path,
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

/// Whether `attr` is ours, `name` (any case, with or without `installation_groans::`).
fn ours(attr: &Attribute, name: &str) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|last| last.ident.to_string().eq_ignore_ascii_case(name))
}

fn error(spanned: impl quote::ToTokens, message: &str) -> TokenStream {
    syn::Error::new_spanned(spanned, message)
        .to_compile_error()
        .into()
}

/// The installer's command line, out of a struct of its parts, each with a doc comment (its help,
/// its line in the plan and its place in the question):
///
/// - `#[installation_groans::Release(INSTALLATION, "name")]` on a `()` (always installed) or a
///   `bool`: a program put in place as `name`, the payload;
/// - `#[installation_groans::Artifact(INSTALLATION)]` on a `bool` (or `()`): something downloaded
///   into the artifacts;
/// - a plain `bool`: a choice for the installer's own code, off unless `--<field>`, or on unless
///   `--no-<field>` with `#[installation_groans::On]`;
/// - an `Option<PathBuf>`: a path for the installer's own code (`--<field> PATH`).
///
/// A field's flag is its name with `-` for `_`, or `#[installation_groans::Flag("name")]`'s (a
/// path and an `On` choice can share one: `--blender PATH` and `--no-blender`).
///
/// Releases and artifacts are on unless `--no-<field>`. Its options, all optional:
/// `#[installation_groans::cli(title = "Nacht des Schreckens", folder = "nacht-des-schreckens",
/// plan = my_plan)]`: what the install says it installs (else the first release's name), its folder
/// for `--user-wide` (else the first release's name, or the title), and a
/// `fn(&Cli, &Places) -> Vec<String>` giving the plan's lines for its parts (else their docs).
///
/// The struct gets the flags every install has (`installation_groans::Flags`: `--accept`,
/// `--locally`/`--user-wide`, `--dir`, `--artifacts`, `--from`, `--reinstall`, the shortcuts,
/// `--computer-speak`), read through it (`cli.reinstall`), a `Cli::new()` that reads the command
/// line, and `<Struct>Releases` and `<Struct>Artifacts`, a field per release and artifact, which a
/// location's `releases` and `artifacts` are.
#[proc_macro_attribute]
pub fn cli(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut item = parse_macro_input!(item as ItemStruct);
    let options = match Punctuated::<syn::MetaNameValue, Token![,]>::parse_terminated.parse(args) {
        Ok(options) => options,
        Err(problem) => return problem.to_compile_error().into(),
    };
    let (mut title, mut folder, mut plan) = (None::<String>, None::<String>, None::<Path>);
    for option in options {
        let name = option.path.get_ident().map(ToString::to_string).unwrap_or_default();
        match (name.as_str(), &option.value) {
            ("title" | "folder", Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(text), .. })) => {
                if name == "title" {
                    title = Some(text.value());
                } else {
                    folder = Some(text.value());
                }
            }
            ("plan", Expr::Path(path)) => plan = Some(path.path.clone()),
            _ => return error(&option, "cli's options are title = \"...\", folder = \"...\" and plan = function"),
        }
    }
    let Fields::Named(fields) = &mut item.fields else {
        return error(&item, "an installer's command line is a struct with named fields");
    };
    let mut parts = Vec::new();
    for field in fields.named.iter_mut() {
        let mut kind = None;
        let mut on = false;
        let mut flag = field.ident.as_ref().unwrap().to_string().replace('_', "-");
        for attr in &field.attrs {
            if ours(attr, "release") {
                let args = match attr.parse_args_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
                    Ok(args) => args,
                    Err(problem) => return problem.to_compile_error().into(),
                };
                let mut args = args.into_iter();
                let Some(installation) = args.next() else {
                    return error(attr, "Release(INSTALLATION, \"name\")");
                };
                let name = match args.next() {
                    Some(Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(name), .. })) => name,
                    None => LitStr::new(&field.ident.as_ref().unwrap().to_string(), Span::call_site()),
                    Some(other) => return error(other, "a release's name is a string"),
                };
                kind = Some(Kind::Release(installation, name));
            } else if ours(attr, "artifact") {
                match attr.parse_args::<Expr>() {
                    Ok(installation) => kind = Some(Kind::Artifact(installation)),
                    Err(problem) => return problem.to_compile_error().into(),
                }
            } else if ours(attr, "on") {
                on = true;
            } else if ours(attr, "flag") {
                match attr.parse_args::<LitStr>() {
                    Ok(name) => flag = name.value(),
                    Err(problem) => return problem.to_compile_error().into(),
                }
            }
        }
        field
            .attrs
            .retain(|attr| !["release", "artifact", "on", "flag"].iter().any(|name| ours(attr, name)));
        let (always, path) = match &field.ty {
            Type::Tuple(tuple) if tuple.elems.is_empty() => (true, false),
            Type::Path(ty) if ty.path.is_ident("bool") => (false, false),
            Type::Path(ty) if ty.path.segments.last().is_some_and(|last| last.ident == "Option") => (false, true),
            other => return error(other, "an installer's parts are bool, () for one always installed, or Option<PathBuf>"),
        };
        let kind = match (kind, path) {
            (None, true) => Kind::Path,
            (None, false) if always => return error(&field.ty, "a choice is a bool"),
            (None, false) => Kind::Choice { on },
            (Some(_), true) => return error(&field.ty, "a release or an artifact is a bool, or ()"),
            (Some(kind), false) => kind,
        };
        if always {
            // (a part always installed is only there to be described)
            field.attrs.push(syn::parse_quote!(#[allow(dead_code)]));
        }
        parts.push((field.ident.clone().unwrap(), flag, doc(&field.attrs), kind, always));
    }
    // 🚩 the flags every install has, read through the struct (cli.reinstall)
    fields.named.push(syn::parse_quote! {
        #[doc(hidden)]
        pub flags: ::installation_groans::Flags
    });

    let name = &item.ident;
    let vis = &item.vis;
    let releases = format_ident!("{name}Releases");
    let artifacts = format_ident!("{name}Artifacts");
    let about = doc(&item.attrs);
    let first_release = parts.iter().find_map(|(_, _, _, kind, _)| match kind {
        Kind::Release(_, name) => Some(name.value()),
        _ => None,
    });
    let title = title.or(first_release.clone()).unwrap_or_else(|| "it".into());
    let folder = folder
        .or(first_release)
        .unwrap_or_else(|| title.to_lowercase().replace(' ', "-"));
    let plan = match plan {
        Some(plan) => quote!(#plan(self, places)),
        None => quote!(::installation_groans::plan_parts(self, places)),
    };
    let (mut defaults, mut described, mut setters, mut paths) = (vec![], vec![], vec![], vec![]);
    let (mut release_fields, mut release_values) = (vec![], vec![]);
    let (mut artifact_fields, mut artifact_values) = (vec![], vec![]);
    for (field, flag, doc, kind, always) in &parts {
        let on = if *always || matches!(kind, Kind::Path) {
            quote!(::core::option::Option::None)
        } else {
            quote!(::core::option::Option::Some(self.#field))
        };
        let on_by_default = !matches!(kind, Kind::Choice { on: false } | Kind::Path);
        defaults.push(match kind {
            _ if *always => quote!(#field: ()),
            Kind::Path => quote!(#field: ::core::option::Option::None),
            _ => quote!(#field: #on_by_default),
        });
        let path = match kind {
            Kind::Path => {
                paths.push(quote! {
                    #flag => self.#field = ::core::option::Option::Some(::core::convert::Into::into(path)),
                });
                quote!(self.#field.clone().map(::core::convert::Into::into))
            }
            _ => {
                if !always {
                    setters.push(quote!(#flag => self.#field = on,));
                }
                quote!(::core::option::Option::None)
            }
        };
        let kind = match kind {
            Kind::Release(installation, release) => {
                release_fields.push(quote!(pub #field: ::installation_groans::Release));
                release_values.push(quote! {
                    #field: ::installation_groans::Release::new(#release, &#installation, places, #on)
                });
                quote!(::installation_groans::Kind::Release(&#installation, #release))
            }
            Kind::Artifact(installation) => {
                artifact_fields.push(quote!(pub #field: ::installation_groans::Artifact));
                artifact_values.push(quote! {
                    #field: ::installation_groans::Artifact::new(#flag, &#installation, places, #on)
                });
                quote!(::installation_groans::Kind::Artifact(&#installation))
            }
            Kind::Choice { .. } => quote!(::installation_groans::Kind::Choice),
            Kind::Path => quote!(::installation_groans::Kind::Path),
        };
        described.push(quote! {
            ::installation_groans::Part {
                flag: #flag,
                doc: #doc,
                kind: #kind,
                on: #on,
                on_by_default: #on_by_default,
                path: #path,
            }
        });
    }

    quote! {
        #item

        /// The releases the install puts in place, a field each: a location's `releases`.
        #vis struct #releases {
            #(#release_fields,)*
        }

        /// The artifacts the install downloads, a field each, and their folder through `Deref`: a
        /// location's `artifacts`.
        #vis struct #artifacts {
            #(#artifact_fields,)*
            #[doc(hidden)]
            pub __folder: ::std::path::PathBuf,
        }

        impl ::core::ops::Deref for #artifacts {
            type Target = ::std::path::Path;
            fn deref(&self) -> &::std::path::Path {
                &self.__folder
            }
        }

        impl ::installation_groans::Installer for #name {
            type Releases = #releases;
            type Artifacts = #artifacts;
            const TITLE: &'static str = #title;
            const FOLDER: &'static str = #folder;
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

            #[allow(unreachable_code, unused_variables)]
            fn set(&mut self, flag: &str, on: bool) -> bool {
                match flag {
                    #(#setters)*
                    _ => return false,
                }
                true
            }

            #[allow(unreachable_code, unused_variables)]
            fn set_path(&mut self, flag: &str, path: ::std::path::PathBuf) -> bool {
                match flag {
                    #(#paths)*
                    _ => return false,
                }
                true
            }

            fn plan(&self, places: &::installation_groans::Places) -> ::std::vec::Vec<::std::string::String> {
                #plan
            }

            #[allow(unused_variables)]
            fn releases(&self, places: &::installation_groans::Places) -> #releases {
                #releases { #(#release_values,)* }
            }

            fn artifacts(&self, places: &::installation_groans::Places) -> #artifacts {
                #artifacts {
                    #(#artifact_values,)*
                    __folder: places.artifacts.clone(),
                }
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
/// `installation_groans::Location<Cli>`, its places, releases and artifacts.
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
