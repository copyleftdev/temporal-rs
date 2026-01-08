//! Testcontainers support for Temporal server.

use testcontainers::{clients::Cli, core::WaitFor, Container, GenericImage};
use url::Url;

/// Default Temporal server image.
pub const DEFAULT_IMAGE: &str = "temporalio/auto-setup";
/// Default Temporal server version.
pub const DEFAULT_TAG: &str = "1.24.2";
/// Default gRPC port.
pub const GRPC_PORT: u16 = 7233;
/// Default HTTP port (for UI/API).
pub const HTTP_PORT: u16 = 8233;

/// A Temporal server container for integration testing.
///
/// # Example
///
/// ```ignore
/// use temporal_testing::TemporalContainer;
///
/// let docker = testcontainers::clients::Cli::default();
/// let container = TemporalContainer::start(&docker).unwrap();
/// let endpoint = container.endpoint();
///
/// // Connect your client to `endpoint`
/// ```
pub struct TemporalContainer<'d> {
    #[allow(dead_code)]
    container: Container<'d, GenericImage>,
    grpc_port: u16,
}

impl<'d> TemporalContainer<'d> {
    /// Start a new Temporal server container with default settings.
    ///
    /// # Errors
    ///
    /// Returns an error if the container fails to start.
    pub fn start(docker: &'d Cli) -> Result<Self, ContainerError> {
        Self::start_with_options(docker, ContainerOptions::default())
    }

    /// Start a new Temporal server container with custom options.
    ///
    /// # Errors
    ///
    /// Returns an error if the container fails to start.
    pub fn start_with_options(
        docker: &'d Cli,
        options: ContainerOptions,
    ) -> Result<Self, ContainerError> {
        let image = GenericImage::new(options.image, options.tag)
            .with_exposed_port(GRPC_PORT)
            .with_wait_for(WaitFor::message_on_stdout("Temporal server is running"));

        let container = docker.run(image);
        let grpc_port = container.get_host_port_ipv4(GRPC_PORT).unwrap();

        Ok(Self {
            container,
            grpc_port,
        })
    }

    /// Get the gRPC endpoint URL for connecting to the Temporal server.
    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("http://localhost:{}", self.grpc_port)
    }

    /// Get the gRPC endpoint as a URL.
    ///
    /// # Errors
    ///
    /// Returns an error if URL parsing fails (should not happen).
    pub fn endpoint_url(&self) -> Result<Url, url::ParseError> {
        Url::parse(&self.endpoint())
    }

    /// Get the mapped gRPC port.
    #[must_use]
    pub const fn grpc_port(&self) -> u16 {
        self.grpc_port
    }
}

/// Options for configuring a Temporal container.
#[derive(Debug, Clone)]
pub struct ContainerOptions {
    /// Docker image name.
    pub image: &'static str,
    /// Docker image tag.
    pub tag: &'static str,
    /// Startup timeout in seconds.
    pub startup_timeout_secs: u64,
}

impl Default for ContainerOptions {
    fn default() -> Self {
        Self {
            image: DEFAULT_IMAGE,
            tag: DEFAULT_TAG,
            startup_timeout_secs: 120,
        }
    }
}

/// Errors that can occur when working with containers.
#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    /// Container failed to start.
    #[error("container failed to start: {0}")]
    StartFailed(String),
    /// Failed to map container port.
    #[error("port mapping failed: {0}")]
    PortMapping(String),
}
