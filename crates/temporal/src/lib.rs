//! High-performance Rust SDK for Temporal workflow orchestration.
//!
//! This crate provides an ergonomic API for building Temporal workflows and activities
//! in Rust, with compile-time safety and excellent performance.
//!
//! # Quick Start
//!
//! ```ignore
//! use temporal::prelude::*;
//!
//! #[activity]
//! async fn greet(ctx: ActivityContext, name: String) -> Result<String, ActivityError> {
//!     Ok(format!("Hello, {}!", name))
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Client::connect("localhost:7233", "default").await?;
//!
//!     Worker::builder()
//!         .client(client)
//!         .task_queue("greeting-queue")
//!         .activity(greet)
//!         .build()?
//!         .run()
//!         .await
//! }
//! ```
//!
//! # Modules
//!
//! - [`client`] - Client for connecting to Temporal server
//! - [`worker`] - Worker for executing workflows and activities
//! - [`activity`] - Activity context and utilities
//! - [`error`] - Error types
//!
//! # Feature Flags
//!
//! - `testcontainers` - Enable testcontainers support for integration tests

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod activity;
pub mod client;
pub mod error;
pub mod worker;

pub mod prelude {
    //! Convenient re-exports for common usage.
    //!
    //! ```ignore
    //! use temporal::prelude::*;
    //! ```

    pub use crate::activity::{ActivityContext, ActivityError, ActivityResult};
    pub use crate::client::Client;
    pub use crate::error::Error;
    pub use crate::worker::{Worker, WorkerBuilder};
    pub use temporal_macros::{activity, query, signal, workflow};

    pub use serde_json::json;
    pub use std::time::Duration;
}

pub use temporal_macros::{activity, query, signal, workflow};
