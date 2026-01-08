//! Implementation of the #[activity] macro.

use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use syn::{ItemFn, Result, Error, FnArg, Pat, ReturnType};

/// Expand the #[activity] macro.
pub fn expand_activity(_attr: TokenStream, input: ItemFn) -> Result<TokenStream> {
    validate_activity_fn(&input)?;

    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_block = &input.block;
    let fn_attrs = &input.attrs;
    let fn_generics = &input.sig.generics;
    let fn_output = &input.sig.output;

    let activity_name = fn_name.to_string();
    let registration_fn = format_ident!("__temporal_activity_register_{}", fn_name);

    let params: Vec<_> = input
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(pat_type) = arg {
                Some(pat_type)
            } else {
                None
            }
        })
        .collect();

    let has_context = params.first().map_or(false, |p| {
        if let Pat::Ident(ident) = &*p.pat {
            ident.ident == "ctx" || ident.ident == "_ctx"
        } else {
            false
        }
    });

    let (ctx_param, other_params) = if has_context && !params.is_empty() {
        let ctx = &params[0];
        let others = &params[1..];
        (Some(ctx), others.to_vec())
    } else {
        (None, params)
    };

    let param_names: Vec<_> = other_params
        .iter()
        .filter_map(|p| {
            if let Pat::Ident(ident) = &*p.pat {
                Some(&ident.ident)
            } else {
                None
            }
        })
        .collect();

    let param_types: Vec<_> = other_params.iter().map(|p| &p.ty).collect();

    let ctx_binding = if ctx_param.is_some() {
        quote! { let ctx = __ctx; }
    } else {
        quote! { let _ = __ctx; }
    };

    let deserialize_params = if param_names.is_empty() {
        quote! {}
    } else {
        quote! {
            #(
                let #param_names: #param_types = ::temporal::activity::deserialize_input(&__input)?;
            )*
        }
    };

    let expanded = quote! {
        #(#fn_attrs)*
        #fn_vis async fn #fn_name #fn_generics(
            __ctx: ::temporal::activity::ActivityContext,
            __input: ::temporal::activity::ActivityInput,
        ) #fn_output {
            #ctx_binding
            #deserialize_params
            #fn_block
        }

        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub fn #registration_fn() -> ::temporal::activity::ActivityRegistration {
            ::temporal::activity::ActivityRegistration {
                name: #activity_name.to_string(),
                handler: ::std::sync::Arc::new(|ctx, input| {
                    Box::pin(async move {
                        let result = #fn_name(ctx, input).await;
                        ::temporal::activity::serialize_result(result)
                    })
                }),
            }
        }
    };

    Ok(expanded)
}

fn validate_activity_fn(input: &ItemFn) -> Result<()> {
    if input.sig.asyncness.is_none() {
        return Err(Error::new_spanned(
            &input.sig.fn_token,
            "activity functions must be async",
        ));
    }

    if let ReturnType::Default = input.sig.output {
        return Err(Error::new_spanned(
            &input.sig,
            "activity functions must return a Result type",
        ));
    }

    Ok(())
}
