# temporal-rs

[![CI](https://github.com/copyleftdev/temporal-rs/workflows/CI/badge.svg)](https://github.com/copyleftdev/temporal-rs/actions)
[![Crates.io](https://img.shields.io/crates/v/temporal-rs.svg)](https://crates.io/crates/temporal-rs)
[![Documentation](https://docs.rs/temporal-rs/badge.svg)](https://docs.rs/temporal-rs)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

A high-performance, elegant Rust SDK for [Temporal](https://temporal.io) workflow orchestration.

## Features

- 🚀 **High Performance** — Built on Rust for minimal overhead and maximum throughput
- 🎯 **Ergonomic API** — Proc macros for clean workflow and activity definitions
- 🔒 **Type Safe** — Compile-time guarantees for workflow correctness
- 🧪 **Test First** — Built-in support for replay testing and testcontainers
- 📊 **Observable** — First-class tracing and metrics integration

## Quick Start

```rust
use temporal_rs::prelude::*;

#[activity]
async fn greet(ctx: ActivityContext, name: String) -> Result<String, ActivityError> {
    Ok(format!("Hello, {}!", name))
}

#[workflow]
async fn greeting_workflow(ctx: WorkflowContext, name: String) -> Result<String, WorkflowError> {
    ctx.execute_activity(greet, name, ActivityOptions::default()).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::connect("localhost:7233", "default").await?;

    Worker::builder()
        .client(client)
        .task_queue("greeting-queue")
        .workflow(greeting_workflow)
        .activity(greet)
        .build()?
        .run()
        .await
}
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
temporal-rs = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Development Setup

### Prerequisites

- Rust 1.75+ (`rustup update stable`)
- Docker (for integration tests)
- pre-commit (`pip install pre-commit`)

### Setup

```bash
# Clone the repository
git clone https://github.com/copyleftdev/temporal-rs.git
cd temporal-rs

# Install pre-commit hooks
./scripts/setup-hooks.sh

# Build
cargo build

# Run tests
cargo test

# Run integration tests (requires Docker)
cargo test --features testcontainers
```

### Pre-commit Hooks

This project uses pre-commit hooks to ensure code quality. The hooks run:

| Hook | Description |
|------|-------------|
| `rustfmt` | Format code according to style guidelines |
| `cargo check` | Verify code compiles |
| `clippy` | Lint for common mistakes and improvements |
| `cargo test` | Run unit tests |

Hooks run automatically on commit. To run manually:

```bash
pre-commit run --all-files
```

## Project Structure

```
temporal-rs/
├── crates/
│   ├── temporal/           # Main SDK crate
│   ├── temporal-macros/    # Proc macros (#[workflow], #[activity])
│   ├── temporal-core/      # Low-level sdk-core bindings
│   └── temporal-testing/   # Test utilities and containers
├── examples/               # Example applications
└── tests/                  # Integration tests
```

## Roadmap

- [x] Project setup and pre-commit hooks
- [ ] **Phase 1**: Foundation & MVP (Activity support)
- [ ] **Phase 2**: Workflow support (Signals, queries, timers)
- [ ] **Phase 3**: Production features (Interceptors, versioning)
- [ ] **Phase 4**: Polish (Documentation, benchmarks)

See [GitHub Issues](https://github.com/copyleftdev/temporal-rs/issues) for detailed plans.

## Contributing

Contributions are welcome! Please read our contributing guidelines and ensure:

1. All pre-commit hooks pass
2. New code has test coverage
3. Documentation is updated

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
