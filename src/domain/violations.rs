//! Core domain models for code quality violations and validation results
//!
//! Architecture: Rich Domain Models - Violations are entities with behavior, not just data
//! - Violations can classify themselves, suggest fixes, and maintain context
//! - ValidationReport acts as an aggregate root managing collections of violations
//! - Domain events can be generated when patterns are detected or when validation completes

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Severity levels for code quality violations
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational messages and suggestions
    Info,
    /// Warnings that should be addressed but don't block builds
    Warning,
    /// Errors that block commits and fail CI/CD builds
    Error,
}

impl Severity {
    /// Whether this severity level should cause validation to fail
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Error)
    }

    /// Convert to string for display
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// A code quality violation detected during analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    /// Unique identifier for the rule that detected this violation
    pub rule_id: String,
    /// Severity level of this violation
    pub severity: Severity,
    /// File path where the violation was found
    pub file_path: PathBuf,
    /// Line number (1-indexed) where the violation occurs
    pub line_number: Option<u32>,
    /// Column number (1-indexed) where the violation starts
    pub column_number: Option<u32>,
    /// Human-readable description of the violation
    pub message: String,
    /// Source code context around the violation
    pub context: Option<String>,
    /// Suggested fix for the violation (if available)
    pub suggested_fix: Option<String>,
    /// When this violation was detected
    pub detected_at: DateTime<Utc>,
}

impl Violation {
    /// Create a new violation
    pub fn new(
        rule_id: impl Into<String>,
        severity: Severity,
        file_path: PathBuf,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule_id: rule_id.into(),
            severity,
            file_path,
            line_number: None,
            column_number: None,
            message: message.into(),
            context: None,
            suggested_fix: None,
            detected_at: Utc::now(),
        }
    }

    /// Set line and column position
    pub fn with_position(mut self, line: u32, column: u32) -> Self {
        self.line_number = Some(line);
        self.column_number = Some(column);
        self
    }

    /// Add source code context
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Add a suggested fix
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggested_fix = Some(suggestion.into());
        self
    }

    /// Whether this violation is blocking (prevents commits/builds)
    pub fn is_blocking(&self) -> bool {
        self.severity.is_blocking()
    }

    /// Format violation for display
    pub fn format_display(&self) -> String {
        let location = match (self.line_number, self.column_number) {
            (Some(line), Some(col)) => format!(":{line}:{col}"),
            (Some(line), None) => format!(":{line}"),
            _ => String::new(),
        };

        format!(
            "{}{} [{}] {}",
            self.file_path.display(),
            location,
            self.severity.as_str(),
            self.message
        )
    }
}

/// Summary statistics for a validation report
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationSummary {
    /// Total number of files analyzed
    pub total_files: usize,
    /// Number of violations by severity level
    pub violations_by_severity: ViolationCounts,
    /// Total execution time in milliseconds
    pub execution_time_ms: u64,
    /// Timestamp when validation was performed
    pub validated_at: DateTime<Utc>,
}

/// Count of violations by severity level
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViolationCounts {
    pub error: usize,
    pub warning: usize,
    pub info: usize,
}

impl ViolationCounts {
    /// Total number of violations across all severities
    pub fn total(&self) -> usize {
        self.error + self.warning + self.info
    }

    /// Whether there are any blocking violations
    pub fn has_blocking(&self) -> bool {
        self.error > 0
    }

    /// Add a violation to the counts
    pub fn add(&mut self, severity: Severity) {
        match severity {
            Severity::Error => self.error += 1,
            Severity::Warning => self.warning += 1,
            Severity::Info => self.info += 1,
        }
    }
}

/// Complete validation report containing all violations and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// All violations found during validation
    pub violations: Vec<Violation>,
    /// Summary statistics
    pub summary: ValidationSummary,
    /// Configuration used for this validation
    pub config_fingerprint: Option<String>,
}

impl ValidationReport {
    /// Create a new empty validation report
    pub fn new() -> Self {
        Self {
            violations: Vec::new(),
            summary: ValidationSummary {
                validated_at: Utc::now(),
                ..Default::default()
            },
            config_fingerprint: None,
        }
    }

    /// Add a violation to the report
    pub fn add_violation(&mut self, violation: Violation) {
        self.summary.violations_by_severity.add(violation.severity);
        self.violations.push(violation);
    }

