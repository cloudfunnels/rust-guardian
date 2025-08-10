//! Report generation with multiple output formats
//! 
//! CDD Principle: Anti-Corruption Layer - Formatters translate domain objects to external formats
//! - ValidationReport (domain) is converted to various external representations
//! - Each formatter encapsulates the rules for its specific output format
//! - Domain logic remains pure while supporting multiple presentation needs

use crate::domain::violations::{ValidationReport, Violation, Severity, GuardianResult};
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
}

impl OutputFormat {
    /// Parse format from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "human" => Some(Self::Human),
            "json" => Some(Self::Json),
            "junit" => Some(Self::Junit),
            "sarif" => Some(Self::Sarif),
            "github" => Some(Self::GitHub),
            _ => None,
        }
    }
    
    /// Get all available format names
    pub fn all_formats() -> &'static [&'static str] {
        &["human", "json", "junit", "sarif", "github"]
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

/// Main report formatter that dispatches to specific formatters
pub struct ReportFormatter {
    options: ReportOptions,
}

impl ReportFormatter {
    /// Create a new report formatter with options
    pub fn new(options: ReportOptions) -> Self {
        Self { options }
    }
    
    /// Create a formatter with default options
    pub fn default() -> Self {
        Self::new(ReportOptions::default())
    }
    
    /// Format a validation report in the specified format
    pub fn format_report(&self, report: &ValidationReport, format: OutputFormat) -> GuardianResult<String> {
        // Filter violations based on options
        let filtered_violations = self.filter_violations(&report.violations);
        
        match format {
            OutputFormat::Human => self.format_human(report, &filtered_violations),
            OutputFormat::Json => self.format_json(report, &filtered_violations),
            OutputFormat::Junit => self.format_junit(report, &filtered_violations),
            OutputFormat::Sarif => self.format_sarif(report, &filtered_violations),
            OutputFormat::GitHub => self.format_github(report, &filtered_violations),
        }
    }
    
    /// Write a formatted report to a writer
    pub fn write_report<W: Write>(
        &self, 
        report: &ValidationReport, 
        format: OutputFormat, 
        mut writer: W
    ) -> GuardianResult<()> {
        let formatted = self.format_report(report, format)?;
        writer.write_all(formatted.as_bytes())
            .map_err(|e| crate::domain::violations::GuardianError::Io { source: e })?;
        Ok(())
    }
    
    /// Filter violations based on report options
    fn filter_violations<'a>(&self, violations: &'a [Violation]) -> Vec<&'a Violation> {
        let mut filtered: Vec<&Violation> = violations.iter()
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
    fn format_human(&self, report: &ValidationReport, violations: &[&Violation]) -> GuardianResult<String> {
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
                output.push_str(&format!("{} \x1b[{}mCode Quality Violations Found\x1b[0m\n\n", icon, color));
            } else {
                output.push_str(&format!("{} Code Quality Violations Found\n\n", icon));
            }
            
            // Group violations by file
            let mut by_file: std::collections::BTreeMap<&std::path::Path, Vec<&Violation>> = 
                std::collections::BTreeMap::new();
            
            for violation in violations {
                by_file.entry(&violation.file_path).or_default().push(violation);
            }
            
            // Display each file's violations
            for (file_path, file_violations) in by_file {
                output.push_str(&format!("📁 {}\n", file_path.display()));
                
                for violation in file_violations {
                    // Format violation with colors
                    let severity_color = match violation.severity {
                        Severity::Error => "31", // Red
                        Severity::Warning => "33", // Yellow
                        Severity::Info => "36", // Cyan
                    };
                    
                    let position = match (violation.line_number, violation.column_number) {
                        (Some(line), Some(col)) => format!("{}:{}", line, col),
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
                                output.push_str(&format!("    \x1b[2m│ {}\x1b[0m\n", context));
                            } else {
                                output.push_str(&format!("    │ {}\n", context));
                            }
                        }
                    }
                    
                    // Show suggestions if available and requested
                    if self.options.show_suggestions {
                        if let Some(suggestion) = &violation.suggested_fix {
                            if self.options.use_colors {
                                output.push_str(&format!("    \x1b[32m💡 {}\x1b[0m\n", suggestion));
                            } else {
                                output.push_str(&format!("    💡 {}\n", suggestion));
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
    fn format_json(&self, report: &ValidationReport, violations: &[&Violation]) -> GuardianResult<String> {
        let json_violations: Vec<JsonValue> = violations.iter().map(|v| {
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
        }).collect();
        
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
        
        serde_json::to_string_pretty(&json_report)
            .map_err(|e| crate::domain::violations::GuardianError::config(format!("JSON serialization failed: {}", e)))
    }
    
    /// Format report in JUnit XML format
    fn format_junit(&self, report: &ValidationReport, violations: &[&Violation]) -> GuardianResult<String> {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        
        let total_tests = violations.len();
        let failures = violations.iter().filter(|v| v.severity == Severity::Error).count();
        let errors = 0; // We don't distinguish between failures and errors for now
        let execution_time = (report.summary.execution_time_ms as f64) / 1000.0;
        
        xml.push_str(&format!(
            "<testsuite name=\"rust-guardian\" tests=\"{}\" failures=\"{}\" errors=\"{}\" time=\"{:.3}\">\n",
            total_tests, failures, errors, execution_time
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
    fn format_sarif(&self, _report: &ValidationReport, violations: &[&Violation]) -> GuardianResult<String> {
        let sarif_results: Vec<JsonValue> = violations.iter().map(|v| {
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
        }).collect();
        
        let sarif_report = serde_json::json!({
            "version": "2.1.0",
            "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
            "runs": [{
                "tool": {
                    "driver": {
                        "name": "rust-guardian",
                        "version": "0.1.0",
                        "informationUri": "https://github.com/cloudfunnels/rust-guardian"
                    }
                },
                "results": sarif_results
            }]
        });
        
        serde_json::to_string_pretty(&sarif_report)
            .map_err(|e| crate::domain::violations::GuardianError::config(format!("SARIF serialization failed: {}", e)))
    }
    
    /// Format report for GitHub Actions
    fn format_github(&self, _report: &ValidationReport, violations: &[&Violation]) -> GuardianResult<String> {
        let mut output = String::new();
        
        for violation in violations {
            let level = match violation.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "notice",
            };
            
            let position = match (violation.line_number, violation.column_number) {
                (Some(line), Some(col)) => format!("line={},col={}", line, col),
                (Some(line), None) => format!("line={}", line),
                _ => String::new(),
            };
            
            let position_part = if position.is_empty() { 
                String::new() 
            } else { 
                format!(" {}", position) 
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
                let text = format!("{} error{}", 
                    report.summary.violations_by_severity.error,
                    if report.summary.violations_by_severity.error == 1 { "" } else { "s" }
                );
                if self.options.use_colors {
                    parts.push(format!("\x1b[31m{}\x1b[0m", text));
                } else {
                    parts.push(text);
                }
            }
            
            if report.summary.violations_by_severity.warning > 0 {
                let text = format!("{} warning{}", 
                    report.summary.violations_by_severity.warning,
                    if report.summary.violations_by_severity.warning == 1 { "" } else { "s" }
                );
                if self.options.use_colors {
                    parts.push(format!("\x1b[33m{}\x1b[0m", text));
                } else {
                    parts.push(text);
                }
            }
            
            if report.summary.violations_by_severity.info > 0 {
                let text = format!("{} info", report.summary.violations_by_severity.info);
                if self.options.use_colors {
                    parts.push(format!("\x1b[36m{}\x1b[0m", text));
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
                "Test violation"
            )
            .with_position(42, 15)
            .with_context("let x = todo!();")
        );
        
        report.set_files_analyzed(10);
        report.set_execution_time(1200);
        
        report
    }
    
    #[test]
    fn test_human_format() {
        let formatter = ReportFormatter::new(ReportOptions {
            use_colors: false,
            ..Default::default()
        });
        
        let report = create_test_report();
        let output = formatter.format_report(&report, OutputFormat::Human).unwrap();
        
        assert!(output.contains("Code Quality Violations Found"));
        assert!(output.contains("src/main.rs"));
        assert!(output.contains("Test violation"));
        assert!(output.contains("Summary:"));
    }
    
    #[test]
    fn test_json_format() {
        let formatter = ReportFormatter::default();
        let report = create_test_report();
        let output = formatter.format_report(&report, OutputFormat::Json).unwrap();
        
        let json: JsonValue = serde_json::from_str(&output).unwrap();
        assert!(json["violations"].is_array());
        assert_eq!(json["violations"].as_array().unwrap().len(), 1);
        assert_eq!(json["violations"][0]["rule_id"], "test_rule");
        assert_eq!(json["summary"]["total_files"], 10);
    }
    
    #[test]
    fn test_junit_format() {
        let formatter = ReportFormatter::default();
        let report = create_test_report();
        let output = formatter.format_report(&report, OutputFormat::Junit).unwrap();
        
        assert!(output.contains("<?xml version=\"1.0\""));
        assert!(output.contains("<testsuite"));
        assert!(output.contains("test_rule"));
        assert!(output.contains("<failure"));
    }
    
    #[test]
    fn test_github_format() {
        let formatter = ReportFormatter::default();
        let report = create_test_report();
        let output = formatter.format_report(&report, OutputFormat::GitHub).unwrap();
        
        assert!(output.contains("::error"));
        assert!(output.contains("file=src/main.rs"));
        assert!(output.contains("line=42,col=15"));
        assert!(output.contains("Test violation"));
    }
    
    #[test]
    fn test_empty_report() {
        let formatter = ReportFormatter::new(ReportOptions {
            use_colors: false,
            ..Default::default()
        });
        
        let report = ValidationReport::new();
        let output = formatter.format_report(&report, OutputFormat::Human).unwrap();
        
        assert!(output.contains("No code quality violations found"));
    }
    
    #[test]
    fn test_severity_filtering() {
        let formatter = ReportFormatter::new(ReportOptions {
            min_severity: Some(Severity::Error),
            ..Default::default()
        });
        
        let mut report = ValidationReport::new();
        report.add_violation(
            crate::domain::violations::Violation::new(
                "warning_rule",
                Severity::Warning,
                PathBuf::from("src/lib.rs"),
                "Warning message"
            )
        );
        report.add_violation(
            crate::domain::violations::Violation::new(
                "error_rule",
                Severity::Error,
                PathBuf::from("src/main.rs"),
                "Error message"
            )
        );
        
        let output = formatter.format_report(&report, OutputFormat::Json).unwrap();
        let json: JsonValue = serde_json::from_str(&output).unwrap();
        
        // Should only include the error, not the warning
        assert_eq!(json["violations"].as_array().unwrap().len(), 1);
        assert_eq!(json["violations"][0]["rule_id"], "error_rule");
    }
}