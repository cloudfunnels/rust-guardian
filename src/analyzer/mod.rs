//! Main analysis orchestrator for Rust Guardian
//!
//! Code Quality Principle: Service Orchestration - Analyzer orchestrates complex validation workflows
//! - Coordinates path filtering, pattern matching, and result aggregation
//! - Provides clean interface for validating single files or directory trees
//! - Handles parallel processing and error recovery gracefully

pub mod rust;

use crate::analyzer::rust::RustAnalyzer;
use crate::config::GuardianConfig;
use crate::domain::violations::{GuardianError, GuardianResult, ValidationReport, Violation};
use crate::patterns::{PathFilter, PatternEngine};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Main analyzer that orchestrates the entire validation process
pub struct Analyzer {
    /// Configuration for this analysis
    config: GuardianConfig,
    /// Pattern engine for detecting violations
    pattern_engine: PatternEngine,
    /// Path filter for determining which files to analyze
    path_filter: PathFilter,
    /// Rust-specific analyzer
    rust_analyzer: RustAnalyzer,
}

/// Options for customizing analysis behavior
#[derive(Debug, Clone)]
pub struct AnalysisOptions {
    /// Whether to use parallel processing
    pub parallel: bool,
    /// Maximum number of files to analyze (-1 for unlimited)
    pub max_files: Option<usize>,
    /// Whether to continue on errors or fail fast
    pub fail_fast: bool,
    /// Additional paths to exclude for this analysis
    pub exclude_patterns: Vec<String>,
    /// Whether to ignore .guardianignore files
    pub ignore_ignore_files: bool,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            parallel: true,
            max_files: None,
            fail_fast: false,
            exclude_patterns: Vec::new(),
            ignore_ignore_files: false,
        }
    }
}

impl Analyzer {
    /// Create a new analyzer with the given configuration
    pub fn new(config: GuardianConfig) -> GuardianResult<Self> {
        let mut pattern_engine = PatternEngine::new();

        // Load all enabled rules into the pattern engine
        for (category_name, category) in &config.patterns {
            if !category.enabled {
                continue;
            }

            for rule in &category.rules {
                if !rule.enabled {
                    continue;
                }

                let effective_severity = config.effective_severity(category, rule);
                pattern_engine.add_rule(rule, effective_severity).map_err(|e| {
                    GuardianError::config(format!(
                        "Failed to add rule '{}' in category '{}': {}",
                        rule.id, category_name, e
                    ))
                })?;
            }
        }

        // Create path filter
        let ignore_file = if config.paths.ignore_file.as_deref() == Some("") {
            None
        } else {
            config.paths.ignore_file.clone()
        };

        let path_filter = PathFilter::new(config.paths.patterns.clone(), ignore_file)
            .map_err(|e| GuardianError::config(format!("Failed to create path filter: {e}")))?;

        Ok(Self { config, pattern_engine, path_filter, rust_analyzer: RustAnalyzer::new() })
    }

    /// Create an analyzer with default configuration
    pub fn with_defaults() -> GuardianResult<Self> {
        Self::new(GuardianConfig::default())
    }

    /// Analyze a single file and return violations
    pub fn analyze_file<P: AsRef<Path>>(&self, file_path: P) -> GuardianResult<Vec<Violation>> {
        let file_path = file_path.as_ref();

        // Check if file should be analyzed
        if !self.path_filter.should_analyze(file_path)? {
            return Ok(Vec::new());
        }

        // Read file content
        let content = fs::read_to_string(file_path).map_err(|e| {
            GuardianError::analysis(
                file_path.display().to_string(),
                format!("Failed to read file: {e}"),
            )
        })?;

        let mut all_violations = Vec::new();

        // Apply pattern matching
        let matches = self.pattern_engine.analyze_file(file_path, &content).map_err(|e| {
            GuardianError::analysis(
                file_path.display().to_string(),
                format!("Pattern analysis failed: {e}"),
            )
        })?;

        all_violations.extend(self.pattern_engine.matches_to_violations(matches));

        // Apply Rust-specific analysis for .rs files
        if self.rust_analyzer.handles_file(file_path) {
            let rust_violations = self.rust_analyzer.analyze(file_path, &content).map_err(|e| {
                GuardianError::analysis(
                    file_path.display().to_string(),
                    format!("Rust analysis failed: {e}"),
                )
            })?;
            all_violations.extend(rust_violations);
        }

        Ok(all_violations)
    }

