//! Report generation with multiple output formats
//!
//! Architecture: Anti-Corruption Layer - Formatters translate domain objects to external formats
//! - ValidationReport (domain) is converted to various external representations
//! - Each formatter encapsulates the rules for its specific output format
//! - Domain logic remains pure while supporting multiple presentation needs

use crate::domain::violations::{GuardianResult, Severity, ValidationReport, Violation};
use serde_json::Value as JsonValue;
use std::io::Write;

/// Supported output formats for validation reports
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable format with colors and context
    Human,
    /// JSON format for programmatic consumption
    Json,
    /// JUnit XML format for CI/CD integration
    Junit,
    /// SARIF format for security tools
    Sarif,
    /// GitHub Actions format for workflow integration
    GitHub,
    /// Agent-friendly format for easy parsing: [line:path] <violation>
    Agent,
}

use std::str::FromStr;

impl FromStr for OutputFormat {
    type Err = String;

    /// Parse format from string
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "human" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            "junit" => Ok(Self::Junit),
            "sarif" => Ok(Self::Sarif),
            "github" => Ok(Self::GitHub),
            "agent" => Ok(Self::Agent),
            _ => Err(format!("Unknown output format: {s}")),
        }
    }
}

impl OutputFormat {
    /// Get all available format names
    pub fn all_formats() -> &'static [&'static str] {
        &["human", "json", "junit", "sarif", "github", "agent"]
    }

    /// Validate that this format is appropriate for the given context
    ///
    /// Architecture Principle: Value objects should validate their domain rules
    pub fn validate_for_context(&self, is_ci_environment: bool) -> GuardianResult<()> {
        match (self, is_ci_environment) {
            (Self::Human, true) => {
                // Human format in CI might not render colors properly
                // This is a warning, not an error
                Ok(())
            }
            (Self::Junit | Self::GitHub | Self::Sarif, false) => {
                // CI formats in interactive use are less optimal but valid
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Check if this format supports colored output
    pub fn supports_colors(&self) -> bool {
        matches!(self, Self::Human)
    }

    /// Check if this format produces structured data
    pub fn is_structured(&self) -> bool {
        matches!(self, Self::Json | Self::Sarif | Self::Junit)
    }
}

/// Options for customizing report output
#[derive(Debug, Clone)]
pub struct ReportOptions {
    /// Whether to use colored output (for human format)
    pub use_colors: bool,
    /// Whether to show context lines around violations
    pub show_context: bool,
    /// Whether to show violation suggestions
    pub show_suggestions: bool,
    /// Maximum number of violations to include
    pub max_violations: Option<usize>,
    /// Minimum severity level to include
    pub min_severity: Option<Severity>,
}

impl Default for ReportOptions {
    fn default() -> Self {
        Self {
            use_colors: true,
            show_context: true,
            show_suggestions: true,
            max_violations: None,
            min_severity: None,
        }
    }
}

impl ReportOptions {
    /// Validate options consistency
    ///
    /// Architecture Principle: Domain objects validate their own invariants
    pub fn validate(&self) -> GuardianResult<()> {
        // Validate maximum violations setting
        if let Some(max) = self.max_violations {
            if max == 0 {
                return Err(crate::domain::violations::GuardianError::config(
                    "max_violations cannot be zero - this would produce empty reports",
                ));
            }
            if max > 10000 {
                return Err(crate::domain::violations::GuardianError::config(
                    "max_violations too high - consider using severity filtering instead",
                ));
            }
        }

        // Validate severity consistency
        if let Some(min_severity) = self.min_severity {
            if min_severity > Severity::Error {
                return Err(crate::domain::violations::GuardianError::config(
                    "min_severity cannot be higher than Error",
                ));
            }
        }

        Ok(())
    }

    /// Check if these options are optimized for the given format
    pub fn is_optimized_for(&self, format: OutputFormat) -> bool {
        match format {
            OutputFormat::Human => true, // Human format supports all options
            OutputFormat::Json | OutputFormat::Sarif => {
                // Structured formats don't use colors or context display
                !self.use_colors && !self.show_context
            }
            OutputFormat::Junit => {
                // JUnit mainly cares about violations, not display options
                !self.use_colors
            }
            OutputFormat::GitHub => {
                // GitHub format doesn't use colors or suggestions
                !self.use_colors && !self.show_suggestions
            }
            OutputFormat::Agent => {
                // Agent format is minimal
                !self.use_colors && !self.show_context && !self.show_suggestions
            }
        }
    }

    /// Create optimized options for a specific format
    pub fn optimized_for(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Human => Self::default(),
            OutputFormat::Json | OutputFormat::Sarif => Self {
                use_colors: false,
                show_context: false,
                show_suggestions: false,
                ..Self::default()
            },
            OutputFormat::Junit => Self {
                use_colors: false,
                show_suggestions: false,
                ..Self::default()
            },
            OutputFormat::GitHub => Self {
                use_colors: false,
                show_suggestions: false,
                ..Self::default()
            },
            OutputFormat::Agent => Self {
                use_colors: false,
                show_context: false,
                show_suggestions: false,
                ..Self::default()
            },
        }
    }
}

/// Main report formatter that dispatches to specific formatters
pub struct ReportFormatter {
    options: ReportOptions,
}

impl ReportFormatter {
    /// Create a new report formatter with options
    ///
    /// Architecture Principle: Constructor validates domain invariants
    pub fn new(options: ReportOptions) -> GuardianResult<Self> {
        options.validate()?;
        Ok(Self { options })
    }

    /// Create a formatter with validated options, panicking on invalid configuration
    ///
    /// For use in contexts where configuration errors are programming errors
    pub fn with_options(options: ReportOptions) -> Self {
        options
            .validate()
            .expect("ReportOptions validation failed - this indicates a programming error");
        Self { options }
    }

    /// Validate formatter configuration and capabilities
    ///
    /// Architecture Principle: Domain models should validate their own consistency
    /// This method ensures the formatter can fulfill its interface contract
    pub fn validate_capabilities(&self) -> GuardianResult<()> {
        // Validate color support when colors are enabled
        if self.options.use_colors && !Self::supports_ansi_colors() {
            return Err(crate::domain::violations::GuardianError::config(
                "Color output requested but terminal does not support ANSI colors",
            ));
        }

        // Validate severity filtering configuration
        if let Some(min_severity) = self.options.min_severity {
            if min_severity > Severity::Error {
                return Err(crate::domain::violations::GuardianError::config(
                    "Minimum severity cannot be higher than Error",
                ));
            }
        }

        // Validate violation limits
        if let Some(max) = self.options.max_violations {
            if max == 0 {
                return Err(crate::domain::violations::GuardianError::config(
                    "Maximum violations cannot be zero - use filtering instead",
                ));
            }
        }

        Ok(())
    }

    /// Check if the current environment supports ANSI color codes
    fn supports_ansi_colors() -> bool {
        // Check if colors are explicitly disabled
        if std::env::var("NO_COLOR").is_ok() {
            return false;
        }

        // GitHub Actions and other CI systems support ANSI colors
        if std::env::var("GITHUB_ACTIONS").is_ok() || std::env::var("CI").is_ok() {
            return true;
        }

        // Check terminal capabilities
        std::env::var("TERM").is_ok_and(|term| term != "dumb")
    }

    /// Validate that a format operation produces expected structure
    ///
    /// Architecture Principle: Anti-corruption layer should validate its transformations
    pub fn validate_format_integrity(
        &self,
        report: &ValidationReport,
        format: OutputFormat,
        output: &str,
    ) -> GuardianResult<()> {
        match format {
            OutputFormat::Json => self.validate_json_structure(output),
            OutputFormat::Junit => self.validate_junit_structure(output),
            OutputFormat::Sarif => self.validate_sarif_structure(output),
            OutputFormat::Human | OutputFormat::GitHub | OutputFormat::Agent => {
                // Text formats have basic structure validation
                if output.is_empty() && !report.violations.is_empty() {
                    return Err(crate::domain::violations::GuardianError::config(
                        "Non-empty report produced empty output",
                    ));
                }
                Ok(())
            }
        }
    }

    /// Validate JSON output structure
    fn validate_json_structure(&self, output: &str) -> GuardianResult<()> {
        let json: JsonValue = serde_json::from_str(output).map_err(|e| {
            crate::domain::violations::GuardianError::config(format!("Invalid JSON structure: {e}"))
        })?;

        // Ensure required fields exist
        if json.get("violations").is_none() {
            return Err(crate::domain::violations::GuardianError::config(
                "JSON output missing required 'violations' field",
            ));
        }

        if json.get("summary").is_none() {
            return Err(crate::domain::violations::GuardianError::config(
                "JSON output missing required 'summary' field",
            ));
        }

        Ok(())
    }

    /// Validate JUnit XML structure
    fn validate_junit_structure(&self, output: &str) -> GuardianResult<()> {
        if !output.starts_with("<?xml version=\"1.0\"") {
            return Err(crate::domain::violations::GuardianError::config(
                "JUnit output must start with XML declaration",
            ));
        }

        if !output.contains("<testsuite") {
            return Err(crate::domain::violations::GuardianError::config(
                "JUnit output must contain testsuite element",
            ));
        }

        Ok(())
    }

    /// Validate SARIF structure
    fn validate_sarif_structure(&self, output: &str) -> GuardianResult<()> {
        let json: JsonValue = serde_json::from_str(output).map_err(|e| {
            crate::domain::violations::GuardianError::config(format!("Invalid SARIF JSON: {e}"))
        })?;

        if json.get("version").and_then(|v| v.as_str()) != Some("2.1.0") {
            return Err(crate::domain::violations::GuardianError::config(
                "SARIF output must specify version 2.1.0",
            ));
        }

        if json
            .get("runs")
            .and_then(|r| r.as_array())
            .is_none_or(|arr| arr.is_empty())
        {
            return Err(crate::domain::violations::GuardianError::config(
                "SARIF output must contain at least one run",
            ));
        }

        Ok(())
    }

    /// Format a validation report in the specified format
    ///
    /// Architecture Principle: Domain services orchestrate self-validating behavior
    /// This method ensures each transformation maintains domain integrity
    pub fn format_report(
        &self,
        report: &ValidationReport,
        format: OutputFormat,
    ) -> GuardianResult<String> {
        // Validate capabilities before processing
        self.validate_capabilities()?;

        // Filter violations based on options
        let filtered_violations = self.filter_violations(&report.violations);

        let output = match format {
            OutputFormat::Human => self.format_human(report, &filtered_violations),
            OutputFormat::Json => self.format_json(report, &filtered_violations),
            OutputFormat::Junit => self.format_junit(report, &filtered_violations),
            OutputFormat::Sarif => self.format_sarif(report, &filtered_violations),
            OutputFormat::GitHub => self.format_github(report, &filtered_violations),
            OutputFormat::Agent => self.format_agent(report, &filtered_violations),
        }?;

        // Validate output integrity before returning
        self.validate_format_integrity(report, format, &output)?;

        Ok(output)
    }

    /// Write a formatted report to a writer
    pub fn write_report<W: Write>(
        &self,
        report: &ValidationReport,
        format: OutputFormat,
        mut writer: W,
    ) -> GuardianResult<()> {
        let formatted = self.format_report(report, format)?;
        writer
            .write_all(formatted.as_bytes())
            .map_err(|e| crate::domain::violations::GuardianError::Io { source: e })?;
        Ok(())
    }

    /// Filter violations based on report options
    fn filter_violations<'a>(&self, violations: &'a [Violation]) -> Vec<&'a Violation> {
        let mut filtered: Vec<&Violation> = violations
            .iter()
            .filter(|v| {
                // Filter by minimum severity
                if let Some(min_severity) = self.options.min_severity {
                    if v.severity < min_severity {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Limit number of violations if requested
        if let Some(max) = self.options.max_violations {
            filtered.truncate(max);
        }

        filtered
    }

    /// Format report in human-readable format
    fn format_human(
        &self,
        report: &ValidationReport,
        violations: &[&Violation],
    ) -> GuardianResult<String> {
        let mut output = String::new();

        if violations.is_empty() {
            if self.options.use_colors {
                output.push_str("✅ \x1b[32mNo code quality violations found\x1b[0m\n");
            } else {
                output.push_str("✅ No code quality violations found\n");
            }
        } else {
            // Header
            let icon = if report.has_errors() { "❌" } else { "⚠️" };
            if self.options.use_colors {
                let color = if report.has_errors() { "31" } else { "33" };
                output.push_str(&format!(
                    "{icon} \x1b[{color}mCode Quality Violations Found\x1b[0m\n\n"
                ));
            } else {
                output.push_str(&format!("{icon} Code Quality Violations Found\n\n"));
            }

            // Group violations by file
            let mut by_file: std::collections::BTreeMap<&std::path::Path, Vec<&Violation>> =
                std::collections::BTreeMap::new();

            for violation in violations {
                by_file
                    .entry(&violation.file_path)
                    .or_default()
                    .push(violation);
            }

            // Display each file's violations
            for (file_path, file_violations) in by_file {
                output.push_str(&format!("📁 {}\n", file_path.display()));

                for violation in file_violations {
                    // Format violation with colors
                    let severity_color = match violation.severity {
                        Severity::Error => "31",   // Red
                        Severity::Warning => "33", // Yellow
                        Severity::Info => "36",    // Cyan
                    };

                    let position = match (violation.line_number, violation.column_number) {
                        (Some(line), Some(col)) => format!("{line}:{col}"),
                        (Some(line), None) => line.to_string(),
                        _ => "?".to_string(),
                    };

                    if self.options.use_colors {
                        output.push_str(&format!(
                            "  \x1b[{}m{}:{}\x1b[0m [\x1b[{}m{}\x1b[0m] {}\n",
                            "2", // Dim
                            position,
                            violation.rule_id,
                            severity_color,
                            violation.severity.as_str(),
                            violation.message
                        ));
                    } else {
                        output.push_str(&format!(
                            "  {}:{} [{}] {}\n",
                            position,
                            violation.rule_id,
                            violation.severity.as_str(),
                            violation.message
                        ));
                    }

                    // Show context if available and requested
                    if self.options.show_context {
                        if let Some(context) = &violation.context {
                            if self.options.use_colors {
                                output.push_str(&format!("    \x1b[2m│ {context}\x1b[0m\n"));
                            } else {
                                output.push_str(&format!("    │ {context}\n"));
                            }
                        }
                    }

                    // Show suggestions if available and requested
                    if self.options.show_suggestions {
                        if let Some(suggestion) = &violation.suggested_fix {
                            if self.options.use_colors {
                                output.push_str(&format!("    \x1b[32m💡 {suggestion}\x1b[0m\n"));
                            } else {
                                output.push_str(&format!("    💡 {suggestion}\n"));
                            }
                        }
                    }

                    output.push('\n');
                }
            }
        }

        // Summary
        output.push_str(&self.format_summary(report));

        Ok(output)
    }

    /// Format report in JSON format
    fn format_json(
        &self,
        report: &ValidationReport,
        violations: &[&Violation],
    ) -> GuardianResult<String> {
        let json_violations: Vec<JsonValue> = violations
            .iter()
            .map(|v| {
                serde_json::json!({
                    "rule_id": v.rule_id,
                    "severity": v.severity.as_str(),
                    "file_path": v.file_path.display().to_string(),
                    "line_number": v.line_number,
                    "column_number": v.column_number,
                    "message": v.message,
                    "context": v.context,
                    "suggested_fix": v.suggested_fix,
                    "detected_at": v.detected_at.to_rfc3339()
                })
            })
            .collect();

        let json_report = serde_json::json!({
            "violations": json_violations,
            "summary": {
                "total_files": report.summary.total_files,
                "violations_by_severity": {
                    "error": report.summary.violations_by_severity.error,
                    "warning": report.summary.violations_by_severity.warning,
                    "info": report.summary.violations_by_severity.info
                },
                "execution_time_ms": report.summary.execution_time_ms,
                "validated_at": report.summary.validated_at.to_rfc3339()
            },
            "config_fingerprint": report.config_fingerprint
        });

        serde_json::to_string_pretty(&json_report).map_err(|e| {
            crate::domain::violations::GuardianError::config(format!(
                "JSON serialization failed: {e}"
            ))
        })
    }

    /// Format report in JUnit XML format
    fn format_junit(
        &self,
        report: &ValidationReport,
        violations: &[&Violation],
    ) -> GuardianResult<String> {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

        let total_tests = violations.len();
        let failures = violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count();
        let errors = 0; // Currently using failures count for both failures and errors
        let execution_time = (report.summary.execution_time_ms as f64) / 1000.0;

        xml.push_str(&format!(
            "<testsuite name=\"rust-guardian\" tests=\"{total_tests}\" failures=\"{failures}\" errors=\"{errors}\" time=\"{execution_time:.3}\">\n"
        ));

        for violation in violations {
            xml.push_str(&format!(
                "  <testcase classname=\"{}\" name=\"{}\">\n",
                violation.rule_id,
                escape_xml(&violation.file_path.display().to_string())
            ));

            if violation.severity == Severity::Error {
                xml.push_str(&format!(
                    "    <failure message=\"{}\">\n",
                    escape_xml(&violation.message)
                ));
                xml.push_str(&format!(
                    "      File: {}:{}:{}\n",
                    violation.file_path.display(),
                    violation.line_number.unwrap_or(0),
                    violation.column_number.unwrap_or(0)
                ));
                if let Some(context) = &violation.context {
                    xml.push_str(&format!("      Context: {}\n", escape_xml(context)));
                }
                xml.push_str("    </failure>\n");
            }

            xml.push_str("  </testcase>\n");
        }

        xml.push_str("</testsuite>\n");
        Ok(xml)
    }

    /// Format report in SARIF format
    fn format_sarif(
        &self,
        _report: &ValidationReport,
        violations: &[&Violation],
    ) -> GuardianResult<String> {
        let sarif_results: Vec<JsonValue> = violations
            .iter()
            .map(|v| {
                let level = match v.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "note",
                };

                serde_json::json!({
                    "ruleId": v.rule_id,
                    "level": level,
                    "message": {
                        "text": v.message
                    },
                    "locations": [{
                        "physicalLocation": {
                            "artifactLocation": {
                                "uri": v.file_path.display().to_string()
                            },
                            "region": {
                                "startLine": v.line_number.unwrap_or(1),
                                "startColumn": v.column_number.unwrap_or(1)
                            },
                            "contextRegion": v.context.as_ref().map(|c| serde_json::json!({
                                "snippet": {
                                    "text": c
                                }
                            }))
                        }
                    }]
                })
            })
            .collect();

        let sarif_report = serde_json::json!({
            "version": "2.1.0",
            "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
            "runs": [{
                "tool": {
                    "driver": {
                        "name": "rust-guardian",
                        "version": "0.1.1",
                        "informationUri": "https://github.com/cloudfunnels/rust-guardian"
                    }
                },
                "results": sarif_results
            }]
        });

        serde_json::to_string_pretty(&sarif_report).map_err(|e| {
            crate::domain::violations::GuardianError::config(format!(
                "SARIF serialization failed: {e}"
            ))
        })
    }

    /// Format report for GitHub Actions
    fn format_github(
        &self,
        _report: &ValidationReport,
        violations: &[&Violation],
    ) -> GuardianResult<String> {
        let mut output = String::new();

        for violation in violations {
            let level = match violation.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "notice",
            };

            let position = match (violation.line_number, violation.column_number) {
                (Some(line), Some(col)) => format!("line={line},col={col}"),
                (Some(line), None) => format!("line={line}"),
                _ => String::new(),
            };

            let position_part = if position.is_empty() {
                String::new()
            } else {
                format!(" {position}")
            };

            output.push_str(&format!(
                "::{} file={},title={}{}::{}\n",
                level,
                violation.file_path.display(),
                violation.rule_id,
                position_part,
                violation.message
            ));
        }

        Ok(output)
    }

    /// Format report for agent consumption: [line:path] <violation>
    fn format_agent(
        &self,
        _report: &ValidationReport,
        violations: &[&Violation],
    ) -> GuardianResult<String> {
        let mut output = String::new();

        for violation in violations {
            let line_number = violation.line_number.unwrap_or(1);
            let path = violation.file_path.display();

            output.push_str(&format!(
                "[{}:{}]\n{}\n\n",
                line_number, path, violation.message
            ));
        }

        Ok(output)
    }

    /// Format the summary section
    fn format_summary(&self, report: &ValidationReport) -> String {
        let mut summary = String::new();

        let total_violations = report.summary.violations_by_severity.total();
        let execution_time = (report.summary.execution_time_ms as f64) / 1000.0;

        if self.options.use_colors {
            summary.push_str("📊 \x1b[1mSummary:\x1b[0m ");
        } else {
            summary.push_str("📊 Summary: ");
        }

        if total_violations == 0 {
            if self.options.use_colors {
                summary.push_str(&format!(
                    "\x1b[32m0 violations\x1b[0m in {} files ({:.1}s)\n",
                    report.summary.total_files, execution_time
                ));
            } else {
                summary.push_str(&format!(
                    "0 violations in {} files ({:.1}s)\n",
                    report.summary.total_files, execution_time
                ));
            }
        } else {
            let mut parts = Vec::new();

            if report.summary.violations_by_severity.error > 0 {
                let text = format!(
                    "{} error{}",
                    report.summary.violations_by_severity.error,
                    if report.summary.violations_by_severity.error == 1 {
                        ""
                    } else {
                        "s"
                    }
                );
                if self.options.use_colors {
                    parts.push(format!("\x1b[31m{text}\x1b[0m"));
                } else {
                    parts.push(text);
                }
            }

            if report.summary.violations_by_severity.warning > 0 {
                let text = format!(
                    "{} warning{}",
                    report.summary.violations_by_severity.warning,
                    if report.summary.violations_by_severity.warning == 1 {
                        ""
                    } else {
                        "s"
                    }
                );
                if self.options.use_colors {
                    parts.push(format!("\x1b[33m{text}\x1b[0m"));
                } else {
                    parts.push(text);
                }
            }

            if report.summary.violations_by_severity.info > 0 {
                let text = format!("{} info", report.summary.violations_by_severity.info);
                if self.options.use_colors {
                    parts.push(format!("\x1b[36m{text}\x1b[0m"));
                } else {
                    parts.push(text);
                }
            }

            summary.push_str(&format!(
                "{} in {} files ({:.1}s)\n",
                parts.join(", "),
                report.summary.total_files,
                execution_time
            ));
        }

        summary
    }
}

impl Default for ReportFormatter {
    /// Create a formatter with default options
    fn default() -> Self {
        Self::with_options(ReportOptions::default())
    }
}

/// Escape XML special characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    // Test imports - unused

    fn create_test_report() -> ValidationReport {
        let mut report = ValidationReport::new();

        report.add_violation(
            crate::domain::violations::Violation::new(
                "test_rule",
                Severity::Error,
                PathBuf::from("src/main.rs"),
                "Test violation",
            )
            .with_position(42, 15)
            .with_context("let x = unimplemented!();"),
        );

        report.set_files_analyzed(10);
        report.set_execution_time(1200);

        report
    }

    #[test]
    fn test_human_format() {
        let options = ReportOptions {
            use_colors: false,
            ..Default::default()
        };

        // Demonstrate self-validation
        options.validate().expect("Test options should be valid");

        let formatter = ReportFormatter::with_options(options);

        let report = create_test_report();
        let output = formatter
            .format_report(&report, OutputFormat::Human)
            .expect("Human format should always succeed for valid reports");

        // Verify the formatter self-validated the output
        formatter
            .validate_format_integrity(&report, OutputFormat::Human, &output)
            .expect("Human output should pass integrity validation");

        assert!(output.contains("Code Quality Violations Found"));
        assert!(output.contains("src/main.rs"));
        assert!(output.contains("Test violation"));
        assert!(output.contains("Summary:"));
    }

    #[test]
    fn test_json_format() {
        let formatter = ReportFormatter::default();
        let report = create_test_report();
        let output = formatter
            .format_report(&report, OutputFormat::Json)
            .expect("JSON format should always succeed for valid reports");

        // Verify the formatter self-validated the JSON structure
        formatter
            .validate_format_integrity(&report, OutputFormat::Json, &output)
            .expect("JSON output should pass integrity validation");

        let json: JsonValue =
            serde_json::from_str(&output).expect("JSON output should be valid JSON");
        assert!(json["violations"].is_array());
        assert_eq!(
            json["violations"]
                .as_array()
                .expect("violations should be an array")
                .len(),
            1
        );
        assert_eq!(json["violations"][0]["rule_id"], "test_rule");
        assert_eq!(json["summary"]["total_files"], 10);
    }

    #[test]
    fn test_junit_format() {
        let formatter = ReportFormatter::default();
        let report = create_test_report();
        let output = formatter
            .format_report(&report, OutputFormat::Junit)
            .expect("JUnit format should always succeed for valid reports");

        // Verify the formatter self-validated the JUnit structure
        formatter
            .validate_format_integrity(&report, OutputFormat::Junit, &output)
            .expect("JUnit output should pass integrity validation");

        assert!(output.contains("<?xml version=\"1.0\""));
        assert!(output.contains("<testsuite"));
        assert!(output.contains("test_rule"));
        assert!(output.contains("<failure"));
    }

    #[test]
    fn test_github_format() {
        let formatter = ReportFormatter::default();
        let report = create_test_report();
        let output = formatter
            .format_report(&report, OutputFormat::GitHub)
            .expect("GitHub format should always succeed for valid reports");

        // Verify the formatter self-validated the GitHub output
        formatter
            .validate_format_integrity(&report, OutputFormat::GitHub, &output)
            .expect("GitHub output should pass integrity validation");

        assert!(output.contains("::error"));
        assert!(output.contains("file=src/main.rs"));
        assert!(output.contains("line=42,col=15"));
        assert!(output.contains("Test violation"));
    }

    #[test]
    fn test_empty_report() {
        let options = ReportOptions {
            use_colors: false,
            ..Default::default()
        };

        // Demonstrate self-validation
        options.validate().expect("Test options should be valid");

        let formatter = ReportFormatter::with_options(options);

        let report = ValidationReport::new();
        let output = formatter
            .format_report(&report, OutputFormat::Human)
            .expect("Human format should always succeed for empty reports");

        // Verify the formatter self-validated the output
        formatter
            .validate_format_integrity(&report, OutputFormat::Human, &output)
            .expect("Empty report output should pass integrity validation");

        assert!(output.contains("No code quality violations found"));
    }

    #[test]
    fn test_severity_filtering() {
        let options = ReportOptions {
            min_severity: Some(Severity::Error),
            ..Default::default()
        };

        // Demonstrate self-validation of filtering options
        options
            .validate()
            .expect("Severity filtering options should be valid");

        let formatter = ReportFormatter::with_options(options);

        let mut report = ValidationReport::new();
        report.add_violation(crate::domain::violations::Violation::new(
            "warning_rule",
            Severity::Warning,
            PathBuf::from("src/lib.rs"),
            "Warning message",
        ));
        report.add_violation(crate::domain::violations::Violation::new(
            "error_rule",
            Severity::Error,
            PathBuf::from("src/main.rs"),
            "Error message",
        ));

        let output = formatter
            .format_report(&report, OutputFormat::Json)
            .expect("JSON format should succeed for severity filtering test");

        // Verify the formatter self-validated the filtered output
        formatter
            .validate_format_integrity(&report, OutputFormat::Json, &output)
            .expect("Filtered JSON output should pass integrity validation");

        let json: JsonValue =
            serde_json::from_str(&output).expect("Severity filtered JSON should be valid");

        // Should only include the error, not the warning
        assert_eq!(
            json["violations"]
                .as_array()
                .expect("filtered violations should be an array")
                .len(),
            1
        );
        assert_eq!(json["violations"][0]["rule_id"], "error_rule");
    }

    #[test]
    fn test_domain_validation_behavior() {
        // Test that invalid options are rejected
        let invalid_options = ReportOptions {
            max_violations: Some(0), // Invalid: zero violations
            ..Default::default()
        };

        assert!(invalid_options.validate().is_err());

        // Test that the formatter constructor validates options
        assert!(ReportFormatter::new(invalid_options).is_err());

        // Test format validation for structured outputs
        let formatter = ReportFormatter::default();

        // Valid JSON should pass validation
        let valid_json = r#"{"violations": [], "summary": {"total_files": 0}}"#;
        assert!(formatter.validate_json_structure(valid_json).is_ok());

        // Invalid JSON should fail validation
        let invalid_json = r#"{"missing_required_fields": true}"#;
        assert!(formatter.validate_json_structure(invalid_json).is_err());

        // Test OutputFormat domain behavior
        assert!(OutputFormat::Human.supports_colors());
        assert!(!OutputFormat::Json.supports_colors());
        assert!(OutputFormat::Json.is_structured());
        assert!(!OutputFormat::Human.is_structured());

        // Test ReportOptions optimization behavior
        let human_options = ReportOptions::optimized_for(OutputFormat::Human);
        assert!(human_options.is_optimized_for(OutputFormat::Human));

        let json_options = ReportOptions::optimized_for(OutputFormat::Json);
        assert!(json_options.is_optimized_for(OutputFormat::Json));
        assert!(!json_options.use_colors); // JSON shouldn't use colors
    }
}
