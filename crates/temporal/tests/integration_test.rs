//! Integration tests for the Temporal SDK.
//!
//! These tests require Docker to be running and will spin up a Temporal server
//! using testcontainers.

use std::sync::Arc;
use std::time::Duration;
use temporal::activity::{ActivityContext, ActivityError, ActivityInput};
use temporal::client::Client;
use temporal::worker::Worker;
use temporal_testing::{TemporalContainer, fixtures};
use testcontainers::clients::Cli;

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

/// Activity that simulates failure.
async fn failing_activity(
    _ctx: ActivityContext,
    _input: ActivityInput,
) -> Result<serde_json::Value, ActivityError> {
    Err(ActivityError::failed("intentional failure"))
}

/// Activity that checks cancellation.
async fn cancellable_activity(
    ctx: ActivityContext,
    _input: ActivityInput,
) -> Result<serde_json::Value, ActivityError> {
    for i in 0..10 {
        if ctx.is_cancelled() {
            return Err(ActivityError::cancelled(format!("cancelled at iteration {}", i)));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(serde_json::json!({"completed": true}))
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_worker_connects_to_temporal() {
    let docker = Cli::default();
    let container = TemporalContainer::start(&docker).expect("Failed to start Temporal container");

    let endpoint = container.endpoint();
    println!("Temporal server running at: {}", endpoint);

    // Connect client
    let client = Client::connect(&endpoint, "default").await;
    assert!(client.is_ok(), "Should connect to Temporal server");

    let client = client.unwrap();
    assert_eq!(client.namespace(), "default");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_worker_registers_activities() {
    let docker = Cli::default();
    let container = TemporalContainer::start(&docker).expect("Failed to start Temporal container");

    let client = Client::connect(&container.endpoint(), "default")
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
#[ignore = "requires Docker"]
async fn test_worker_starts_and_stops() {
    let docker = Cli::default();
    let container = TemporalContainer::start(&docker).expect("Failed to start Temporal container");

    let client = Client::connect(&container.endpoint(), "default")
        .await
        .expect("Failed to connect");

    let task_queue = fixtures::unique_task_queue("test-start-stop");

    let mut worker = Worker::builder()
        .client(client)
        .task_queue(&task_queue)
        .activity_handler("echo", Arc::new(|ctx, input| Box::pin(echo_activity(ctx, input))))
        .build()
        .expect("Failed to build worker");

    // Create a channel to signal shutdown from within the spawned task
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let worker_handle = tokio::spawn(async move {
        tokio::select! {
            result = worker.run() => result,
            _ = &mut shutdown_rx => {
                worker.shutdown();
                // Give it a moment to process shutdown
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(())
            }
        }
    });

    // Let worker poll for a bit
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Signal shutdown
    let _ = shutdown_tx.send(());

    // Wait for worker to stop
    let result = tokio::time::timeout(Duration::from_secs(5), worker_handle).await;
    assert!(result.is_ok(), "Worker should stop within timeout");
}

/// Test that verifies the activity context provides correct info.
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

/// Test activity error types.
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

/// Test activity input deserialization.
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

/// Test that worker builder validates required fields.
#[tokio::test]
async fn test_worker_builder_validation() {
    // Missing client
    let result = Worker::builder()
        .task_queue("test-queue")
        .build();
    assert!(result.is_err());

    // Missing task queue - need a client first
    // This would require a connection, so we just verify the builder pattern works
}
