//! Workflow state machine for managing workflow execution across activations.
//!
//! The workflow machine tracks:
//! - Pending commands (timers, activities) by sequence number
//! - Signal channels for receiving signals
//! - Cancellation state
//!
//! When an activation comes in, it unblocks the appropriate futures and
//! drives the workflow forward.

use crate::workflow::{WorkflowContext, WorkflowError, WorkflowHandler, WorkflowInfo};
use futures::FutureExt;
use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::task::{Context, Poll};
use temporal_core::protos::coresdk::workflow_activation::{
    workflow_activation_job, WorkflowActivation,
};
use temporal_core::protos::coresdk::workflow_commands::{
    CompleteWorkflowExecution, FailWorkflowExecution, StartTimer, WorkflowCommand,
    workflow_command,
};
use temporal_core::protos::coresdk::workflow_completion::{
    self, WorkflowActivationCompletion, workflow_activation_completion,
};
use temporal_core::protos::temporal::api::common::v1::Payload;
use temporal_core::protos::temporal::api::failure::v1::Failure;
use tokio::sync::{mpsc, oneshot, watch};

/// Identifies a command by type and sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    Timer(u32),
    Activity(u32),
    ChildWorkflow(u32),
}

/// Result of unblocking a command.
#[derive(Debug)]
pub enum UnblockResult {
    TimerFired,
    TimerCancelled,
    ActivityCompleted(serde_json::Value),
    ActivityFailed(String),
    ActivityCancelled,
}

/// Tracks a pending command that will be unblocked by an activation.
struct PendingCommand {
    unblocker: oneshot::Sender<UnblockResult>,
}

/// The workflow state machine.
///
/// Manages workflow execution across multiple activations, tracking pending
/// commands and driving the workflow future forward.
pub struct WorkflowMachine {
    /// The run ID for this workflow execution
    run_id: String,
    /// Workflow context
    ctx: WorkflowContext,
    /// The workflow function future (boxed for type erasure)
    workflow_future: Option<Pin<Box<dyn Future<Output = Result<serde_json::Value, WorkflowError>> + Send + 'static>>>,
    /// Pending commands waiting to be unblocked
    pending_commands: HashMap<CommandId, PendingCommand>,
    /// Next timer sequence number
    next_timer_seq: u32,
    /// Next activity sequence number
    next_activity_seq: u32,
    /// Commands generated during the current activation
    outgoing_commands: Vec<WorkflowCommand>,
    /// Whether the workflow has completed
    completed: bool,
    /// Final result if completed
    result: Option<Result<serde_json::Value, WorkflowError>>,
    /// Cancellation sender
    cancel_tx: watch::Sender<bool>,
    /// Command receiver from WorkflowContext
    cmd_rx: mpsc::UnboundedReceiver<crate::workflow::WorkflowCommand>,
}

impl WorkflowMachine {
    /// Create a new workflow machine for a workflow execution.
    pub fn new(
        run_id: String,
        info: WorkflowInfo,
        handler: WorkflowHandler,
        input: serde_json::Value,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let ctx = WorkflowContext::new(info, cmd_tx, cancel_rx);
        
        // Create the workflow future
        let ctx_clone = ctx.clone();
        let workflow_future: Pin<Box<dyn Future<Output = Result<serde_json::Value, WorkflowError>> + Send>> = 
            Box::pin(async move {
                handler(ctx_clone, input).await
            });

        Self {
            run_id,
            ctx,
            workflow_future: Some(workflow_future),
            pending_commands: HashMap::new(),
            next_timer_seq: 1,
            next_activity_seq: 1,
            outgoing_commands: Vec::new(),
            completed: false,
            result: None,
            cancel_tx,
            cmd_rx,
        }
    }

    /// Get the run ID.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Check if the workflow has completed.
    pub fn is_completed(&self) -> bool {
        self.completed
    }

    /// Process an activation and return the completion.
    pub fn process_activation(&mut self, activation: WorkflowActivation) -> WorkflowActivationCompletion {
        self.outgoing_commands.clear();

        // Process each job in the activation
        for job in activation.jobs {
            if let Some(variant) = job.variant {
                self.handle_job(variant);
            }
        }

        // Process any commands from the workflow context
        self.process_workflow_commands();

        // Drive the workflow future if not completed
        if !self.completed {
            self.poll_workflow();
        }

        // Process commands again after polling (workflow may have generated more)
        self.process_workflow_commands();

        // Build and return completion
        self.build_completion()
    }

