#!/usr/bin/env bash
# Setup development environment for temporal-rs
# Usage: ./scripts/setup-hooks.sh

set -euo pipefail

echo "🔧 Setting up temporal-rs development environment..."

# Check for required tools
check_command() {
    if ! command -v "$1" &> /dev/null; then
        echo "❌ $1 is not installed. Please install it first."
        exit 1
    fi
    echo "✓ $1 found"
}

echo ""
echo "Checking required tools..."
check_command "cargo"
check_command "rustfmt"
check_command "clippy-driver"

# Check for pre-commit
if ! command -v pre-commit &> /dev/null; then
    echo ""
    echo "⚠️  pre-commit not found. Installing via pip..."
    pip install pre-commit
fi
echo "✓ pre-commit found"

# Install Rust components if missing
echo ""
echo "Ensuring Rust components are installed..."
rustup component add rustfmt clippy

# Install pre-commit hooks
echo ""
echo "Installing pre-commit hooks..."
pre-commit install
pre-commit install --hook-type commit-msg

# Run hooks on all files to verify setup
echo ""
echo "Running hooks on all files to verify setup..."
pre-commit run --all-files || true

echo ""
echo "✅ Development environment setup complete!"
echo ""
echo "Pre-commit hooks will now run automatically on each commit."
echo "To run manually: pre-commit run --all-files"
echo ""
