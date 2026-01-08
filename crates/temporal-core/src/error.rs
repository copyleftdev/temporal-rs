//! Error types for the core SDK bindings.

use thiserror::Error;

/// Result type alias for core operations.
pub type Result<T> = std::result::Result<T, CoreError>;

/// Errors that can occur in the core SDK layer.
#[derive(Error, Debug)]
pub enum CoreError {
    /// Failed to connect to the Temporal server.
    #[error("connection failed: {0}")]
    Connection(String),

    /// Invalid configuration provided.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// Worker initialization failed.
    #[error("worker initialization failed: {0}")]
    WorkerInit(String),

    /// Runtime initialization failed.
    #[error("runtime initialization failed: {0}")]
    RuntimeInit(String),

    /// gRPC transport error.
    #[error("transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    /// gRPC status error.
    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),

    /// URL parsing error.
    #[error("invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    /// Internal error from sdk-core.
    #[error("internal error: {0}")]
    Internal(String),
}
