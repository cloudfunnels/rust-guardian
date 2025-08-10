# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- GitHub Actions workflows for CI/CD and automated publishing
- Automated release process with version verification
- Documentation validation workflows
- Security audit integration in CI
- Code coverage reporting
- Release documentation and guidelines

### Changed
- Improved project structure for crates.io publishing

## [0.1.0] - TBD

### Added
- Initial release of rust-guardian
- Dynamic code quality enforcement for Rust projects
- Pattern-based analysis for placeholder code detection
- CLI tool and library API
- Comprehensive configuration system with YAML support
- Multiple output formats (human, JSON, JUnit, SARIF, GitHub Actions)
- Watch mode for real-time validation
- Parallel processing with intelligent caching
- Architectural boundary enforcement
- Support for custom patterns and rules

### Features
- **Pattern Detection**: TODO comments, unimplemented macros, placeholder code
- **Architecture Enforcement**: Module boundary validation, bounded context integrity
- **Performance**: Async API, parallel processing, file caching
- **Integration**: CI/CD support, multiple output formats
- **Flexibility**: Custom .guardianignore files, configurable severity levels
- **CLI Tools**: Check, watch, validate-config, list-rules, explain commands

[Unreleased]: https://github.com/cloudfunnels/rust-guardian/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/cloudfunnels/rust-guardian/releases/tag/v0.1.0