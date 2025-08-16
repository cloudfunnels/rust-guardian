# Rust Guardian
Rust Guardian is a self-contained, production-ready Rust crate providing comprehensive code quality analysis for detecting placeholder code, enforcing architectural boundaries, and maintaining code quality standards. It offers both a CLI tool and library API with async support for CI/CD integration.

Always reference these instructions first and fallback to search or bash commands only when you encounter unexpected information that does not match the info here.

## Working Effectively
- Bootstrap, build, and test the repository:
  - Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh && source ~/.cargo/env`
  - Check versions: `rustc --version && cargo --version` (requires Rust 1.70+)
  - Debug build: `cargo build` -- takes 1m 45s to complete. NEVER CANCEL. Set timeout to 120+ seconds.
  - Release build: `cargo build --release` -- takes 1m 5s to complete. NEVER CANCEL. Set timeout to 90+ seconds.
  - Run tests: `cargo test` -- takes 1m 35s to complete. NEVER CANCEL. Set timeout to 120+ seconds.
- Install the CLI tool:
  - From source: `cargo install --path .` -- takes 35s to complete. NEVER CANCEL. Set timeout to 60+ seconds.
  - Verify installation: `rust-guardian --version`
- Run the application:
  - ALWAYS run the bootstrapping steps first (build/install).
  - CLI usage: `rust-guardian check src/` (analyzes source files)
  - Watch mode: `rust-guardian watch src/` (real-time analysis)
  - Config validation: `rust-guardian validate-config guardian.yaml`

## Validation
- Always manually validate any new code by running through complete end-to-end scenarios after making changes.
- ALWAYS run at least one complete workflow after making changes: build → test → install → run on sample code.
- You can build and run the CLI application in both debug and release modes.
- Always run `cargo fmt` and `cargo clippy` before you are done or similar tools may fail in CI.

## Common tasks
The following are outputs from frequently run commands. Reference them instead of viewing, searching, or running bash commands to save time.

### Repo root
```
.git
.gitignore
.guardianignore
CHANGELOG.md
Cargo.lock
Cargo.toml
LICENSE
README.md
examples/
src/
target/ (after build)
.rust/ (cache directory)
```

### Build and test performance
- **Debug build**: ~1m 45s (set timeout: 120+ seconds)
- **Release build**: ~1m 5s (set timeout: 90+ seconds)  
- **Tests**: ~1m 35s (set timeout: 120+ seconds)
- **Install**: ~35s (set timeout: 60+ seconds)
- **Runtime performance**: 20-150ms for typical projects (very fast)
- **Memory usage**: ~50-100MB peak

### Key CLI commands validated to work
```bash
# Basic analysis
rust-guardian check src/                    # Analyze source directory
rust-guardian check . --format json         # JSON output for CI/CD
rust-guardian check . --severity error      # Show only errors

# Configuration and rules
rust-guardian validate-config guardian.yaml # Validate config file
rust-guardian rules                         # List all available rules
rust-guardian explain todo_comments         # Explain specific rule

# Performance and caching
rust-guardian check . --cache               # Enable caching
rust-guardian cache stats                   # Show cache statistics
rust-guardian cache clear                   # Clear cache

# Watch mode for development
rust-guardian watch src/                    # Real-time analysis

