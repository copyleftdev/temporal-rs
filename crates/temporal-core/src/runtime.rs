//! Core runtime management.

use crate::error::{CoreError, Result};
use std::sync::Arc;
use temporalio_common::telemetry::TelemetryOptions;
use temporalio_sdk_core::{CoreRuntime as SdkCoreRuntime, RuntimeOptions};

/// Wrapper around the SDK Core runtime.
///
/// The runtime manages telemetry, metrics, and the underlying tokio runtime
/// integration for SDK Core operations.
#[derive(Clone)]
pub struct CoreRuntime {
    inner: Arc<SdkCoreRuntime>,
}

impl CoreRuntime {
    /// Create a new runtime with default options.
    ///
    /// # Errors
    ///
    /// Returns an error if runtime initialization fails.
    pub fn new() -> Result<Self> {
        Self::with_options(RuntimeOptions::default())
    }

    /// Create a new runtime with custom options.
    ///
    /// # Errors
    ///
    /// Returns an error if runtime initialization fails.
    pub fn with_options(options: RuntimeOptions) -> Result<Self> {
        let inner = SdkCoreRuntime::new_assume_tokio(options)
            .map_err(|e| CoreError::RuntimeInit(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Create a runtime with custom telemetry options.
    ///
    /// # Errors
    ///
    /// Returns an error if runtime initialization fails.
    pub fn with_telemetry(telemetry: TelemetryOptions) -> Result<Self> {
        let options = RuntimeOptions::builder()
            .telemetry_options(telemetry)
            .build()
            .map_err(|e| CoreError::RuntimeInit(e.to_string()))?;
        Self::with_options(options)
    }

    /// Get a reference to the inner SDK Core runtime.
    #[must_use]
    pub fn inner(&self) -> &SdkCoreRuntime {
        &self.inner
    }
}

impl Default for CoreRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create default CoreRuntime")
    }
}
