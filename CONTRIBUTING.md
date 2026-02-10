# Contributing to Rust Guardian

Thank you for your interest in contributing to Rust Guardian!

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/rust-guardian.git`
3. Create a new branch: `git checkout -b feature/your-feature-name`

## Development Setup

### Prerequisites

- Rust 1.70 or higher
- Cargo (comes with Rust)

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running Locally

```bash
cargo run -- check src/
```

## Code Quality

Before submitting a pull request, make sure your code:

1. Passes all tests: `cargo test`
2. Passes clippy checks: `cargo clippy --all-targets --all-features -- -D warnings`
3. Is properly formatted: `cargo fmt --all -- --check`

## Submitting Changes

1. Commit your changes with clear, descriptive commit messages
2. Push to your fork
3. Create a pull request against the `main` branch
4. Describe your changes in the PR description

## Questions?

If you have questions, please open an issue on GitHub.

## License

By contributing to Rust Guardian, you agree that your contributions will be licensed under the MIT License.
