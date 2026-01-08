//! Temporal client for connecting to the server.
//!
//! The client is used to start workflows, send signals, and query workflow state.
//!
//! # Example
//!
//! ```ignore
//! use temporal::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Client::connect("localhost:7233", "default").await?;
//!
//!     // Start a workflow
//!     let handle = client
//!         .start_workflow("my-workflow", "my-workflow-id", "task-queue", json!({"key": "value"}))
//!         .await?;
//!
//!     // Wait for result
//!     let result = handle.result().await?;
//!
//!     Ok(())
//! }
//! ```

use crate::error::{ClientError, Error};
use std::sync::Arc;
use temporal_core::client::ClientConfig;
use temporal_core::{CoreClient, CoreRuntime, Url};

/// A client for connecting to a Temporal server.
///
/// The client provides methods for starting workflows, sending signals,
/// and querying workflow state.
#[derive(Clone)]
pub struct Client {
    inner: Arc<CoreClient>,
    runtime: Arc<CoreRuntime>,
}

impl Client {
    /// Connect to a Temporal server.
    ///
    /// # Arguments
    ///
    /// * `target` - The server address (e.g., "localhost:7233" or "http://localhost:7233")
    /// * `namespace` - The namespace to connect to
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let client = Client::connect("localhost:7233", "default").await?;
    /// ```
    pub async fn connect(
        target: impl AsRef<str>,
        namespace: impl Into<String>,
    ) -> Result<Self, Error> {
        let target_str = target.as_ref();
        let url = if target_str.starts_with("http://") || target_str.starts_with("https://") {
            Url::parse(target_str).map_err(|e| {
                Error::Client(ClientError::InvalidConfig(format!("invalid URL: {}", e)))
            })?
        } else {
            Url::parse(&format!("http://{}", target_str)).map_err(|e| {
                Error::Client(ClientError::InvalidConfig(format!("invalid URL: {}", e)))
            })?
        };

        let runtime = CoreRuntime::new()?;
        let config = ClientConfig::new(url, namespace);
        let inner = CoreClient::connect(config, &runtime).await?;

        Ok(Self {
            inner: Arc::new(inner),
            runtime: Arc::new(runtime),
        })
    }

    /// Get the namespace this client is connected to.
    #[must_use]
    pub fn namespace(&self) -> &str {
        self.inner.namespace()
    }

    /// Get the target URL.
    #[must_use]
    pub fn target_url(&self) -> &Url {
        self.inner.target_url()
    }

    /// Get a reference to the inner core client.
    #[must_use]
    pub fn inner(&self) -> &CoreClient {
        &self.inner
    }

    /// Get a reference to the runtime.
    #[must_use]
    pub fn runtime(&self) -> &CoreRuntime {
        &self.runtime
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("namespace", &self.namespace())
            .field("target_url", &self.target_url().to_string())
            .finish()
    }
}
