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

use crate::activity::{ActivityHandler, ActivityRegistration};
use crate::client::Client;
use crate::error::{Error, WorkerError};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use temporal_core::worker::WorkerOptions;
use temporal_core::CoreWorker;
use tokio_util::sync::CancellationToken;

/// A worker that polls for and executes tasks.
pub struct Worker {
    client: Client,
    task_queue: String,
    activities: Arc<RwLock<HashMap<String, ActivityHandler>>>,
    core_worker: Option<CoreWorker>,
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
        tracing::info!(
            task_queue = %self.task_queue,
            activities = ?self.activities.read().keys().collect::<Vec<_>>(),
            "starting worker"
        );

        let options = WorkerOptions::new(&self.task_queue);
        let core_worker =
            CoreWorker::new(self.client.inner(), self.client.runtime(), options)?;

        self.core_worker = Some(core_worker);

        // Main worker loop
        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => {
                    tracing::info!("worker shutdown requested");
                    break;
                }
                // TODO: Poll for tasks and execute them
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    // Placeholder for actual polling
                }
            }
        }

        // Shutdown core worker
        if let Some(ref worker) = self.core_worker {
            worker.shutdown().await;
        }

        tracing::info!("worker stopped");
        Ok(())
    }

    /// Request the worker to shut down gracefully.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Register an activity by name.
    pub fn register_activity(&self, name: impl Into<String>, handler: ActivityHandler) {
        self.activities.write().insert(name.into(), handler);
    }
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("task_queue", &self.task_queue)
            .field("activities", &self.activities.read().keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Builder for creating workers.
#[derive(Default)]
pub struct WorkerBuilder {
    client: Option<Client>,
    task_queue: Option<String>,
    activities: HashMap<String, ActivityHandler>,
    max_concurrent_activities: Option<usize>,
    max_concurrent_workflow_tasks: Option<usize>,
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
        self.activities.insert(registration.name, registration.handler);
        self
    }

    /// Register an activity by name and handler.
    #[must_use]
    pub fn activity_handler(mut self, name: impl Into<String>, handler: ActivityHandler) -> Self {
        self.activities.insert(name.into(), handler);
        self
    }

    /// Set the maximum concurrent activity executions.
    #[must_use]
    pub const fn max_concurrent_activities(mut self, max: usize) -> Self {
        self.max_concurrent_activities = Some(max);
        self
    }

    /// Set the maximum concurrent workflow task executions.
    #[must_use]
    pub const fn max_concurrent_workflow_tasks(mut self, max: usize) -> Self {
        self.max_concurrent_workflow_tasks = Some(max);
        self
    }

    /// Build the worker.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> Result<Worker, Error> {
        let client = self.client.ok_or_else(|| {
            Error::Worker(WorkerError::Init("client is required".to_string()))
        })?;

        let task_queue = self.task_queue.ok_or_else(|| {
            Error::Worker(WorkerError::Init("task_queue is required".to_string()))
        })?;

        Ok(Worker {
            client,
            task_queue,
            activities: Arc::new(RwLock::new(self.activities)),
            core_worker: None,
            shutdown: CancellationToken::new(),
        })
    }
}

impl std::fmt::Debug for WorkerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerBuilder")
            .field("task_queue", &self.task_queue)
            .field("activities", &self.activities.keys().collect::<Vec<_>>())
            .finish()
    }
}
