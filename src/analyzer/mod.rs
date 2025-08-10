//! Main analysis orchestrator for Rust Guardian
//!
//! CDD Principle: Domain Services - Analyzer orchestrates complex validation workflows
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
    /// Additional paths to exclude (temporary)
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
                pattern_engine
                    .add_rule(rule, effective_severity)
                    .map_err(|e| {
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

        Ok(Self {
            config,
            pattern_engine,
            path_filter,
            rust_analyzer: RustAnalyzer::new(),
        })
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
        let matches = self
            .pattern_engine
            .analyze_file(file_path, &content)
            .map_err(|e| {
                GuardianError::analysis(
                    file_path.display().to_string(),
                    format!("Pattern analysis failed: {e}"),
                )
            })?;

        all_violations.extend(self.pattern_engine.matches_to_violations(matches));

        // Apply Rust-specific analysis for .rs files
        if self.rust_analyzer.handles_file(file_path) {
            let rust_violations = self
                .rust_analyzer
                .analyze(file_path, &content)
                .map_err(|e| {
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

        files
            .par_iter()
            .for_each(|file_path| match self.analyze_file(file_path) {
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
        let errors = Arc::try_unwrap(errors).unwrap().into_inner().unwrap();
        if !errors.is_empty() {
            if options.fail_fast {
                let (file_path, error) = errors.into_iter().next().unwrap();
                return Err(GuardianError::analysis(
                    file_path.display().to_string(),
                    error.to_string(),
                ));
            } else {
                // Log all errors
                for (file_path, error) in errors {
                    tracing::warn!("Failed to analyze {}: {}", file_path.display(), error);
                }
            }
        }

        let violations = Arc::try_unwrap(violations).unwrap().into_inner().unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::with_defaults().unwrap();
        let stats = analyzer.pattern_stats();

        // Should have default patterns loaded
        assert!(stats.enabled_rules > 0);
        assert!(stats.regex_patterns > 0);
    }

    #[test]
    fn test_single_file_analysis() -> GuardianResult<()> {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");

        fs::write(&file_path, "// TODO: implement this\nfn main() {}")?;

        let analyzer = Analyzer::with_defaults()?;
        let violations = analyzer.analyze_file(&file_path)?;

        // Should find the TODO comment
        assert!(!violations.is_empty());
        assert!(violations.iter().any(|v| v.rule_id.contains("todo")));

        Ok(())
    }

    #[test]
    fn test_directory_analysis() -> GuardianResult<()> {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create directory structure
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("target/debug"))?;

        // Create test files
        fs::write(root.join("src/lib.rs"), "// TODO: implement\nfn test() {}")?;
        fs::write(
            root.join("src/main.rs"),
            "fn main() { println!(\"Hello\"); }",
        )?;
        fs::write(root.join("target/debug/app"), "binary file")?; // Should be excluded

        let analyzer = Analyzer::with_defaults()?;
        let report = analyzer.analyze_directory(root, &AnalysisOptions::default())?;

        // Should analyze Rust files but not target directory
        assert!(report.summary.total_files > 0);
        assert!(report.has_violations());

        // Should find the TODO
        let todo_violations: Vec<_> = report
            .violations
            .iter()
            .filter(|v| v.rule_id.contains("todo"))
            .collect();
        assert!(!todo_violations.is_empty());

        Ok(())
    }

    #[test]
    fn test_analysis_options() -> GuardianResult<()> {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/lib.rs"), "// TODO: test")?;
        fs::write(root.join("src/main.rs"), "// TODO: main")?;

        let analyzer = Analyzer::with_defaults()?;

        // Test max_files limitation
        let options = AnalysisOptions {
            max_files: Some(1),
            ..Default::default()
        };

        let report = analyzer.analyze_directory(root, &options)?;
        assert_eq!(report.summary.total_files, 1);

        Ok(())
    }

    #[test]
    fn test_pattern_stats() {
        let analyzer = Analyzer::with_defaults().unwrap();
        let stats = analyzer.pattern_stats();

        assert!(stats.total_rules() > 0);
        assert!(stats.total_categories() > 0);
        assert!(stats.enabled_rules > 0);
    }
}
