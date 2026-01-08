//! Worker for executing workflows and activities.
//!
//! Workers poll task queues for work and execute workflow and activity code.
//!
//! # Example
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

use crate::activity::{
    ActivityContext, ActivityHandler, ActivityInfo, ActivityInput, ActivityRegistration,
};
use crate::workflow::{WorkflowHandler, WorkflowRegistration};
use crate::client::Client;
use crate::error::{Error, WorkerError};
use futures::FutureExt;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use temporal_core::worker::WorkerOptions;
use temporal_core::CoreWorker;
use temporal_core::PollError;
use temporal_core::CoreWorkerTrait;
use temporal_core::protos::coresdk::activity_result::ActivityExecutionResult;
use temporal_core::protos::coresdk::activity_task::{activity_task, ActivityTask};
use temporal_core::protos::coresdk::ActivityTaskCompletion;
use temporal_core::protos::temporal::api::common::v1::Payload;
use temporal_core::protos::temporal::api::failure::v1::Failure;
use tokio_util::sync::CancellationToken;

/// A worker that polls for and executes tasks.
pub struct Worker {
    client: Client,
    task_queue: String,
    activities: Arc<RwLock<HashMap<String, ActivityHandler>>>,
    workflows: Arc<RwLock<HashMap<String, WorkflowHandler>>>,
    task_tokens_to_cancels: Arc<RwLock<HashMap<Vec<u8>, CancellationToken>>>,
    shutdown: CancellationToken,
}

impl Worker {
    /// Create a new worker builder.
    #[must_use]
    pub fn builder() -> WorkerBuilder {
        WorkerBuilder::new()
    }

    /// Get the task queue this worker is polling.
    #[must_use]
    pub fn task_queue(&self) -> &str {
        &self.task_queue
    }

    /// Get the client this worker is using.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Run the worker until shutdown is requested.
    ///
    /// # Errors
    ///
    /// Returns an error if the worker fails to start or encounters a fatal error.
    pub async fn run(&mut self) -> Result<(), Error> {
        let activity_count = self.activities.read().len();
        tracing::info!(
            task_queue = %self.task_queue,
            activity_count = activity_count,
            "starting worker"
        );

        let options = WorkerOptions::new(&self.task_queue);
        let core_worker = Arc::new(CoreWorker::new(
            self.client.inner(),
            self.client.runtime(),
            options,
        )?);

        if activity_count > 0 {
            self.run_activity_loop(core_worker.clone()).await?;
        } else {
            tracing::warn!("no activities registered, waiting for shutdown");
            self.shutdown.cancelled().await;
        }

        core_worker.shutdown().await;
        tracing::info!("worker stopped");
        Ok(())
    }

