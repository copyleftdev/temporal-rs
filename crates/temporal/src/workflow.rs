//! Workflow context and utilities.
//!
//! Workflows are the core abstraction in Temporal. They orchestrate activities,
//! handle signals, and maintain durable state across failures.
//!
//! # Example
//!
//! ```ignore
//! use temporal::prelude::*;
//!
//! #[workflow]
//! async fn order_workflow(ctx: WorkflowContext, order: Order) -> WorkflowResult<Receipt> {
//!     // Execute an activity
//!     let validated = ctx.execute_activity::<ValidateOrder>(order.clone()).await?;
//!     
//!     // Sleep for a duration (deterministically)
//!     ctx.sleep(Duration::from_secs(60)).await;
//!     
//!     // Execute another activity
//!     let receipt = ctx.execute_activity::<ProcessPayment>(validated).await?;
//!     
//!     Ok(receipt)
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::sync::{mpsc, watch};
use parking_lot::RwLock;

/// The context passed to workflow functions.
///
/// Provides methods for executing activities, sleeping, getting the current time,
/// and other workflow operations. All operations are deterministic and replay-safe.
#[derive(Clone)]
pub struct WorkflowContext {
    info: WorkflowInfo,
    shared: Arc<RwLock<WorkflowSharedState>>,
    command_tx: mpsc::UnboundedSender<WorkflowCommand>,
    cancel_rx: watch::Receiver<bool>,
}

impl WorkflowContext {
    /// Create a new workflow context (internal use).
    pub(crate) fn new(
        info: WorkflowInfo,
        command_tx: mpsc::UnboundedSender<WorkflowCommand>,
        cancel_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            info,
            shared: Arc::new(RwLock::new(WorkflowSharedState::default())),
            command_tx,
            cancel_rx,
        }
    }

    /// Get information about this workflow execution.
    #[must_use]
    pub fn info(&self) -> &WorkflowInfo {
        &self.info
    }

    /// Get the workflow ID.
    #[must_use]
    pub fn workflow_id(&self) -> &str {
        &self.info.workflow_id
    }

    /// Get the run ID.
    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.info.run_id
    }

    /// Get the workflow type name.
    #[must_use]
    pub fn workflow_type(&self) -> &str {
        &self.info.workflow_type
    }

    /// Get the current workflow time.
    ///
    /// This returns the deterministic workflow time, not wall-clock time.
    /// Use this instead of `std::time::Instant::now()` in workflows.
    #[must_use]
    pub fn now(&self) -> SystemTime {
        self.shared.read().current_time.unwrap_or(SystemTime::UNIX_EPOCH)
    }

    /// Check if the workflow is currently replaying.
    ///
    /// During replay, side effects should not be re-executed.
    #[must_use]
    pub fn is_replaying(&self) -> bool {
        self.shared.read().is_replaying
    }

    /// Sleep for a duration.
    ///
    /// This creates a timer that is durable across workflow task boundaries.
    /// The timer will fire even if the worker restarts.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Sleep for 1 hour
    /// ctx.sleep(Duration::from_secs(3600)).await;
    /// ```
    pub async fn sleep(&self, duration: Duration) {
        let seq = self.next_timer_seq();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let _ = self.command_tx.send(WorkflowCommand::StartTimer {
            seq,
            duration,
            result_tx,
        });

        // Wait for timer to fire
        let _ = result_rx.await;
    }

    /// Execute an activity and wait for its result.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The activity result type (must implement Deserialize)
    ///
    /// # Arguments
    ///
    /// * `activity_type` - The name of the activity to execute
    /// * `input` - The input to pass to the activity
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result: String = ctx.execute_activity("greet", "Alice").await?;
    /// ```
    pub async fn execute_activity<T>(
        &self,
        activity_type: impl Into<String>,
        input: impl Serialize,
    ) -> Result<T, WorkflowError>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.execute_activity_with_options(activity_type, input, ActivityOptions::default())
            .await
    }

    /// Execute an activity with custom options.
    pub async fn execute_activity_with_options<T>(
        &self,
        activity_type: impl Into<String>,
        input: impl Serialize,
        options: ActivityOptions,
    ) -> Result<T, WorkflowError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let seq = self.next_activity_seq();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let input_payload = serde_json::to_value(&input)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        let _ = self.command_tx.send(WorkflowCommand::ScheduleActivity {
            seq,
            activity_type: activity_type.into(),
            input: input_payload,
            options,
            result_tx,
        });

        // Wait for activity result
        let result = result_rx
            .await
            .map_err(|_| WorkflowError::Internal("Activity result channel closed".into()))?;

        match result {
            ActivityResult::Completed(payload) => {
                serde_json::from_value(payload)
                    .map_err(|e| WorkflowError::Serialization(e.to_string()))
            }
            ActivityResult::Failed(msg) => Err(WorkflowError::ActivityFailed(msg)),
            ActivityResult::Cancelled => Err(WorkflowError::Cancelled),
            ActivityResult::TimedOut => Err(WorkflowError::ActivityTimedOut),
        }
    }

    /// Wait for the workflow to be cancelled.
    ///
    /// This returns when a cancellation request is received.
    pub async fn cancelled(&self) {
        let mut rx = self.cancel_rx.clone();
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    }

    /// Check if the workflow has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.cancel_rx.borrow()
    }

    /// Create a signal channel to receive signals of the given name.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut signals = ctx.signal_channel::<ApprovalSignal>("approval");
    /// while let Some(signal) = signals.recv().await {
    ///     // Handle signal
    /// }
    /// ```
    pub fn signal_channel<T>(&self, signal_name: impl Into<String>) -> SignalReceiver<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = self.command_tx.send(WorkflowCommand::SubscribeSignal {
            name: signal_name.into(),
            sender: tx,
        });
        SignalReceiver {
            rx,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the current attempt number (starts at 1).
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.info.attempt
    }

    /// Get the namespace.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.info.namespace
    }

    /// Get the task queue.
    #[must_use]
    pub fn task_queue(&self) -> &str {
        &self.info.task_queue
    }

    fn next_timer_seq(&self) -> u32 {
        let mut shared = self.shared.write();
        let seq = shared.next_timer_seq;
        shared.next_timer_seq += 1;
        seq
    }

    fn next_activity_seq(&self) -> u32 {
        let mut shared = self.shared.write();
        let seq = shared.next_activity_seq;
        shared.next_activity_seq += 1;
        seq
    }
}

