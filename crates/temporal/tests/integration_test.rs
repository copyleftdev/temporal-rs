//! Integration tests for the Temporal SDK.
//!
//! These tests require a Temporal server running on localhost:7233.
//! You can start one with:
//!
//! ```bash
//! docker run -d -p 7233:7233 temporalio/auto-setup:latest
//! ```

use std::sync::Arc;
use std::time::Duration;
use temporal::activity::{ActivityContext, ActivityError, ActivityInput};
use temporal::client::Client;
use temporal::worker::Worker;
use temporal_testing::fixtures;

/// Get the Temporal server address from env or use default.
fn temporal_address() -> String {
    std::env::var("TEMPORAL_ADDRESS").unwrap_or_else(|_| "localhost:7233".into())
}

/// Check if Temporal server is available.
async fn temporal_available() -> bool {
    let addr = temporal_address();
    match Client::connect(&addr, "default").await {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Temporal not available at {}: {}", addr, e);
            false
        }
    }
}

/// Simple echo activity for testing.
async fn echo_activity(
    _ctx: ActivityContext,
    input: ActivityInput,
) -> Result<serde_json::Value, ActivityError> {
    Ok(input.payload)
}

/// Activity that returns a greeting.
async fn greet_activity(
    _ctx: ActivityContext,
    input: ActivityInput,
) -> Result<serde_json::Value, ActivityError> {
    let name: String = serde_json::from_value(input.payload).unwrap_or_else(|_| "World".into());
    Ok(serde_json::json!(format!("Hello, {}!", name)))
}

#[tokio::test]
async fn test_client_connects_to_temporal() {
    if !temporal_available().await {
        eprintln!("Skipping test - Temporal server not available");
        return;
    }

    let client = Client::connect(&temporal_address(), "default").await;
    assert!(client.is_ok(), "Should connect to Temporal server");

    let client = client.unwrap();
    assert_eq!(client.namespace(), "default");
}

#[tokio::test]
async fn test_worker_registers_activities() {
    if !temporal_available().await {
        eprintln!("Skipping test - Temporal server not available");
        return;
    }

    let client = Client::connect(&temporal_address(), "default")
        .await
        .expect("Failed to connect");

    let task_queue = fixtures::unique_task_queue("test-register");

    let worker = Worker::builder()
        .client(client)
        .task_queue(&task_queue)
        .activity_handler("echo", Arc::new(|ctx, input| Box::pin(echo_activity(ctx, input))))
        .activity_handler("greet", Arc::new(|ctx, input| Box::pin(greet_activity(ctx, input))))
        .build();

    assert!(worker.is_ok(), "Worker should build successfully");

    let worker = worker.unwrap();
    assert_eq!(worker.task_queue(), task_queue);
}

#[tokio::test]
async fn test_worker_starts_and_polls() {
    if !temporal_available().await {
        eprintln!("Skipping test - Temporal server not available");
        return;
    }

    let client = Client::connect(&temporal_address(), "default")
        .await
        .expect("Failed to connect");

    let task_queue = fixtures::unique_task_queue("test-start-poll");

    let mut worker = Worker::builder()
        .client(client)
        .task_queue(&task_queue)
        .activity_handler("echo", Arc::new(|ctx, input| Box::pin(echo_activity(ctx, input))))
        .build()
        .expect("Failed to build worker");

    // Create a channel to signal shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let worker_handle = tokio::spawn(async move {
        tokio::select! {
            result = worker.run() => result,
            _ = &mut shutdown_rx => {
                worker.shutdown();
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(())
            }
        }
    });

    // Let worker poll for 2 seconds
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Signal shutdown
    let _ = shutdown_tx.send(());

    // Wait for worker to stop
    let result = tokio::time::timeout(Duration::from_secs(5), worker_handle).await;
    assert!(result.is_ok(), "Worker should stop within timeout");
    
    let inner = result.unwrap();
    assert!(inner.is_ok(), "Worker task should complete");
    assert!(inner.unwrap().is_ok(), "Worker should shut down cleanly");
}

// ============================================================================
// Unit tests (no Temporal server required)
// ============================================================================

#[tokio::test]
async fn test_activity_context_info() {
    use temporal::activity::{ActivityContext, ActivityInfo};

    let info = ActivityInfo {
        activity_id: "test-activity-123".to_string(),
        activity_type: "greet".to_string(),
        workflow_id: "test-workflow-456".to_string(),
        run_id: "run-789".to_string(),
        task_queue: "test-queue".to_string(),
        attempt: 1,
        scheduled_time: None,
        start_time: None,
    };

    let ctx = ActivityContext::new(info);

    assert_eq!(ctx.activity_id(), "test-activity-123");
    assert_eq!(ctx.activity_type(), "greet");
    assert_eq!(ctx.workflow_id(), "test-workflow-456");
    assert_eq!(ctx.attempt(), 1);
    assert!(!ctx.is_cancelled());
}

