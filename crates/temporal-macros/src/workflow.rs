//! Implementation of the `#[workflow]` macro.

use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use syn::{ItemFn, Result, FnArg, Pat, Type};

/// Expand the workflow macro.
pub fn expand_workflow(_attr: TokenStream, input: ItemFn) -> Result<TokenStream> {
    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_block = &input.block;
    let fn_attrs = &input.attrs;

    // Generate registration function name
    let register_fn = format_ident!("{}_registration", fn_name);

    // Extract parameters (skip first which should be WorkflowContext)
    let params: Vec<_> = input.sig.inputs.iter().skip(1).collect();

    // Get input type if there is one
    let (input_type, has_input) = if params.is_empty() {
        (quote!(()), false)
    } else if let Some(FnArg::Typed(pat_type)) = params.first() {
        let ty = &pat_type.ty;
        (quote!(#ty), true)
    } else {
        (quote!(()), false)
    };

    // Get return type
    let return_type = match &input.sig.output {
        syn::ReturnType::Default => quote!(()),
        syn::ReturnType::Type(_, ty) => quote!(#ty),
    };

    // Generate the workflow handler wrapper
    let handler_body = if has_input {
        quote! {
            let input: #input_type = ::serde_json::from_value(input_value)
                .map_err(|e| ::temporal::workflow::WorkflowError::Serialization(e.to_string()))?;
            let result = #fn_name(ctx, input).await?;
            ::serde_json::to_value(result)
                .map_err(|e| ::temporal::workflow::WorkflowError::Serialization(e.to_string()))
        }
    } else {
        quote! {
            let result = #fn_name(ctx).await?;
            ::serde_json::to_value(result)
                .map_err(|e| ::temporal::workflow::WorkflowError::Serialization(e.to_string()))
        }
    };

    let expanded = quote! {
        #(#fn_attrs)*
        #fn_vis async fn #fn_name #input -> #return_type
        #fn_block

        /// Get the workflow registration for this workflow.
        #fn_vis fn #register_fn() -> ::temporal::workflow::WorkflowRegistration {
            ::temporal::workflow::WorkflowRegistration {
                name: stringify!(#fn_name).to_string(),
                handler: ::std::sync::Arc::new(|ctx, input_value| {
                    Box::pin(async move {
                        #handler_body
                    })
                }),
            }
        }
    };

    Ok(expanded)
}