impl std::fmt::Debug for WorkflowContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowContext")
            .field("workflow_id", &self.info.workflow_id)
            .field("workflow_type", &self.info.workflow_type)
            .field("run_id", &self.info.run_id)
            .finish()
    }
}

/// Information about a workflow execution.
#[derive(Debug, Clone)]
pub struct WorkflowInfo {
    /// The workflow ID.
    pub workflow_id: String,
    /// The run ID.
    pub run_id: String,
    /// The workflow type name.
    pub workflow_type: String,
    /// The namespace.
    pub namespace: String,
    /// The task queue.
    pub task_queue: String,
    /// Current attempt (starts at 1).
    pub attempt: u32,
    /// When the workflow execution started.
    pub start_time: Option<SystemTime>,
    /// Timeout for the entire workflow run.
    pub workflow_run_timeout: Option<Duration>,
    /// Timeout for a single workflow execution.
    pub workflow_execution_timeout: Option<Duration>,
}

impl Default for WorkflowInfo {
    fn default() -> Self {
        Self {
            workflow_id: String::new(),
            run_id: String::new(),
            workflow_type: String::new(),
            namespace: "default".into(),
            task_queue: String::new(),
            attempt: 1,
            start_time: None,
            workflow_run_timeout: None,
            workflow_execution_timeout: None,
        }
    }
}

/// Shared state for workflow context.
#[derive(Debug, Default)]
struct WorkflowSharedState {
    current_time: Option<SystemTime>,
    is_replaying: bool,
    next_timer_seq: u32,
    next_activity_seq: u32,
}

/// Options for executing an activity.
#[derive(Debug, Clone)]
pub struct ActivityOptions {
    /// Task queue to schedule the activity on (defaults to workflow's task queue).
    pub task_queue: Option<String>,
    /// Timeout from schedule to start.
    pub schedule_to_start_timeout: Option<Duration>,
    /// Timeout from start to close.
    pub start_to_close_timeout: Option<Duration>,
    /// Timeout from schedule to close.
    pub schedule_to_close_timeout: Option<Duration>,
    /// Heartbeat timeout.
    pub heartbeat_timeout: Option<Duration>,
    /// Retry policy.
    pub retry_policy: Option<RetryPolicy>,
}