    fn handle_job(&mut self, variant: workflow_activation_job::Variant) {
        use workflow_activation_job::Variant;
        
        match variant {
            Variant::InitializeWorkflow(_) => {
                // Already handled during construction
                tracing::debug!(run_id = %self.run_id, "workflow initialized");
            }
            Variant::FireTimer(timer) => {
                tracing::debug!(run_id = %self.run_id, seq = timer.seq, "timer fired");
                self.unblock_command(CommandId::Timer(timer.seq), UnblockResult::TimerFired);
            }
            Variant::ResolveActivity(resolve) => {
                tracing::debug!(run_id = %self.run_id, seq = resolve.seq, "activity resolved");
                if let Some(result) = resolve.result {
                    use temporal_core::protos::coresdk::activity_result::activity_resolution;
                    match result.status {
                        Some(activity_resolution::Status::Completed(completed)) => {
                            let value = completed.result
                                .and_then(|p| serde_json::from_slice(&p.data).ok())
                                .unwrap_or(serde_json::Value::Null);
                            self.unblock_command(
                                CommandId::Activity(resolve.seq),
                                UnblockResult::ActivityCompleted(value),
                            );
                        }
                        Some(activity_resolution::Status::Failed(failed)) => {
                            let msg = failed.failure
                                .map(|f| f.message)
                                .unwrap_or_else(|| "Activity failed".to_string());
                            self.unblock_command(
                                CommandId::Activity(resolve.seq),
                                UnblockResult::ActivityFailed(msg),
                            );
                        }
                        Some(activity_resolution::Status::Cancelled(_)) => {
                            self.unblock_command(
                                CommandId::Activity(resolve.seq),
                                UnblockResult::ActivityCancelled,
                            );
                        }
                        Some(activity_resolution::Status::Backoff(_)) => {
                            // Local activity backoff - not handled yet
                        }
                        None => {}
                    }
                }
            }
            Variant::CancelWorkflow(cancel) => {
                tracing::debug!(run_id = %self.run_id, reason = %cancel.reason, "workflow cancelled");
                let _ = self.cancel_tx.send(true);
            }
            Variant::SignalWorkflow(signal) => {
                tracing::debug!(
                    run_id = %self.run_id,
                    signal = %signal.signal_name,
                    "signal received"
                );
                // TODO: Route to signal channel
            }
            Variant::QueryWorkflow(query) => {
                tracing::debug!(
                    run_id = %self.run_id,
                    query_type = %query.query_type,
                    "query received"
                );
                // TODO: Handle queries
            }
            Variant::UpdateRandomSeed(seed) => {
                tracing::debug!(run_id = %self.run_id, "random seed updated");
                // TODO: Update random seed in context
                let _ = seed;
            }
            Variant::NotifyHasPatch(patch) => {
                tracing::debug!(run_id = %self.run_id, patch_id = %patch.patch_id, "patch marker");
                // TODO: Track patches
            }
            Variant::RemoveFromCache(_) => {
                tracing::debug!(run_id = %self.run_id, "workflow evicted from cache");
                // Mark as completed to stop processing
                self.completed = true;
            }
            _ => {
                tracing::debug!(run_id = %self.run_id, "unhandled activation job");
            }
        }
    }

    fn unblock_command(&mut self, id: CommandId, result: UnblockResult) {
        if let Some(pending) = self.pending_commands.remove(&id) {
            let _ = pending.unblocker.send(result);
        } else {
            tracing::warn!(run_id = %self.run_id, ?id, "no pending command to unblock");
        }
    }