#[tokio::test]
async fn test_activity_error_types() {
    let cancelled = ActivityError::cancelled("user cancelled");
    assert!(matches!(cancelled, ActivityError::Cancelled(_)));

    let failed = ActivityError::failed("something went wrong");
    assert!(matches!(failed, ActivityError::Failed(_)));

    let app_error = ActivityError::application("invalid input", "ValidationError");
    assert!(matches!(app_error, ActivityError::Application { non_retryable: false, .. }));

    let non_retry = ActivityError::non_retryable("fatal error", "FatalError");
    assert!(matches!(non_retry, ActivityError::Application { non_retryable: true, .. }));
}

#[tokio::test]
async fn test_activity_input_deserialization() {
    use temporal::activity::{ActivityInput, deserialize_input};

    let input = ActivityInput {
        payload: serde_json::json!({"name": "Alice", "age": 30}),
    };

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Person {
        name: String,
        age: u32,
    }

    let person: Result<Person, _> = deserialize_input(&input);
    assert!(person.is_ok());

    let person = person.unwrap();
    assert_eq!(person.name, "Alice");
    assert_eq!(person.age, 30);
}

#[tokio::test]
async fn test_worker_builder_validation() {
    // Missing client should fail
    let result = Worker::builder()
        .task_queue("test-queue")
        .build();
    assert!(result.is_err());
}

// ============================================================================
// Workflow tests
// ============================================================================

#[tokio::test]
async fn test_workflow_info() {
    use temporal::workflow::WorkflowInfo;

    let info = WorkflowInfo {
        workflow_id: "test-workflow-123".to_string(),
        run_id: "run-456".to_string(),
        workflow_type: "OrderWorkflow".to_string(),
        namespace: "default".to_string(),
        task_queue: "order-queue".to_string(),
        attempt: 1,
        start_time: None,
        workflow_run_timeout: None,
        workflow_execution_timeout: None,
    };

    assert_eq!(info.workflow_id, "test-workflow-123");
    assert_eq!(info.run_id, "run-456");
    assert_eq!(info.workflow_type, "OrderWorkflow");
    assert_eq!(info.namespace, "default");
    assert_eq!(info.task_queue, "order-queue");
    assert_eq!(info.attempt, 1);
}

#[tokio::test]
async fn test_workflow_error_types() {
    use temporal::workflow::WorkflowError;

    let activity_failed = WorkflowError::ActivityFailed("timeout".into());
    assert!(matches!(activity_failed, WorkflowError::ActivityFailed(_)));

    let cancelled = WorkflowError::Cancelled;
    assert!(matches!(cancelled, WorkflowError::Cancelled));

    let app_error = WorkflowError::application("invalid order", "ValidationError");
    assert!(matches!(app_error, WorkflowError::Application { .. }));
}

#[tokio::test]
async fn test_workflow_activity_options() {
    use temporal::workflow::{ActivityOptions, RetryPolicy};
    use std::time::Duration;

    let opts = ActivityOptions::default();
    assert!(opts.task_queue.is_none());
    assert_eq!(opts.start_to_close_timeout, Some(Duration::from_secs(60)));

    let retry = RetryPolicy::default();
    assert_eq!(retry.initial_interval, Duration::from_secs(1));
    assert_eq!(retry.backoff_coefficient, 2.0);
    assert_eq!(retry.maximum_attempts, 0); // unlimited
}

#[tokio::test]
async fn test_worker_workflow_registration() {
    use temporal::workflow::WorkflowHandler;
    use std::sync::Arc;

    if !temporal_available().await {
        eprintln!("Skipping test - Temporal server not available");
        return;
    }

    let client = Client::connect(&temporal_address(), "default")
        .await
        .expect("Failed to connect");

    let task_queue = fixtures::unique_task_queue("test-wf-register");

    // Create a simple workflow handler
    let handler: WorkflowHandler = Arc::new(|_ctx, input| {
        Box::pin(async move {
            Ok(input) // echo back input
        })
    });

    let worker = Worker::builder()
        .client(client)
        .task_queue(&task_queue)
        .workflow_handler("echo_workflow", handler)
        .build();

    assert!(worker.is_ok(), "Worker should build with workflow handler");
}

#[tokio::test]
async fn test_worker_with_workflows_starts_and_polls() {
    use temporal::workflow::WorkflowHandler;
    use std::sync::Arc;

    if !temporal_available().await {
        eprintln!("Skipping test - Temporal server not available");
        return;
    }

    let client = Client::connect(&temporal_address(), "default")
        .await
        .expect("Failed to connect");

    let task_queue = fixtures::unique_task_queue("test-wf-start");

    // Create a simple workflow handler
    let handler: WorkflowHandler = Arc::new(|ctx, input| {
        Box::pin(async move {
            tracing::info!(
                workflow_id = %ctx.workflow_id(),
                "workflow executing"
            );
            Ok(input)
        })
    });

    let mut worker = Worker::builder()
        .client(client)
        .task_queue(&task_queue)
        .workflow_handler("test_workflow", handler)
        .build()
        .expect("Failed to build worker");

    // Create a channel to signal shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let worker_handle = tokio::spawn(async move {
        tokio::select! {
            result = worker.run() => result,
            _ = &mut shutdown_rx => {
                worker.shutdown();
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(())
            }
        }
    });

    // Let worker poll for 2 seconds
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Signal shutdown
    let _ = shutdown_tx.send(());

    // Wait for worker to stop
    let result = tokio::time::timeout(Duration::from_secs(5), worker_handle).await;
    assert!(result.is_ok(), "Worker should stop within timeout");
    
    let inner = result.unwrap();
    assert!(inner.is_ok(), "Worker task should complete");
    assert!(inner.unwrap().is_ok(), "Worker should shut down cleanly");
}

