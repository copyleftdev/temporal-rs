//! Testing utilities for the Temporal SDK.
//!
//! This crate provides helpers for testing Temporal workflows and activities,
//! including testcontainers integration for spinning up real Temporal servers.
//!
//! # Features
//!
//! - `testcontainers` - Enable testcontainers support for integration tests

#![warn(missing_docs)]
#![deny(unsafe_code)]

#[cfg(feature = "testcontainers")]
pub mod containers;

pub mod fixtures;

#[cfg(feature = "testcontainers")]
pub use containers::{ContainerError, ContainerOptions, TemporalContainer};
