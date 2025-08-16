# Release Process

This document describes the automated release process for rust-guardian.

## Overview

The project uses GitHub Actions to automate the entire release process, including:
- Pre-release validation (tests, linting, formatting)
- Publishing to crates.io
- Creating GitHub releases with binaries

## Setup Requirements

Before releases can be automated, the following secrets must be configured in the GitHub repository:

### Required Secrets

1. **CRATES_TOKEN**: A crates.io API token for publishing
   - Go to https://crates.io/me
   - Generate a new token with `publish-update` scope
   - Add it as a repository secret named `CRATES_TOKEN`

### Automatic Setup

- **GITHUB_TOKEN**: Automatically provided by GitHub Actions (no setup required)

## Release Process

### 1. Prepare for Release

1. Update the version in `Cargo.toml`:
   ```toml
   version = "0.2.0"  # Update this
   ```

2. Update the changelog/documentation if needed

3. Commit and push changes:
   ```bash
   git add Cargo.toml
   git commit -m "Bump version to 0.2.0"
   git push origin main
   ```

### 2. Create Release

1. Create and push a version tag:
   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

2. The GitHub Actions workflow will automatically:
   - Run all tests and quality checks
   - Verify the tag version matches `Cargo.toml`
   - Build the project with all features
   - Publish to crates.io
   - Create a GitHub release with:
     - Release notes from git commits
     - Binary artifacts
     - Checksums

### 3. Post-Release

After the automated release:

1. **crates.io**: The crate will be available at https://crates.io/crates/rust-guardian
2. **docs.rs**: Documentation will be automatically built at https://docs.rs/rust-guardian
3. **GitHub**: Release will be available with downloadable binaries

## Workflow Details

### CI Workflow (`.github/workflows/ci.yml`)

Runs on every push and pull request:
- Tests on stable, beta, and nightly Rust
- Code formatting checks (`cargo fmt`)
- Linting (`cargo clippy`)
- Security audit (`cargo audit`)
- Coverage reporting
- CLI functionality tests

### Release Workflow (`.github/workflows/release.yml`)

Triggered on version tags (e.g., `v0.2.0`):
- Pre-release validation (same as CI)
- Version verification
- crates.io publishing
- GitHub release creation
- Binary artifact generation

### Documentation Workflow (`.github/workflows/docs.yml`)

Runs on documentation changes:
- Documentation tests (`cargo test --doc`)
- Link checking in README
- Package metadata validation

## Troubleshooting

### Failed Release

If a release fails:

1. Check the GitHub Actions logs for specific errors
2. Common issues:
   - Version mismatch between tag and `Cargo.toml`
   - Missing or invalid `CRATES_TOKEN`
   - Test failures
   - Formatting issues

### Fixing a Failed Release

1. Fix the underlying issue
2. Delete the failed tag (if needed):
   ```bash
   git tag -d v0.2.0
   git push origin :refs/tags/v0.2.0
   ```
3. Create the tag again after fixes

### Manual Release

If automation fails and manual release is needed:

```bash
# Ensure you're on the correct commit
git checkout v0.2.0

# Build and test
cargo build --release --all-features
cargo test --all-features

# Publish to crates.io
cargo publish --token YOUR_CRATES_TOKEN
```

## Version Strategy

This project follows [Semantic Versioning](https://semver.org/):

- **MAJOR** version: Incompatible API changes
- **MINOR** version: Backward-compatible functionality additions
- **PATCH** version: Backward-compatible bug fixes

Pre-release versions can use suffixes:
- `1.0.0-alpha.1`: Alpha release
- `1.0.0-beta.1`: Beta release
- `1.0.0-rc.1`: Release candidate

## Crates.io Configuration

The crate is configured for optimal crates.io and docs.rs integration:

```toml
[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]
```

This ensures:
- All features are documented on docs.rs
- Documentation builds with the `docsrs` cfg flag for conditional compilation
- Examples and integration tests are properly documented