# Different output formats
rust-guardian check . --format human        # Human-readable (default)
rust-guardian check . --format json         # Machine-readable JSON
rust-guardian check . --format junit        # JUnit XML for CI
rust-guardian check . --format github       # GitHub Actions format
```

### Sample analysis results
When run on a typical Rust project with placeholder code:
- **Execution time**: 20-150ms for small-medium projects
- **Common violations found**: TODO comments, unimplemented!() macros, empty Ok(()) returns
- **Exit codes**: 0 = success, 1 = violations found
- **File support**: .rs files, respects .gitignore and .guardianignore patterns

### Configuration structure
Default configuration includes:
- **Placeholders**: TODO/FIXME/HACK comments, unimplemented!/todo!/panic! macros
- **Incomplete implementations**: Empty Ok(()) returns 
- **Architectural violations**: Hardcoded paths, architectural header missing
- **Severity levels**: error (fails CI), warning (informational), info (suggestions)
- **Path filtering**: Supports .gitignore-style patterns with include/exclude

### Common validation scenarios
After making changes, always test these scenarios:
1. **Basic functionality**: `rust-guardian check src/` should analyze files and report violations
2. **Clean code**: Run on clean code should return minimal/no violations
3. **Problematic code**: Run on code with TODO comments should detect them
4. **Performance**: Release mode should complete analysis in under 1 second for small projects
5. **Configuration**: `rust-guardian validate-config` should validate YAML syntax
6. **Help system**: `rust-guardian --help` and `rust-guardian rules` should work

### Project structure
- **src/main.rs**: CLI application entry point
- **src/lib.rs**: Library API for programmatic use
- **src/analyzer/**: Core analysis engine with Rust AST parsing
- **src/patterns/**: Pattern matching for different rule types (regex, AST, semantic)
- **src/config/**: Configuration loading and validation
- **src/report/**: Output formatting (human, JSON, JUnit, etc.)
- **src/cache/**: File caching for performance
- **examples/guardian.yaml**: Example configuration file
- **Cargo.toml**: Dependencies include syn, regex, clap, tokio, rayon

### Dependencies and requirements
- **Rust**: 1.70+ (tested with 1.88.0)
- **Key dependencies**: syn (AST parsing), regex (pattern matching), clap (CLI), tokio (async), rayon (parallelism)
- **Development dependencies**: tempfile, criterion, rstest, tokio-test
- **Features**: cli (default), cache, colors, full
- **No external system dependencies** - self-contained Rust application

### Sample workflow for testing changes
```bash
# Complete validation workflow (NEVER CANCEL any of these commands)
cargo build                                 # Build (1m 45s)
cargo test                                  # Test (1m 35s)  
cargo build --release                       # Release build (1m 5s)
cargo install --path .                      # Install (35s)

# Test on sample problematic code
echo '// TODO: implement this
fn main() { 
    todo!(); 
}' > /tmp/test.rs

rust-guardian check /tmp/test.rs           # Should find 2 violations
rust-guardian check /tmp/test.rs --format json | jq '.summary'

# Test on clean code  
echo 'fn main() { println!("Hello!"); }' > /tmp/clean.rs
rust-guardian check /tmp/clean.rs          # Should find minimal violations

# Performance validation
time rust-guardian check src/              # Should complete quickly (<1s)
```

### Build troubleshooting
- **Rust not found**: Install with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Build timeout**: NEVER CANCEL - builds take 1-2 minutes, set timeout to 120+ seconds
- **Test failures**: All 55 tests should pass (51 unit + 4 CLI tests)
- **Performance issues**: Use `--release` build for better performance
- **Binary not found**: After `cargo install --path .`, binary is at `~/.cargo/bin/rust-guardian`

### CI/CD integration examples
```bash
# GitHub Actions
rust-guardian check --format github --severity error >> $GITHUB_STEP_SUMMARY

# GitLab CI  
rust-guardian check --format junit --severity error > guardian-report.xml

# General CI
rust-guardian check . --format json --severity error || exit 1
```

### NEVER CANCEL commands - Required timeouts
- `cargo build`: Set timeout to 120+ seconds (takes ~1m 45s)
- `cargo build --release`: Set timeout to 90+ seconds (takes ~1m 5s)
- `cargo test`: Set timeout to 120+ seconds (takes ~1m 35s)
- `cargo install --path .`: Set timeout to 60+ seconds (takes ~35s)

### Quick reference for development
```bash
# Development cycle
cargo check              # Fast syntax check (~10s)
cargo test               # Run tests (1m 35s) - NEVER CANCEL
cargo clippy             # Lint code (~20s)
cargo fmt                # Format code (~5s)

# Testing the tool
./target/release/rust-guardian check src/     # Test on own codebase
./target/debug/rust-guardian --help           # Verify CLI works
rust-guardian rules                           # List available rules
```