impl Default for ActivityOptions {
    fn default() -> Self {
        Self {
            task_queue: None,
            schedule_to_start_timeout: None,
            start_to_close_timeout: Some(Duration::from_secs(60)),
            schedule_to_close_timeout: None,
            heartbeat_timeout: None,
            retry_policy: Some(RetryPolicy::default()),
        }
    }
}

/// Retry policy for activities.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Initial retry interval.
    pub initial_interval: Duration,
    /// Backoff coefficient.
    pub backoff_coefficient: f64,
    /// Maximum retry interval.
    pub maximum_interval: Option<Duration>,
    /// Maximum number of attempts.
    pub maximum_attempts: u32,
    /// Non-retryable error types.
    pub non_retryable_error_types: Vec<String>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial_interval: Duration::from_secs(1),
            backoff_coefficient: 2.0,
            maximum_interval: Some(Duration::from_secs(100)),
            maximum_attempts: 0, // unlimited
            non_retryable_error_types: vec![],
        }
    }
}

/// Internal workflow commands sent to the worker.
#[derive(Debug)]
pub(crate) enum WorkflowCommand {
    StartTimer {
        seq: u32,
        duration: Duration,
        result_tx: tokio::sync::oneshot::Sender<()>,
    },
    ScheduleActivity {
        seq: u32,
        activity_type: String,
        input: serde_json::Value,
        options: ActivityOptions,
        result_tx: tokio::sync::oneshot::Sender<ActivityResult>,
    },
    SubscribeSignal {
        name: String,
        sender: mpsc::UnboundedSender<serde_json::Value>,
    },
    Complete {
        result: Result<serde_json::Value, WorkflowError>,
    },
}

/// Result of an activity execution.
#[derive(Debug)]
pub(crate) enum ActivityResult {
    Completed(serde_json::Value),
    Failed(String),
    Cancelled,
    TimedOut,
}

/// Error type for workflow operations.
#[derive(Error, Debug, Clone)]
pub enum WorkflowError {
    /// Activity execution failed.
    #[error("activity failed: {0}")]
    ActivityFailed(String),

    /// Activity timed out.
    #[error("activity timed out")]
    ActivityTimedOut,

    /// Workflow was cancelled.
    #[error("workflow cancelled")]
    Cancelled,

    /// Child workflow failed.
    #[error("child workflow failed: {0}")]
    ChildWorkflowFailed(String),

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Internal SDK error.
    #[error("internal error: {0}")]
    Internal(String),

    /// Application-specific error.
    #[error("{message}")]
    Application {
        /// Error message.
        message: String,
        /// Error type for matching.
        error_type: String,
    },
}

impl WorkflowError {
    /// Create an application error.
    #[must_use]
    pub fn application(message: impl Into<String>, error_type: impl Into<String>) -> Self {
        Self::Application {
            message: message.into(),
            error_type: error_type.into(),
        }
    }
}

/// Result type for workflow functions.
pub type WorkflowResult<T> = Result<T, WorkflowError>;

/// A receiver for signals of a specific type.
pub struct SignalReceiver<T> {
    rx: mpsc::UnboundedReceiver<serde_json::Value>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SignalReceiver<T>
where
    T: for<'de> Deserialize<'de>,
{
    /// Receive the next signal, waiting if necessary.
    pub async fn recv(&mut self) -> Option<T> {
        let value = self.rx.recv().await?;
        serde_json::from_value(value).ok()
    }

    /// Try to receive a signal without waiting.
    pub fn try_recv(&mut self) -> Option<T> {
        let value = self.rx.try_recv().ok()?;
        serde_json::from_value(value).ok()
    }
}

/// Type alias for workflow handler functions.
pub type WorkflowHandler = Arc<
    dyn Fn(
            WorkflowContext,
            serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, WorkflowError>> + Send>>
        + Send
        + Sync,
>;

/// Registration information for a workflow.
pub struct WorkflowRegistration {
    /// Workflow type name.
    pub name: String,
    /// Handler function.
    pub handler: WorkflowHandler,
}

impl std::fmt::Debug for WorkflowRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowRegistration")
            .field("name", &self.name)
            .finish()
    }
}
