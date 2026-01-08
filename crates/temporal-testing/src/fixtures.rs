//! Test fixtures and helpers.

use std::time::Duration;

/// Default namespace for testing.
pub const TEST_NAMESPACE: &str = "default";

/// Default task queue for testing.
pub const TEST_TASK_QUEUE: &str = "test-task-queue";

/// Default timeout for test operations.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Generate a unique task queue name for a test.
#[must_use]
pub fn unique_task_queue(prefix: &str) -> String {
    let suffix: String = (0..6)
        .map(|_| fastrand::alphanumeric())
        .collect();
    format!("{}-{}", prefix, suffix)
}

/// Generate a unique workflow ID for a test.
#[must_use]
pub fn unique_workflow_id(prefix: &str) -> String {
    let suffix: String = (0..8)
        .map(|_| fastrand::alphanumeric())
        .collect();
    format!("{}-{}", prefix, suffix)
}
