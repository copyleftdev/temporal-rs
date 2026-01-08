//! Example: Simple Activity Worker
//!
//! This example demonstrates how to create a worker that executes activities.
//!
//! To run this example, you need a Temporal server running locally:
//!
//! ```bash
//! # Start Temporal server (using Docker)
//! docker run -d --name temporal -p 7233:7233 temporalio/auto-setup:latest
//!
//! # Run this example
//! cargo run --example activity_worker
//! ```

use std::sync::Arc;
use std::time::Duration;
use temporal::prelude::*;

/// A simple greeting activity that returns a greeting message.
async fn greet_activity(
    ctx: ActivityContext,
    input: ActivityInput,
) -> Result<serde_json::Value, ActivityError> {
    // Extract the name from input
    let name: String = serde_json::from_value(input.payload.clone())
        .unwrap_or_else(|_| "World".to_string());

    tracing::info!(
        activity_id = %ctx.activity_id(),
        name = %name,
        "executing greet activity"
    );

    // Simulate some work
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Check for cancellation
    if ctx.is_cancelled() {
        return Err(ActivityError::cancelled("Activity was cancelled"));
    }

    let greeting = format!("Hello, {}!", name);
    Ok(serde_json::json!(greeting))
}

/// An activity that demonstrates heartbeating for long-running work.
async fn long_running_activity(
    ctx: ActivityContext,
    input: ActivityInput,
) -> Result<serde_json::Value, ActivityError> {
    let iterations: u32 = serde_json::from_value(input.payload.clone()).unwrap_or(5);

    tracing::info!(
        activity_id = %ctx.activity_id(),
        iterations = iterations,
        "starting long-running activity"
    );

    for i in 0..iterations {
        // Check for cancellation
        if ctx.is_cancelled() {
            tracing::warn!("Activity cancelled at iteration {}", i);
            return Err(ActivityError::cancelled(format!(
                "Cancelled at iteration {}",
                i
            )));
        }

        // Send heartbeat with progress
        ctx.heartbeat(serde_json::json!({
            "iteration": i,
            "total": iterations,
            "progress": (i as f64 / iterations as f64) * 100.0
        }));

        // Simulate work
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(serde_json::json!({
        "completed_iterations": iterations,
        "status": "success"
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("temporal=debug".parse()?)
                .add_directive("activity_worker=debug".parse()?),
        )
        .init();

    tracing::info!("Starting activity worker example");

    // Connect to Temporal server
    let server_url = std::env::var("TEMPORAL_ADDRESS").unwrap_or_else(|_| "localhost:7233".into());
    let namespace = std::env::var("TEMPORAL_NAMESPACE").unwrap_or_else(|_| "default".into());

    tracing::info!(server = %server_url, namespace = %namespace, "connecting to Temporal");

    let client = Client::connect(&server_url, &namespace).await?;

    tracing::info!("connected to Temporal server");

    // Create the worker
    let mut worker = Worker::builder()
        .client(client)
        .task_queue("example-task-queue")
        .activity_handler(
            "greet",
            Arc::new(|ctx, input| Box::pin(greet_activity(ctx, input))),
        )
        .activity_handler(
            "long_running",
            Arc::new(|ctx, input| Box::pin(long_running_activity(ctx, input))),
        )
        .build()?;

    tracing::info!(
        task_queue = "example-task-queue",
        "worker created, starting to poll for activities"
    );

    // Run the worker (blocks until shutdown)
    worker.run().await?;

    Ok(())
}
