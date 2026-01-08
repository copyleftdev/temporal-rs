//! Low-level bindings to Temporal SDK Core.
//!
//! This crate provides a stable abstraction over `temporalio-sdk-core`,
//! isolating the rest of the SDK from upstream API changes.
//!
//! Most users should use the high-level `temporal` crate instead.

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod client;
pub mod error;
pub mod runtime;
pub mod worker;

pub use client::CoreClient;
pub use error::{CoreError, Result};
pub use runtime::CoreRuntime;
pub use worker::CoreWorker;

pub use temporalio_common::protos;
pub use url::Url;
