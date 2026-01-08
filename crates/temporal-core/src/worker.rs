//! Core worker for polling and executing tasks.

use crate::client::CoreClient;
use crate::error::{CoreError, Result};
use crate::runtime::CoreRuntime;
use std::sync::Arc;
use temporalio_common::worker::{WorkerConfig, WorkerTaskTypes, WorkerVersioningStrategy};
use temporalio_common::Worker as WorkerTrait;
use temporalio_sdk_core::init_worker;

/// Configuration for a worker.
#[derive(Debug, Clone)]
pub struct WorkerOptions {
    /// The task queue to poll from.
    pub task_queue: String,
    /// Maximum concurrent workflow tasks.
    pub max_concurrent_workflow_tasks: usize,
    /// Maximum concurrent activity tasks.
    pub max_concurrent_activities: usize,
    /// Maximum concurrent local activity tasks.
    pub max_concurrent_local_activities: usize,
    /// Build ID for versioning.
    pub build_id: String,
}

impl WorkerOptions {
    /// Create new worker options for the given task queue.
    #[must_use]
    pub fn new(task_queue: impl Into<String>) -> Self {
        Self {
            task_queue: task_queue.into(),
            max_concurrent_workflow_tasks: 100,
            max_concurrent_activities: 100,
            max_concurrent_local_activities: 100,
            build_id: format!("temporal-rs-{}", env!("CARGO_PKG_VERSION")),
        }
    }

    /// Set the maximum concurrent workflow tasks.
    #[must_use]
    pub const fn with_max_concurrent_workflow_tasks(mut self, max: usize) -> Self {
        self.max_concurrent_workflow_tasks = max;
        self
    }

    /// Set the maximum concurrent activity tasks.
    #[must_use]
    pub const fn with_max_concurrent_activities(mut self, max: usize) -> Self {
        self.max_concurrent_activities = max;
        self
    }

    /// Set the build ID for versioning.
    #[must_use]
    pub fn with_build_id(mut self, build_id: impl Into<String>) -> Self {
        self.build_id = build_id.into();
        self
    }
}

/// Wrapper around the SDK Core worker.
pub struct CoreWorker {
    inner: Arc<dyn WorkerTrait>,
    options: WorkerOptions,
}

impl CoreWorker {
    /// Create a new worker with the given options.
    ///
    /// # Errors
    ///
    /// Returns an error if worker initialization fails.
    pub fn new(
        client: &CoreClient,
        runtime: &CoreRuntime,
        options: WorkerOptions,
    ) -> Result<Self> {
        let config = WorkerConfig::builder()
            .namespace(client.namespace())
            .task_queue(&options.task_queue)
            .max_outstanding_workflow_tasks(options.max_concurrent_workflow_tasks)
            .max_outstanding_activities(options.max_concurrent_activities)
            .max_outstanding_local_activities(options.max_concurrent_local_activities)
            .versioning_strategy(WorkerVersioningStrategy::None {
                build_id: options.build_id.clone(),
            })
            .task_types(WorkerTaskTypes::all())
            .build()
            .map_err(|e| CoreError::InvalidConfig(e.to_string()))?;

        let worker = init_worker(runtime.inner(), config, client.inner().clone())
            .map_err(|e| CoreError::WorkerInit(e.to_string()))?;

        Ok(Self {
            inner: Arc::new(worker),
            options,
        })
    }

    /// Get the task queue this worker is polling.
    #[must_use]
    pub fn task_queue(&self) -> &str {
        &self.options.task_queue
    }

    /// Get the inner worker trait object.
    #[must_use]
    pub fn inner(&self) -> &Arc<dyn WorkerTrait> {
        &self.inner
    }

    /// Shutdown the worker gracefully.
    pub async fn shutdown(&self) {
        self.inner.shutdown().await;
    }
}