    /// Analyze multiple files and return a complete validation report
    pub fn analyze_paths<P: AsRef<Path>>(
        &self,
        paths: &[P],
        options: &AnalysisOptions,
    ) -> GuardianResult<ValidationReport> {
        let start_time = Instant::now();
        let mut report = ValidationReport::new();

        // Collect all files to analyze
        let mut files_to_analyze = Vec::new();

        for path in paths {
            let path = path.as_ref();

            if path.is_file() {
                files_to_analyze.push(path.to_path_buf());
            } else if path.is_dir() {
                let discovered_files = self.path_filter.find_files(path)?;
                files_to_analyze.extend(discovered_files);
            }
        }

        // Apply additional exclusions if specified
        if !options.exclude_patterns.is_empty() {
            let mut temp_filter = self.path_filter.clone();
            for pattern in &options.exclude_patterns {
                temp_filter.add_pattern(pattern.clone())?;
            }
            files_to_analyze = temp_filter.filter_paths(&files_to_analyze)?;
        }

        // Limit number of files if requested
        if let Some(max_files) = options.max_files {
            files_to_analyze.truncate(max_files);
        }

        let total_files = files_to_analyze.len();

        // Analyze files (parallel or sequential)
        let violations = if options.parallel && files_to_analyze.len() > 1 {
            self.analyze_files_parallel(&files_to_analyze, options)?
        } else {
            self.analyze_files_sequential(&files_to_analyze, options)?
        };

        // Build final report
        for violation in violations {
            report.add_violation(violation);
        }

        report.set_files_analyzed(total_files);
        report.set_execution_time(start_time.elapsed().as_millis() as u64);
        report.set_config_fingerprint(self.config.fingerprint());
        report.sort_violations();

        Ok(report)
    }

    /// Analyze files sequentially
    fn analyze_files_sequential(
        &self,
        files: &[PathBuf],
        options: &AnalysisOptions,
    ) -> GuardianResult<Vec<Violation>> {
        let mut all_violations = Vec::new();

        for file_path in files {
            match self.analyze_file(file_path) {
                Ok(violations) => {
                    all_violations.extend(violations);
                }
                Err(e) => {
                    if options.fail_fast {
                        return Err(e);
                    } else {
                        // Log error and continue
                        tracing::warn!("Failed to analyze {}: {}", file_path.display(), e);
                    }
                }
            }
        }

        Ok(all_violations)
    }

