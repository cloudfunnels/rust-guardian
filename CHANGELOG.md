# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-08-10

### Added
- Initial release of Rust Guardian
- Pattern-based code analysis for detecting placeholder code and TODO comments
- Support for regex patterns, AST patterns, and semantic patterns
- CLI tool with multiple output formats (human, JSON, JUnit, SARIF, GitHub Actions)
- Library API for programmatic code validation
- Configuration via YAML files with flexible path patterns
- .guardianignore file support (gitignore-style)
- Watch mode for real-time validation during development
- Parallel processing with intelligent caching for performance
- Built-in patterns for:
  - TODO/FIXME/HACK comments
  - Unimplemented macros (todo!, unimplemented!, panic!)
  - Empty Ok(()) returns
  - Architectural boundary violations
  - Hardcoded paths
- Severity levels (error, warning, info)
- Integration support for CI/CD pipelines
- Pre-commit hook compatibility
- Comprehensive test coverage
- Documentation and examples

### Changed

### Deprecated

### Removed

### Fixed

### Security

[Unreleased]: https://github.com/cloudfunnels/rust-guardian/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/cloudfunnels/rust-guardian/releases/tag/v0.1.0