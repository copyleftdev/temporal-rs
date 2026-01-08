//! Activity context and utilities.
//!
//! Activities are the building blocks of Temporal workflows. They represent
//! units of work that can fail, be retried, and heartbeat progress.
//!
//! # Example
//!
//! ```ignore
//! use temporal::prelude::*;
//!
//! #[activity]
//! async fn process_order(ctx: ActivityContext, order: Order) -> Result<Receipt, ActivityError> {
//!     // Report progress
//!     ctx.heartbeat(json!({"status": "processing", "step": 1}));
//!
//!     // Check for cancellation
//!     if ctx.is_cancelled() {
//!         return Err(ActivityError::cancelled("activity was cancelled"));
//!     }
//!
//!     // Do the work
//!     let receipt = process(order).await?;
//!
//!     ctx.heartbeat(json!({"status": "complete", "step": 2}));
//!     Ok(receipt)
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// The context passed to activity functions.
///
/// Provides methods for heartbeating, checking cancellation, and
/// accessing activity metadata.
#[derive(Clone)]
pub struct ActivityContext {
    info: ActivityInfo,
    heartbeat_tx: Option<tokio::sync::mpsc::UnboundedSender<serde_json::Value>>,
    cancel_token: CancellationToken,
}

impl ActivityContext {
    /// Create a new activity context.
    #[must_use]
    pub fn new(info: ActivityInfo) -> Self {
        Self {
            info,
            heartbeat_tx: None,
            cancel_token: CancellationToken::new(),
        }
    }

    /// Create a new activity context with a cancellation token.
    #[must_use]
    pub fn new_with_cancel(info: ActivityInfo, cancel_token: CancellationToken) -> Self {
        Self {
            info,
            heartbeat_tx: None,
            cancel_token,
        }
    }

    /// Send a heartbeat with optional details.
    ///
    /// Heartbeats indicate that the activity is still making progress.
    /// For long-running activities, heartbeating prevents the activity
    /// from timing out.
    ///
    /// # Arguments
    ///
    /// * `details` - Optional JSON value with progress information
    pub fn heartbeat(&self, details: serde_json::Value) {
        if let Some(ref tx) = self.heartbeat_tx {
            let _ = tx.send(details);
        }
        tracing::debug!(
            activity_id = %self.info.activity_id,
            "activity heartbeat"
        );
    }

    /// Check if the activity has been cancelled.
    ///
    /// Activities should periodically check this and exit gracefully
    /// when cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Wait for the activity to be cancelled.
    ///
    /// This is useful for long-running activities that want to gracefully
    /// shut down when cancelled.
    pub async fn cancelled(&self) {
        self.cancel_token.cancelled().await;
    }

    /// Get information about this activity.
    #[must_use]
    pub fn info(&self) -> &ActivityInfo {
        &self.info
    }

    /// Get the activity ID.
    #[must_use]
    pub fn activity_id(&self) -> &str {
        &self.info.activity_id
    }

    /// Get the activity type name.
    #[must_use]
    pub fn activity_type(&self) -> &str {
        &self.info.activity_type
    }

    /// Get the workflow ID this activity is part of.
    #[must_use]
    pub fn workflow_id(&self) -> &str {
        &self.info.workflow_id
    }

    /// Get the attempt number (starts at 1).
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.info.attempt
    }
}

impl std::fmt::Debug for ActivityContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivityContext")
            .field("activity_id", &self.info.activity_id)
            .field("activity_type", &self.info.activity_type)
            .field("attempt", &self.info.attempt)
            .finish()
    }
}

/// Information about an activity execution.
#[derive(Debug, Clone)]
pub struct ActivityInfo {
    /// Unique activity ID.
    pub activity_id: String,
    /// Activity type name.
    pub activity_type: String,
    /// Workflow ID this activity belongs to.
    pub workflow_id: String,
    /// Run ID of the workflow.
    pub run_id: String,
    /// Task queue the activity was scheduled on.
    pub task_queue: String,
    /// Current attempt number (starts at 1).
    pub attempt: u32,
    /// Scheduled time.
    pub scheduled_time: Option<std::time::SystemTime>,
    /// Start time of this attempt.
    pub start_time: Option<std::time::SystemTime>,
}

impl Default for ActivityInfo {
    fn default() -> Self {
        Self {
            activity_id: String::new(),
            activity_type: String::new(),
            workflow_id: String::new(),
            run_id: String::new(),
            task_queue: String::new(),
            attempt: 1,
            scheduled_time: None,
            start_time: None,
        }
    }
}

/// Error type for activities.
#[derive(Error, Debug)]
pub enum ActivityError {
    /// The activity was cancelled.
    #[error("activity cancelled: {0}")]
    Cancelled(String),

    /// The activity failed with an error.
    #[error("activity failed: {0}")]
    Failed(String),

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// The activity panicked.
    #[error("activity panicked: {0}")]
    Panic(String),

    /// Application-specific error.
    #[error("{message}")]
    Application {
        /// Error message.
        message: String,
        /// Error type for matching.
        error_type: String,
        /// Whether this error is non-retryable.
        non_retryable: bool,
    },
}

impl ActivityError {
    /// Create a cancelled error.
    #[must_use]
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::Cancelled(message.into())
    }

    /// Create a failed error.
    #[must_use]
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }

    /// Create an application error.
    #[must_use]
    pub fn application(message: impl Into<String>, error_type: impl Into<String>) -> Self {
        Self::Application {
            message: message.into(),
            error_type: error_type.into(),
            non_retryable: false,
        }
    }

    /// Create a non-retryable application error.
    #[must_use]
    pub fn non_retryable(message: impl Into<String>, error_type: impl Into<String>) -> Self {
        Self::Application {
            message: message.into(),
            error_type: error_type.into(),
            non_retryable: true,
        }
    }
}

/// Result type for activity functions.
pub type ActivityResult<T> = Result<T, ActivityError>;

/// Input to an activity function (for macro use).
#[derive(Debug, Clone, Default)]
pub struct ActivityInput {
    /// Raw JSON payload.
    pub payload: serde_json::Value,
}

/// Deserialize activity input from JSON.
///
/// # Errors
///
/// Returns an error if deserialization fails.
pub fn deserialize_input<T: for<'de> Deserialize<'de>>(
    input: &ActivityInput,
) -> Result<T, ActivityError> {
    serde_json::from_value(input.payload.clone())
        .map_err(|e| ActivityError::Serialization(e.to_string()))
}

/// Serialize activity result to JSON.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn serialize_result<T: Serialize>(
    result: Result<T, ActivityError>,
) -> Result<serde_json::Value, ActivityError> {
    match result {
        Ok(value) => {
            serde_json::to_value(value).map_err(|e| ActivityError::Serialization(e.to_string()))
        }
        Err(e) => Err(e),
    }
}

/// Type alias for activity handler functions.
pub type ActivityHandler = Arc<
    dyn Fn(
            ActivityContext,
            ActivityInput,
        ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, ActivityError>> + Send>>
        + Send
        + Sync,
>;

/// Registration information for an activity.
pub struct ActivityRegistration {
    /// Activity type name.
    pub name: String,
    /// Handler function.
    pub handler: ActivityHandler,
}

impl std::fmt::Debug for ActivityRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivityRegistration")
            .field("name", &self.name)
            .finish()
    }
}