    /// Analyze files in parallel
    fn analyze_files_parallel(
        &self,
        files: &[PathBuf],
        options: &AnalysisOptions,
    ) -> GuardianResult<Vec<Violation>> {
        let violations = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));

        files.par_iter().for_each(|file_path| match self.analyze_file(file_path) {
            Ok(file_violations) => {
                if let Ok(mut v) = violations.lock() {
                    v.extend(file_violations);
                }
            }
            Err(e) => {
                if let Ok(mut errs) = errors.lock() {
                    errs.push((file_path.clone(), e));
                }
            }
        });

        // Handle errors
        let errors = Arc::try_unwrap(errors)
            .map_err(|_| {
                GuardianError::analysis(
                    "parallel_analysis".to_string(),
                    "Failed to unwrap errors Arc".to_string(),
                )
            })?
            .into_inner()
            .map_err(|_| {
                GuardianError::analysis(
                    "parallel_analysis".to_string(),
                    "Failed to lock errors mutex".to_string(),
                )
            })?;

        if !errors.is_empty() {
            if options.fail_fast {
                if let Some((file_path, error)) = errors.into_iter().next() {
                    return Err(GuardianError::analysis(
                        file_path.display().to_string(),
                        error.to_string(),
                    ));
                }
            } else {
                // Log all errors
                for (file_path, error) in errors {
                    tracing::warn!("Failed to analyze {}: {}", file_path.display(), error);
                }
            }
        }

        let violations = Arc::try_unwrap(violations)
            .map_err(|_| {
                GuardianError::analysis(
                    "parallel_analysis".to_string(),
                    "Failed to unwrap violations Arc".to_string(),
                )
            })?
            .into_inner()
            .map_err(|_| {
                GuardianError::analysis(
                    "parallel_analysis".to_string(),
                    "Failed to lock violations mutex".to_string(),
                )
            })?;
        Ok(violations)
    }

    /// Analyze a directory tree and return a validation report
    pub fn analyze_directory<P: AsRef<Path>>(
        &self,
        root: P,
        options: &AnalysisOptions,
    ) -> GuardianResult<ValidationReport> {
        self.analyze_paths(&[root.as_ref()], options)
    }

    /// Get configuration fingerprint for cache validation
    pub fn config_fingerprint(&self) -> String {
        self.config.fingerprint()
    }

    /// Get statistics about the configured patterns
    pub fn pattern_stats(&self) -> PatternStats {
        let mut stats = PatternStats::default();

        for category in self.config.patterns.values() {
            if category.enabled {
                stats.enabled_categories += 1;

                for rule in &category.rules {
                    if rule.enabled {
                        stats.enabled_rules += 1;
                        match rule.rule_type {
                            crate::config::RuleType::Regex => stats.regex_patterns += 1,
                            crate::config::RuleType::Ast => stats.ast_patterns += 1,
                            crate::config::RuleType::Semantic => stats.semantic_patterns += 1,
                            crate::config::RuleType::ImportAnalysis => stats.import_patterns += 1,
                        }
                    } else {
                        stats.disabled_rules += 1;
                    }
                }
            } else {
                stats.disabled_categories += 1;
                stats.disabled_rules += category.rules.len();
            }
        }

        stats
    }
}

/// Statistics about configured patterns
#[derive(Debug, Default)]
pub struct PatternStats {
    pub enabled_categories: usize,
    pub disabled_categories: usize,
    pub enabled_rules: usize,
    pub disabled_rules: usize,
    pub regex_patterns: usize,
    pub ast_patterns: usize,
    pub semantic_patterns: usize,
    pub import_patterns: usize,
}

impl PatternStats {
    pub fn total_categories(&self) -> usize {
        self.enabled_categories + self.disabled_categories
    }

    pub fn total_rules(&self) -> usize {
        self.enabled_rules + self.disabled_rules
    }
}

/// Trait for custom file analyzers
pub trait FileAnalyzer {
    /// Analyze a file and return violations
    fn analyze(&self, file_path: &Path, content: &str) -> GuardianResult<Vec<Violation>>;

    /// Check if this analyzer handles the given file type
    fn handles_file(&self, file_path: &Path) -> bool;
}