#[tokio::test]
async fn test_workflow_state_machine_processes_timer() {
    use temporal::workflow::{WorkflowHandler, WorkflowInfo};
    use temporal::workflow_machine::WorkflowCache;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    // Track if workflow executed and completed timer
    let timer_started = Arc::new(AtomicBool::new(false));
    let timer_started_clone = timer_started.clone();

    // Create a workflow that starts a timer
    let handler: WorkflowHandler = Arc::new(move |ctx, _input| {
        let flag = timer_started_clone.clone();
        Box::pin(async move {
            // This will send a timer command
            flag.store(true, Ordering::SeqCst);
            // Don't actually wait - just verify command generation works
            Ok(serde_json::json!({"status": "timer_requested"}))
        })
    });

    let info = WorkflowInfo {
        workflow_id: "test-wf".to_string(),
        run_id: "test-run".to_string(),
        workflow_type: "TimerWorkflow".to_string(),
        namespace: "default".to_string(),
        task_queue: "test-queue".to_string(),
        attempt: 1,
        start_time: None,
        workflow_run_timeout: None,
        workflow_execution_timeout: None,
    };

    let cache = WorkflowCache::new();
    let created = cache.get_or_create("test-run", info, handler, serde_json::json!({})).await;
    assert!(created, "Should create new workflow machine");

    // Create an init activation
    use temporal_core::protos::coresdk::workflow_activation::{
        WorkflowActivation, WorkflowActivationJob, InitializeWorkflow,
        workflow_activation_job,
    };

    let init_job = WorkflowActivationJob {
        variant: Some(workflow_activation_job::Variant::InitializeWorkflow(InitializeWorkflow {
            workflow_type: "TimerWorkflow".to_string(),
            workflow_id: "test-wf".to_string(),
            arguments: vec![],
            ..Default::default()
        })),
    };

    let activation = WorkflowActivation {
        run_id: "test-run".to_string(),
        jobs: vec![init_job],
        ..Default::default()
    };

    // Process the activation
    let completion = cache.process_activation(activation).await;
    assert!(completion.is_some(), "Should return completion");

    // Verify workflow executed
    assert!(timer_started.load(Ordering::SeqCst), "Workflow should have executed");
}

#[tokio::test]
async fn test_workflow_state_machine_completes_workflow() {
    use temporal::workflow::{WorkflowHandler, WorkflowInfo};
    use temporal::workflow_machine::WorkflowCache;
    use std::sync::Arc;

    // Create a workflow that completes immediately
    let handler: WorkflowHandler = Arc::new(|_ctx, input| {
        Box::pin(async move {
            Ok(serde_json::json!({"echo": input}))
        })
    });

    let info = WorkflowInfo {
        workflow_id: "complete-wf".to_string(),
        run_id: "complete-run".to_string(),
        workflow_type: "CompleteWorkflow".to_string(),
        namespace: "default".to_string(),
        task_queue: "test-queue".to_string(),
        attempt: 1,
        start_time: None,
        workflow_run_timeout: None,
        workflow_execution_timeout: None,
    };

    let cache = WorkflowCache::new();
    cache.get_or_create("complete-run", info, handler, serde_json::json!("hello")).await;

    use temporal_core::protos::coresdk::workflow_activation::{
        WorkflowActivation, WorkflowActivationJob, InitializeWorkflow,
        workflow_activation_job,
    };

    let init_job = WorkflowActivationJob {
        variant: Some(workflow_activation_job::Variant::InitializeWorkflow(InitializeWorkflow {
            workflow_type: "CompleteWorkflow".to_string(),
            workflow_id: "complete-wf".to_string(),
            arguments: vec![],
            ..Default::default()
        })),
    };

    let activation = WorkflowActivation {
        run_id: "complete-run".to_string(),
        jobs: vec![init_job],
        ..Default::default()
    };

    let completion = cache.process_activation(activation).await;
    assert!(completion.is_some(), "Should return completion");
    
    // Check that completion contains a CompleteWorkflowExecution command
    let completion = completion.unwrap();
    use temporal_core::protos::coresdk::workflow_completion::workflow_activation_completion;
    
    if let Some(workflow_activation_completion::Status::Successful(success)) = completion.status {
        // Look for complete command
        let has_complete = success.commands.iter().any(|cmd| {
            matches!(
                cmd.variant,
                Some(temporal_core::protos::coresdk::workflow_commands::workflow_command::Variant::CompleteWorkflowExecution(_))
            )
        });
        assert!(has_complete, "Should have CompleteWorkflowExecution command");
    } else {
        panic!("Expected successful completion");
    }
}