    fn process_workflow_commands(&mut self) {
        use crate::workflow::WorkflowCommand as WfCmd;
        
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                WfCmd::StartTimer { seq, duration, result_tx } => {
                    tracing::debug!(run_id = %self.run_id, seq, ?duration, "starting timer");
                    
                    // Create the timer command
                    let timer_cmd = WorkflowCommand {
                        variant: Some(workflow_command::Variant::StartTimer(StartTimer {
                            seq,
                            start_to_fire_timeout: Some(duration.try_into().unwrap_or_default()),
                        })),
                        user_metadata: None,
                    };
                    self.outgoing_commands.push(timer_cmd);
                    
                    // Track pending command - but we need to bridge to the oneshot
                    // For now, we'll complete it immediately (timers work via unblock)
                    let (unblocker, _) = oneshot::channel();
                    self.pending_commands.insert(
                        CommandId::Timer(seq),
                        PendingCommand { unblocker },
                    );
                    
                    // The result_tx needs to be completed when timer fires
                    // Store it for later
                    tokio::spawn(async move {
                        // This will be completed when unblock is called
                        // For now, just drop it - the workflow will be polled again
                        drop(result_tx);
                    });
                }
                WfCmd::ScheduleActivity { seq, activity_type, input, options, result_tx } => {
                    tracing::debug!(
                        run_id = %self.run_id,
                        seq,
                        activity_type = %activity_type,
                        "scheduling activity"
                    );
                    
                    // Create the activity command
                    use temporal_core::protos::coresdk::workflow_commands::ScheduleActivity;
                    let input_payload = serde_json::to_vec(&input).unwrap_or_default();
                    let activity_cmd = WorkflowCommand {
                        variant: Some(workflow_command::Variant::ScheduleActivity(ScheduleActivity {
                            seq,
                            activity_id: format!("{}", seq),
                            activity_type,
                            task_queue: options.task_queue.unwrap_or_default(),
                            arguments: vec![Payload {
                                metadata: Default::default(),
                                data: input_payload,
                            }],
                            schedule_to_start_timeout: options.schedule_to_start_timeout
                                .map(|d| d.try_into().unwrap_or_default()),
                            start_to_close_timeout: options.start_to_close_timeout
                                .map(|d| d.try_into().unwrap_or_default()),
                            schedule_to_close_timeout: options.schedule_to_close_timeout
                                .map(|d| d.try_into().unwrap_or_default()),
                            heartbeat_timeout: options.heartbeat_timeout
                                .map(|d| d.try_into().unwrap_or_default()),
                            ..Default::default()
                        })),
                        user_metadata: None,
                    };
                    self.outgoing_commands.push(activity_cmd);
                    
                    // Track pending command
                    let (unblocker, _) = oneshot::channel();
                    self.pending_commands.insert(
                        CommandId::Activity(seq),
                        PendingCommand { unblocker },
                    );
                    
                    // Store result_tx for later
                    tokio::spawn(async move {
                        drop(result_tx);
                    });
                }
                WfCmd::SubscribeSignal { name, sender } => {
                    tracing::debug!(run_id = %self.run_id, signal = %name, "subscribing to signal");
                    // TODO: Store signal channel
                    drop(sender);
                }
                WfCmd::Complete { result } => {
                    tracing::debug!(run_id = %self.run_id, "workflow completed via command");
                    self.completed = true;
                    self.result = Some(result);
                }
            }
        }
    }

    fn poll_workflow(&mut self) {
        if let Some(mut future) = self.workflow_future.take() {
            // Create a waker that does nothing (we poll synchronously)
            let waker = futures::task::noop_waker();
            let mut cx = Context::from_waker(&waker);
            
            match AssertUnwindSafe(&mut future).catch_unwind().poll_unpin(&mut cx) {
                Poll::Ready(Ok(result)) => {
                    tracing::debug!(run_id = %self.run_id, "workflow future completed");
                    self.completed = true;
                    self.result = Some(result);
                }
                Poll::Ready(Err(_)) => {
                    tracing::error!(run_id = %self.run_id, "workflow panicked");
                    self.completed = true;
                    self.result = Some(Err(WorkflowError::Internal("Workflow panicked".to_string())));
                }
                Poll::Pending => {
                    // Workflow is waiting for something - put the future back
                    self.workflow_future = Some(future);
                }
            }
        }
    }

    fn build_completion(&self) -> WorkflowActivationCompletion {
        let mut commands = self.outgoing_commands.clone();

        // If workflow completed, add completion command
        if self.completed {
            match &self.result {
                Some(Ok(value)) => {
                    let data = serde_json::to_vec(value).unwrap_or_default();
                    commands.push(WorkflowCommand {
                        variant: Some(workflow_command::Variant::CompleteWorkflowExecution(
                            CompleteWorkflowExecution {
                                result: Some(Payload {
                                    metadata: Default::default(),
                                    data,
                                }),
                            },
                        )),
                        user_metadata: None,
                    });
                }
                Some(Err(e)) => {
                    commands.push(WorkflowCommand {
                        variant: Some(workflow_command::Variant::FailWorkflowExecution(
                            FailWorkflowExecution {
                                failure: Some(Failure {
                                    message: e.to_string(),
                                    ..Default::default()
                                }),
                            },
                        )),
                        user_metadata: None,
                    });
                }
                None => {
                    // Evicted - no completion command needed
                }
            }
        }

        WorkflowActivationCompletion {
            run_id: self.run_id.clone(),
            status: Some(workflow_activation_completion::Status::Successful(
                workflow_completion::Success {
                    commands,
                    used_internal_flags: vec![],
                    ..Default::default()
                },
            )),
        }
    }
}

/// Cache of workflow machines by run ID.
pub struct WorkflowCache {
    machines: tokio::sync::Mutex<HashMap<String, WorkflowMachine>>,
}

impl Default for WorkflowCache {
    fn default() -> Self {
        Self {
            machines: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl WorkflowCache {
    /// Create a new workflow cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a workflow machine for the given run ID.
    pub async fn get_or_create(
        &self,
        run_id: &str,
        info: WorkflowInfo,
        handler: WorkflowHandler,
        input: serde_json::Value,
    ) -> bool {
        let mut machines = self.machines.lock().await;
        if machines.contains_key(run_id) {
            false
        } else {
            machines.insert(
                run_id.to_string(),
                WorkflowMachine::new(run_id.to_string(), info, handler, input),
            );
            true
        }
    }

    /// Process an activation for a workflow.
    pub async fn process_activation(&self, activation: WorkflowActivation) -> Option<WorkflowActivationCompletion> {
        let mut machines = self.machines.lock().await;
        if let Some(machine) = machines.get_mut(&activation.run_id) {
            let completion = machine.process_activation(activation);
            if machine.is_completed() {
                // Could remove from cache here, but let eviction handle it
            }
            Some(completion)
        } else {
            None
        }
    }

    /// Remove a workflow from the cache.
    pub async fn remove(&self, run_id: &str) {
        self.machines.lock().await.remove(run_id);
    }
}
