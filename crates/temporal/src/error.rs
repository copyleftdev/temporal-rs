//! Error types for the Temporal SDK.

use thiserror::Error;

/// The main error type for Temporal SDK operations.
#[derive(Error, Debug)]
pub enum Error {
    /// Client connection error.
    #[error("client error: {0}")]
    Client(#[from] ClientError),

    /// Worker error.
    #[error("worker error: {0}")]
    Worker(#[from] WorkerError),

    /// Activity error.
    #[error("activity error: {0}")]
    Activity(#[from] crate::activity::ActivityError),

    /// Core SDK error.
    #[error("core error: {0}")]
    Core(#[from] temporal_core::CoreError),
}

/// Errors that can occur with the client.
#[derive(Error, Debug)]
pub enum ClientError {
    /// Failed to connect to the server.
    #[error("connection failed: {0}")]
    Connection(String),

    /// Invalid configuration.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// Workflow not found.
    #[error("workflow not found: {0}")]
    WorkflowNotFound(String),

    /// Operation timed out.
    #[error("operation timed out")]
    Timeout,
}

/// Errors that can occur with the worker.
#[derive(Error, Debug)]
pub enum WorkerError {
    /// Worker initialization failed.
    #[error("initialization failed: {0}")]
    Init(String),

    /// Worker is already running.
    #[error("worker is already running")]
    AlreadyRunning,

    /// Worker shutdown failed.
    #[error("shutdown failed: {0}")]
    Shutdown(String),

    /// Activity registration failed.
    #[error("activity registration failed: {0}")]
    ActivityRegistration(String),

    /// Workflow registration failed.
    #[error("workflow registration failed: {0}")]
    WorkflowRegistration(String),
}

/// Result type alias for Temporal SDK operations.
pub type Result<T> = std::result::Result<T, Error>;