    /// Whether the report contains any violations
    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }

    /// Whether the report contains blocking violations (errors)
    pub fn has_errors(&self) -> bool {
        self.summary.violations_by_severity.has_blocking()
    }

    /// Get violations of a specific severity
    pub fn violations_by_severity(&self, severity: Severity) -> impl Iterator<Item = &Violation> {
        self.violations
            .iter()
            .filter(move |v| v.severity == severity)
    }

    /// Set the number of files analyzed
    pub fn set_files_analyzed(&mut self, count: usize) {
        self.summary.total_files = count;
    }

    /// Set the execution time
    pub fn set_execution_time(&mut self, duration_ms: u64) {
        self.summary.execution_time_ms = duration_ms;
    }

    /// Set the configuration fingerprint
    pub fn set_config_fingerprint(&mut self, fingerprint: impl Into<String>) {
        self.config_fingerprint = Some(fingerprint.into());
    }

    /// Merge another report into this one
    pub fn merge(&mut self, other: ValidationReport) {
        for violation in other.violations {
            self.add_violation(violation);
        }
        self.summary.total_files += other.summary.total_files;
    }

    /// Sort violations by file path and line number for consistent output
    pub fn sort_violations(&mut self) {
        self.violations.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then_with(|| a.line_number.unwrap_or(0).cmp(&b.line_number.unwrap_or(0)))
                .then_with(|| a.severity.cmp(&b.severity))
        });
    }
}

impl Default for ValidationReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Error types that can occur during validation
#[derive(Debug, thiserror::Error)]
pub enum GuardianError {
    /// Configuration file could not be loaded or parsed
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    /// File could not be read or accessed
    #[error("IO error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    /// Pattern compilation failed
    #[error("Pattern error: {message}")]
    Pattern { message: String },

    /// Analysis failed for a specific file
    #[error("Analysis error in {file}: {message}")]
    Analysis { file: String, message: String },

    /// Cache operation failed
    #[error("Cache error: {message}")]
    Cache { message: String },

    /// Validation operation failed
    #[error("Validation error: {message}")]
    Validation { message: String },
}

impl GuardianError {
    /// Create a configuration error
    pub fn config(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Create a pattern error
    pub fn pattern(message: impl Into<String>) -> Self {
        Self::Pattern {
            message: message.into(),
        }
    }

    /// Create an analysis error
    pub fn analysis(file: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Analysis {
            file: file.into(),
            message: message.into(),
        }
    }

    /// Create a cache error
    pub fn cache(message: impl Into<String>) -> Self {
        Self::Cache {
            message: message.into(),
        }
    }

    /// Create a validation error
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }
}

/// Result type for Guardian operations
pub type GuardianResult<T> = Result<T, GuardianError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_violation_creation() {
        let violation = Violation::new(
            "test_rule",
            Severity::Error,
            PathBuf::from("src/lib.rs"),
            "Test message",
        );

        assert_eq!(violation.rule_id, "test_rule");
        assert_eq!(violation.severity, Severity::Error);
        assert_eq!(violation.file_path, Path::new("src/lib.rs"));
        assert_eq!(violation.message, "Test message");
        assert!(violation.is_blocking());
    }

    #[test]
    fn test_violation_with_position() {
        let violation = Violation::new(
            "test_rule",
            Severity::Warning,
            PathBuf::from("src/lib.rs"),
            "Test message",
        )
        .with_position(42, 15)
        .with_context("let x = unimplemented!();");

        assert_eq!(violation.line_number, Some(42));
        assert_eq!(violation.column_number, Some(15));
        assert_eq!(
            violation.context,
            Some("let x = unimplemented!();".to_string())
        );
        assert!(!violation.is_blocking());
    }

    #[test]
    fn test_validation_report() {
        let mut report = ValidationReport::new();

        report.add_violation(Violation::new(
            "rule1",
            Severity::Error,
            PathBuf::from("src/main.rs"),
            "Error message",
        ));

        report.add_violation(Violation::new(
            "rule2",
            Severity::Warning,
            PathBuf::from("src/lib.rs"),
            "Warning message",
        ));

        assert!(report.has_violations());
        assert!(report.has_errors());
        assert_eq!(report.summary.violations_by_severity.total(), 2);
        assert_eq!(report.summary.violations_by_severity.error, 1);
        assert_eq!(report.summary.violations_by_severity.warning, 1);
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
        assert!(Severity::Error.is_blocking());
        assert!(!Severity::Warning.is_blocking());
    }
}