/// Self-validation methods for analyzer functionality
/// Following code quality principle: Components should be self-validating
#[cfg(test)]
impl Analyzer {
    /// Validate analyzer creation and configuration
    pub fn validate_initialization(&self) -> GuardianResult<()> {
        let stats = self.pattern_stats();

        if stats.enabled_rules == 0 {
            return Err(GuardianError::config(
                "Analyzer must have at least one enabled rule".to_string(),
            ));
        }

        if stats.regex_patterns == 0 && stats.ast_patterns == 0 && stats.semantic_patterns == 0 {
            return Err(GuardianError::config(
                "Analyzer must have at least one pattern type enabled".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate single file analysis capabilities
    pub fn validate_file_analysis(&self, test_content: &str) -> GuardianResult<()> {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to create temp dir: {e}"),
            )
        })?;
        let file_path = temp_dir.path().join("validation_test.rs");

        fs::write(&file_path, test_content).map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to write test file: {e}"),
            )
        })?;

        let violations = self.analyze_file(&file_path)?;

        // Validate that violations are properly formatted
        for violation in &violations {
            if violation.rule_id.is_empty() {
                return Err(GuardianError::analysis(
                    "validation".to_string(),
                    "Violation missing rule_id".to_string(),
                ));
            }
            if violation.message.is_empty() {
                return Err(GuardianError::analysis(
                    "validation".to_string(),
                    "Violation missing message".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Validate directory analysis capabilities
    pub fn validate_directory_analysis(&self) -> GuardianResult<()> {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to create temp dir: {e}"),
            )
        })?;
        let root = temp_dir.path();

        // Create realistic directory structure
        fs::create_dir_all(root.join("src")).map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to create src dir: {e}"),
            )
        })?;
        fs::create_dir_all(root.join("target/debug")).map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to create target dir: {e}"),
            )
        })?;

        // Create test files with known patterns
        fs::write(root.join("src/lib.rs"), "//! Test module\n//!\n//! Code Quality Principle: Self-validation\npub fn test() { /* implementation */ }")
            .map_err(|e| GuardianError::analysis("validation".to_string(), format!("Failed to write lib.rs: {e}")))?;
        fs::write(root.join("src/main.rs"), "//! Main module\n//!\n//! Code Quality Principle: Entry point\nfn main() { eprintln!(\"Application starting\"); }")
            .map_err(|e| GuardianError::analysis("validation".to_string(), format!("Failed to write main.rs: {e}")))?;
        fs::write(root.join("target/debug/app"), "binary content").map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to write binary: {e}"),
            )
        })?;

        let report = self.analyze_directory(root, &AnalysisOptions::default())?;

        // Validate report structure
        if report.summary.total_files == 0 {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Directory analysis should find at least one file".to_string(),
            ));
        }

        // Validate that target directory is excluded
        let target_violations = report
            .violations
            .iter()
            .filter(|v| v.file_path.to_string_lossy().contains("target/"))
            .count();

        if target_violations > 0 {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Target directory should be excluded from analysis".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate analysis options functionality
    pub fn validate_analysis_options(&self) -> GuardianResult<()> {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to create temp dir: {e}"),
            )
        })?;
        let root = temp_dir.path();

        fs::create_dir_all(root.join("src")).map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to create src dir: {e}"),
            )
        })?;
        fs::write(
            root.join("src/lib.rs"),
            "//! Test lib\n//!\n//! Code Quality Principle: Testing\npub fn lib() {}",
        )
        .map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to write lib.rs: {e}"),
            )
        })?;
        fs::write(
            root.join("src/main.rs"),
            "//! Test main\n//!\n//! Code Quality Principle: Entry\nfn main() {}",
        )
        .map_err(|e| {
            GuardianError::analysis(
                "validation".to_string(),
                format!("Failed to write main.rs: {e}"),
            )
        })?;

        // Test max_files limitation
        let options = AnalysisOptions { max_files: Some(1), ..Default::default() };

        let report = self.analyze_directory(root, &options)?;

        if report.summary.total_files != 1 {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                format!("Expected 1 file with max_files=1, got {}", report.summary.total_files),
            ));
        }

        Ok(())
    }
}

/// Comprehensive validation entry point for the analyzer
/// This replaces traditional unit tests with domain self-validation
#[cfg(test)]
pub fn validate_analyzer_domain() -> GuardianResult<()> {
    let analyzer = Analyzer::with_defaults()?;

    // Validate core functionality
    analyzer.validate_initialization()?;
    analyzer.validate_file_analysis(
        "//! Test\n//!\n//! Code Quality Principle: Validation\nfn test() {}",
    )?;
    analyzer.validate_directory_analysis()?;
    analyzer.validate_analysis_options()?;

    // Validate pattern statistics
    let stats = analyzer.pattern_stats();
    if stats.total_rules() == 0 {
        return Err(GuardianError::config(
            "Pattern statistics validation failed: no rules configured".to_string(),
        ));
    }

    if stats.total_categories() == 0 {
        return Err(GuardianError::config(
            "Pattern statistics validation failed: no categories configured".to_string(),
        ));
    }

    Ok(())
}
