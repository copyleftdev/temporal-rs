//! Procedural macros for the Temporal SDK.
//!
//! This crate provides the `#[activity]`, `#[workflow]`, `#[signal]`, and `#[query]` macros
//! for defining Temporal workflows and activities with minimal boilerplate.
//!
//! # Example
//!
//! ```ignore
//! use temporal::prelude::*;
//!
//! #[activity]
//! async fn greet(ctx: ActivityContext, name: String) -> Result<String, ActivityError> {
//!     Ok(format!("Hello, {}!", name))
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

mod activity;
mod workflow;

/// Marks an async function as a Temporal activity.
///
/// The macro transforms the function to handle:
/// - Automatic serialization/deserialization of inputs and outputs
/// - Activity context injection
/// - Error handling and conversion
///
/// # Example
///
/// ```ignore
/// use temporal::prelude::*;
///
/// #[activity]
/// async fn process_order(ctx: ActivityContext, order: Order) -> Result<Receipt, ActivityError> {
///     // Heartbeat to indicate progress
///     ctx.heartbeat(json!({"status": "processing"}));
///
///     // Do the work
///     let receipt = process(order).await?;
///
///     Ok(receipt)
/// }
/// ```
///
/// # Requirements
///
/// - The function must be `async`
/// - Input parameters must implement `serde::Deserialize`
/// - Return type must be `Result<T, E>` where `T: serde::Serialize`
#[proc_macro_attribute]
pub fn activity(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    activity::expand_activity(attr.into(), input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as a Temporal workflow.
///
/// The macro transforms the function to handle:
/// - Automatic serialization/deserialization of inputs and outputs
/// - Workflow context injection
/// - Generation of a registration function
///
/// # Example
///
/// ```ignore
/// use temporal::prelude::*;
///
/// #[workflow]
/// async fn order_workflow(ctx: WorkflowContext, order: Order) -> WorkflowResult<Receipt> {
///     let validated = ctx.execute_activity("validate", &order).await?;
///     ctx.sleep(Duration::from_secs(60)).await;
///     let receipt = ctx.execute_activity("process", &validated).await?;
///     Ok(receipt)
/// }
///
/// // Register with: .workflow_registration(order_workflow_registration())
/// ```
#[proc_macro_attribute]
pub fn workflow(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    workflow::expand_workflow(attr.into(), input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks a function as a signal handler for a workflow.
///
/// **Note**: This macro is planned for Phase 2.
#[proc_macro_attribute]
pub fn signal(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Marks a function as a query handler for a workflow.
///
/// **Note**: This macro is planned for Phase 2.
#[proc_macro_attribute]
pub fn query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
