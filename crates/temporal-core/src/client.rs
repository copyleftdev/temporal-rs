//! Core client for connecting to Temporal server.

use crate::error::{CoreError, Result};
use crate::runtime::CoreRuntime;
use std::sync::Arc;
use temporalio_client::{Client, ClientOptions, RetryClient};
use url::Url;

/// Options for configuring a client connection.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// The Temporal server URL (e.g., "http://localhost:7233").
    pub target_url: Url,
    /// The namespace to connect to.
    pub namespace: String,
    /// Client identity string.
    pub identity: String,
    /// Client name for metrics/logging.
    pub client_name: String,
    /// Client version for metrics/logging.
    pub client_version: String,
}

impl ClientConfig {
    /// Create a new client configuration.
    #[must_use]
    pub fn new(target_url: Url, namespace: impl Into<String>) -> Self {
        Self {
            target_url,
            namespace: namespace.into(),
            identity: format!("temporal-rs@{}", hostname()),
            client_name: "temporal-rs".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Set the client identity.
    #[must_use]
    pub fn with_identity(mut self, identity: impl Into<String>) -> Self {
        self.identity = identity.into();
        self
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self::new(
            Url::parse("http://localhost:7233").expect("valid default URL"),
            "default",
        )
    }
}

/// Wrapper around the SDK Core client with retry support.
#[derive(Clone)]
pub struct CoreClient {
    inner: Arc<RetryClient<Client>>,
    config: ClientConfig,
}

impl CoreClient {
    /// Connect to a Temporal server with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect(config: ClientConfig, runtime: &CoreRuntime) -> Result<Self> {
        let options = ClientOptions::builder()
            .target_url(config.target_url.clone())
            .client_name(config.client_name.clone())
            .client_version(config.client_version.clone())
            .identity(config.identity.clone())
            .build();

        let client = options
            .connect(
                config.namespace.clone(),
                runtime.inner().telemetry().get_temporal_metric_meter(),
            )
            .await
            .map_err(|e| CoreError::Connection(e.to_string()))?;

        Ok(Self {
            inner: Arc::new(client),
            config,
        })
    }

    /// Connect with default runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect_with_defaults(
        target_url: impl AsRef<str>,
        namespace: impl Into<String>,
    ) -> Result<Self> {
        let url = Url::parse(target_url.as_ref())?;
        let config = ClientConfig::new(url, namespace);
        let runtime = CoreRuntime::new()?;
        Self::connect(config, &runtime).await
    }

    /// Get the namespace this client is connected to.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.config.namespace
    }

    /// Get the target URL.
    #[must_use]
    pub fn target_url(&self) -> &Url {
        &self.config.target_url
    }

    /// Get the inner retry client.
    #[must_use]
    pub fn inner(&self) -> &RetryClient<Client> {
        &self.inner
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".to_string())
}