    async fn run_activity_loop(&self, core_worker: Arc<CoreWorker>) -> Result<(), Error> {
        loop {
            tokio::select! {
                biased;
                _ = self.shutdown.cancelled() => {
                    tracing::info!("worker shutdown requested");
                    break;
                }
                poll_result = core_worker.inner().poll_activity_task() => {
                    match poll_result {
                        Ok(task) => {
                            self.handle_activity_task(core_worker.clone(), task);
                        }
                        Err(PollError::ShutDown) => {
                            tracing::debug!("activity poller shutdown");
                            break;
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "activity poll error");
                            return Err(Error::Worker(WorkerError::Init(e.to_string())));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_activity_task(&self, core_worker: Arc<CoreWorker>, task: ActivityTask) {
        match task.variant {
            Some(activity_task::Variant::Start(start)) => {
                let activity_type = start.activity_type.clone();
                let activity_id = start.activity_id.clone();
                tracing::debug!(
                    activity_type = %activity_type,
                    activity_id = %activity_id,
                    "received activity"
                );

                let handler = self.activities.read().get(&activity_type).cloned();
                let Some(handler) = handler else {
                    tracing::error!(activity_type = %activity_type, "no handler registered");
                    self.complete_activity_with_error(
                        core_worker,
                        task.task_token,
                        format!("No handler for activity type: {}", activity_type),
                    );
                    return;
                };

                let cancel_token = CancellationToken::new();
                self.task_tokens_to_cancels
                    .write()
                    .insert(task.task_token.clone(), cancel_token.clone());

                let info = ActivityInfo {
                    activity_id: start.activity_id,
                    activity_type: start.activity_type,
                    workflow_id: start
                        .workflow_execution
                        .as_ref()
                        .map(|e| e.workflow_id.clone())
                        .unwrap_or_default(),
                    run_id: start
                        .workflow_execution
                        .as_ref()
                        .map(|e| e.run_id.clone())
                        .unwrap_or_default(),
                    task_queue: self.task_queue.clone(),
                    attempt: start.attempt,
                    scheduled_time: None,
                    start_time: None,
                };

                let ctx = ActivityContext::new_with_cancel(info, cancel_token);
                let input = ActivityInput {
                    payload: start
                        .input
                        .into_iter()
                        .next()
                        .and_then(|p| serde_json::from_slice(&p.data).ok())
                        .unwrap_or(serde_json::Value::Null),
                };

                let task_token = task.task_token;
                let task_tokens = self.task_tokens_to_cancels.clone();

                tokio::spawn(async move {
                    let result = AssertUnwindSafe(handler(ctx, input)).catch_unwind().await;
                    let exec_result = match result {
                        Ok(Ok(value)) => {
                            let data = serde_json::to_vec(&value).unwrap_or_default();
                            ActivityExecutionResult::ok(Payload {
                                metadata: Default::default(),
                                data,
                            })
                        }
                        Ok(Err(e)) => {
                            let msg = e.to_string();
                            ActivityExecutionResult::fail(Failure::application_failure(msg, true))
                        }
                        Err(_) => ActivityExecutionResult::fail(Failure::application_failure(
                            "Activity panicked".to_string(),
                            true,
                        )),
                    };

                    task_tokens.write().remove(&task_token);
                    let _ = core_worker
                        .inner()
                        .complete_activity_task(ActivityTaskCompletion {
                            task_token,
                            result: Some(exec_result),
                        })
                        .await;
                });
            }
            Some(activity_task::Variant::Cancel(_)) => {
                if let Some(ct) = self.task_tokens_to_cancels.read().get(&task.task_token) {
                    ct.cancel();
                }
            }
            None => {
                tracing::warn!("activity task with no variant");
            }
        }
    }

    fn complete_activity_with_error(
        &self,
        core_worker: Arc<CoreWorker>,
        task_token: Vec<u8>,
        msg: String,
    ) {
        tokio::spawn(async move {
            let result =
                ActivityExecutionResult::fail(Failure::application_failure(msg, true));
            let _ = core_worker
                .inner()
                .complete_activity_task(ActivityTaskCompletion {
                    task_token,
                    result: Some(result),
                })
                .await;
        });
    }

    /// Request the worker to shut down gracefully.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Register an activity by name.
    pub fn register_activity(&self, name: impl Into<String>, handler: ActivityHandler) {
        self.activities.write().insert(name.into(), handler);
    }

    /// Register a workflow by name.
    pub fn register_workflow(&self, name: impl Into<String>, handler: WorkflowHandler) {
        self.workflows.write().insert(name.into(), handler);
    }
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("task_queue", &self.task_queue)
            .field(
                "activities",
                &self.activities.read().keys().collect::<Vec<_>>(),
            )
            .field(
                "workflows",
                &self.workflows.read().keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Builder for creating workers.
#[derive(Default)]
pub struct WorkerBuilder {
    client: Option<Client>,
    task_queue: Option<String>,
    activities: HashMap<String, ActivityHandler>,
    workflows: HashMap<String, WorkflowHandler>,
}

impl WorkerBuilder {
    /// Create a new worker builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the client to use.
    #[must_use]
    pub fn client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Set the task queue to poll from.
    #[must_use]
    pub fn task_queue(mut self, task_queue: impl Into<String>) -> Self {
        self.task_queue = Some(task_queue.into());
        self
    }

    /// Register an activity from a registration struct.
    #[must_use]
    pub fn activity_registration(mut self, registration: ActivityRegistration) -> Self {
        self.activities
            .insert(registration.name, registration.handler);
        self
    }

    /// Register an activity by name and handler.
    #[must_use]
    pub fn activity_handler(mut self, name: impl Into<String>, handler: ActivityHandler) -> Self {
        self.activities.insert(name.into(), handler);
        self
    }

    /// Register a workflow from a registration struct.
    #[must_use]
    pub fn workflow_registration(mut self, registration: WorkflowRegistration) -> Self {
        self.workflows.insert(registration.name, registration.handler);
        self
    }

    /// Register a workflow by name and handler.
    #[must_use]
    pub fn workflow_handler(mut self, name: impl Into<String>, handler: WorkflowHandler) -> Self {
        self.workflows.insert(name.into(), handler);
        self
    }

    /// Build the worker.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> Result<Worker, Error> {
        let client = self
            .client
            .ok_or_else(|| Error::Worker(WorkerError::Init("client is required".to_string())))?;

        let task_queue = self.task_queue.ok_or_else(|| {
            Error::Worker(WorkerError::Init("task_queue is required".to_string()))
        })?;

        Ok(Worker {
            client,
            task_queue,
            activities: Arc::new(RwLock::new(self.activities)),
            workflows: Arc::new(RwLock::new(self.workflows)),
            task_tokens_to_cancels: Arc::new(RwLock::new(HashMap::new())),
            shutdown: CancellationToken::new(),
        })
    }
}

impl std::fmt::Debug for WorkerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerBuilder")
            .field("task_queue", &self.task_queue)
            .field("activities", &self.activities.keys().collect::<Vec<_>>())
            .field("workflows", &self.workflows.keys().collect::<Vec<_>>())
            .finish()
    }
}
