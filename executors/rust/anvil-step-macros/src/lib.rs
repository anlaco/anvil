// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! The `#[step]` attribute. See the `anvil-step` crate for what it is for.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, FnArg, Ident, ItemFn, LitStr, Pat, Token, Type};

/// One `name: Type` of the `outputs(...)` list.
struct Output {
    name: Ident,
    ty: Type,
}

impl Parse for Output {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty = input.parse()?;
        Ok(Output { name, ty })
    }
}

/// What the attribute takes: what the signature cannot say.
#[derive(Default)]
struct Args {
    name: Option<LitStr>,
    outputs: Vec<Output>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = Args::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            if key == "name" {
                input.parse::<Token![=]>()?;
                args.name = Some(input.parse()?);
            } else if key == "outputs" {
                let inner;
                syn::parenthesized!(inner in input);
                args.outputs = Punctuated::<Output, Token![,]>::parse_terminated(&inner)?
                    .into_iter()
                    .collect();
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "expected `name = \"...\"` or `outputs(name: Type, ...)`",
                ));
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(args)
    }
}

/// Registers a function as a step Anvil can call and describe.
///
/// The whole authoring surface, together with `Outcome` and `export!`. See the
/// `anvil-step` crate docs.
#[proc_macro_attribute]
pub fn step(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as Args);
    let func = parse_macro_input!(item as ItemFn);
    match expand(args, func) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand(args: Args, func: ItemFn) -> syn::Result<TokenStream> {
    let fname = &func.sig.ident;
    let step_name = args
        .name
        .map(|l| l.value())
        .unwrap_or_else(|| fname.to_string());
    let doc = first_doc_line(&func);

    let mut wants_ctx = None;
    let mut extractions = Vec::new();
    let mut call_args = Vec::new();
    let mut specs = Vec::new();

    for (i, arg) in func.sig.inputs.iter().enumerate() {
        let FnArg::Typed(pt) = arg else {
            return Err(syn::Error::new_spanned(
                arg,
                "a step is a free function: `self` cannot be a step parameter",
            ));
        };
        let Pat::Ident(ident) = &*pt.pat else {
            return Err(syn::Error::new_spanned(
                &pt.pat,
                "a step parameter needs a plain name: the sequence sends it by name",
            ));
        };
        let pname = &ident.ident;
        let ty = &*pt.ty;

        // `ctx` is the executor talking to the step, not a value out of the
        // sequence — so it is injected and never described. Recognised by name,
        // like Python does, and checked by type so a slip is caught here.
        if pname == "ctx" {
            if i != 0 {
                return Err(syn::Error::new_spanned(
                    pt,
                    "`ctx` goes first: it is the executor talking to the step, not a parameter",
                ));
            }
            if !is_ctx(ty) {
                return Err(syn::Error::new_spanned(
                    ty,
                    "a parameter called `ctx` is the invocation context and must be `Ctx` or `&Ctx`; \
                     rename it if you meant a parameter of the sequence",
                ));
            }
            wants_ctx = Some(matches!(ty, Type::Reference(_)));
            continue;
        }

        let lit = pname.to_string();
        let local = format_ident!("__anvil_arg_{}", pname);
        extractions.push(quote! {
            let #local = match <#ty as ::anvil_step::Input>::extract(#lit, inputs) {
                ::core::result::Result::Ok(v) => v,
                ::core::result::Result::Err(e) => return ::anvil_step::Outcome::error(e),
            };
        });
        call_args.push(quote! { #local });
        let pdoc = "";
        specs.push(quote! {
            ::anvil_step::ParameterSpec {
                name: #lit,
                r#type: <#ty as ::anvil_step::Input>::TYPE,
                required: <#ty as ::anvil_step::Input>::REQUIRED,
                doc: #pdoc,
            }
        });
    }

    let ctx_arg = match wants_ctx {
        Some(true) => vec![quote! { ctx }],
        Some(false) => vec![quote! { ctx.clone() }],
        None => vec![],
    };

    let outputs = args.outputs.iter().map(|o| {
        let lit = o.name.to_string();
        let ty = &o.ty;
        quote! {
            ::anvil_step::OutputSpec {
                name: #lit,
                r#type: <#ty as ::anvil_step::Scalar>::TYPE,
                doc: "",
            }
        }
    });

    // A private module per step: `inventory::submit!` needs items at module
    // level, and the name keeps two steps in one file from colliding.
    let holder = format_ident!("__anvil_step_{}", fname);

    Ok(quote! {
        #func

        #[doc(hidden)]
        #[allow(non_snake_case)]
        mod #holder {
            use super::*;

            const INPUTS: &[::anvil_step::ParameterSpec] = &[#(#specs),*];
            const OUTPUTS: &[::anvil_step::OutputSpec] = &[#(#outputs),*];

            fn call(
                ctx: &::anvil_step::Ctx,
                inputs: &[::anvil_step::Named],
            ) -> ::anvil_step::Outcome {
                #(#extractions)*
                ::anvil_step::outcome_of(super::#fname(#(#ctx_arg,)* #(#call_args),*))
            }

            ::anvil_step::inventory::submit! {
                ::anvil_step::Step {
                    spec: ::anvil_step::StepSpec {
                        name: #step_name,
                        inputs: INPUTS,
                        outputs: OUTPUTS,
                        doc: #doc,
                    },
                    call,
                }
            }
        }
    }
    .into())
}

/// Whether the type is `Ctx` or `&Ctx`, however it was spelled.
fn is_ctx(ty: &Type) -> bool {
    match ty {
        Type::Reference(r) => is_ctx(&r.elem),
        Type::Path(p) => p.path.segments.last().is_some_and(|s| s.ident == "Ctx"),
        _ => false,
    }
}

/// The first line of the doc comment becomes the step's `doc` in the catalog —
/// the same rule as the Python SDK, and the reason a step's documentation is
/// worth writing: it is what a sequence author reads.
fn first_doc_line(func: &ItemFn) -> String {
    for attr in &func.attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            {
                let line = s.value().trim().to_string();
                if !line.is_empty() {
                    return line;
                }
            }
        }
    }
    String::new()
}
