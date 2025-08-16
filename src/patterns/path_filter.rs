//! Path filtering using .gitignore-style patterns
//!
//! Architectural Principle: Service Layer - PathFilter orchestrates complex path matching logic
//! - Encapsulates the rules for include/exclude pattern evaluation
//! - Provides clean interface for determining whether a path should be analyzed
//! - Handles .guardianignore file discovery and parsing

use crate::domain::violations::{GuardianError, GuardianResult};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Manages path filtering using .gitignore-style patterns
#[derive(Debug, Clone)]
pub struct PathFilter {
    /// Include/exclude patterns
    patterns: Vec<FilterPattern>,
    /// Whether to process .guardianignore files
    process_ignore_files: bool,
    /// Name of ignore files to process
    ignore_filename: String,
}

/// A single path filter pattern
#[derive(Debug, Clone)]
struct FilterPattern {
    /// The glob pattern
    pattern: glob::Pattern,
    /// Whether this is an include pattern (starts with !)
    is_include: bool,
    /// Original pattern string for debugging
    original: String,
}

impl PathFilter {
    /// Create a new path filter with the given patterns
    pub fn new(patterns: Vec<String>, ignore_filename: Option<String>) -> GuardianResult<Self> {
        let mut filter_patterns = Vec::new();

        for pattern_str in patterns {
            let (is_include, pattern_str) = if let Some(stripped) = pattern_str.strip_prefix('!') {
                (true, stripped.to_string())
            } else {
                (false, pattern_str)
            };

            let pattern = glob::Pattern::new(&pattern_str).map_err(|e| {
                GuardianError::pattern(format!("Invalid pattern '{pattern_str}': {e}"))
            })?;

            filter_patterns.push(FilterPattern { pattern, is_include, original: pattern_str });
        }

        Ok(Self {
            patterns: filter_patterns,
            process_ignore_files: ignore_filename.is_some(),
            ignore_filename: ignore_filename.unwrap_or_else(|| ".guardianignore".to_string()),
        })
    }

    /// Create a default path filter with sensible exclusions
    pub fn with_defaults() -> GuardianResult<Self> {
        Self::new(
            vec![
                // Exclude common build/cache directories
                "target/**".to_string(),
                "**/node_modules/**".to_string(),
                "**/.git/**".to_string(),
                "**/*.generated.*".to_string(),
                "**/dist/**".to_string(),
                "**/build/**".to_string(),
            ],
            Some(".guardianignore".to_string()),
        )
    }

    /// Check if a file should be analyzed based on all patterns and ignore files
    pub fn should_analyze<P: AsRef<Path>>(&self, path: P) -> GuardianResult<bool> {
        let path = path.as_ref();
        let _path_str = path.to_string_lossy();

        // Start with default: include all files
        let mut should_include = true;

        // Apply patterns in order (like .gitignore)
        for pattern in &self.patterns {
            let matches = self.pattern_matches_path(pattern, path);

            if matches {
                should_include = pattern.is_include;
            }
        }

        // If excluded by configured patterns, return false
        if !should_include {
            return Ok(false);
        }

        // Check .guardianignore files if enabled
        if self.process_ignore_files {
            let ignored_by_files = self.is_ignored_by_files(path)?;
            if ignored_by_files {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Check if path is ignored by .guardianignore files
    fn is_ignored_by_files<P: AsRef<Path>>(&self, path: P) -> GuardianResult<bool> {
        let path = path.as_ref();
        let mut current_dir = path.parent();
        let mut is_ignored = false;

        // Walk up the directory tree looking for .guardianignore files
        while let Some(dir) = current_dir {
            let ignore_file = dir.join(&self.ignore_filename);

            if ignore_file.exists() {
                let patterns = self.load_ignore_file(&ignore_file)?;

                // Check if any pattern in this file matches
                for pattern in patterns {
                    // Make path relative to the ignore file's directory
                    if let Ok(relative_path) = path.strip_prefix(dir) {
                        let matches = self.pattern_matches_path(&pattern, relative_path);

                        if matches {
                            is_ignored = !pattern.is_include;
                        }
                    }
                }
            }

            current_dir = dir.parent();
        }

        Ok(is_ignored)
    }

    /// Load patterns from a .guardianignore file
    fn load_ignore_file<P: AsRef<Path>>(&self, path: P) -> GuardianResult<Vec<FilterPattern>> {
        let content = fs::read_to_string(&path).map_err(|e| {
            GuardianError::config(format!(
                "Failed to read ignore file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;

        let mut patterns = Vec::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let (is_include, pattern_str) = if let Some(stripped) = line.strip_prefix('!') {
                (true, stripped.to_string())
            } else {
                (false, line.to_string())
            };

            match glob::Pattern::new(&pattern_str) {
                Ok(pattern) => {
                    patterns.push(FilterPattern { pattern, is_include, original: pattern_str });
                }
                Err(e) => {
                    // Log warning but don't fail - just skip invalid patterns
                    tracing::warn!(
                        "Invalid pattern '{}' in {}: {}",
                        pattern_str,
                        path.as_ref().display(),
                        e
                    );
                }
            }
        }

        Ok(patterns)
    }

    /// Get all files that should be analyzed in a directory tree
    pub fn find_files<P: AsRef<Path>>(&self, root: P) -> GuardianResult<Vec<PathBuf>> {
        let root = root.as_ref();
        let mut files = Vec::new();

        for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();

            // Only process files, not directories
            if path.is_file() && self.should_analyze(path)? {
                files.push(path.to_path_buf());
            }
        }

        Ok(files)
    }

    /// Filter a list of paths to only those that should be analyzed
    pub fn filter_paths<P: AsRef<Path>>(&self, paths: &[P]) -> GuardianResult<Vec<PathBuf>> {
        let mut filtered = Vec::new();

        for path in paths {
            if self.should_analyze(path)? {
                filtered.push(path.as_ref().to_path_buf());
            }
        }

        Ok(filtered)
    }

    /// Add a pattern to the filter
    pub fn add_pattern(&mut self, pattern: String) -> GuardianResult<()> {
        let (is_include, pattern_str) = if let Some(stripped) = pattern.strip_prefix('!') {
            (true, stripped.to_string())
        } else {
            (false, pattern)
        };

        let glob_pattern = glob::Pattern::new(&pattern_str)
            .map_err(|e| GuardianError::pattern(format!("Invalid pattern '{pattern_str}': {e}")))?;

        self.patterns.push(FilterPattern {
            pattern: glob_pattern,
            is_include,
            original: pattern_str,
        });

        Ok(())
    }

    /// Get debug information about patterns and their matches
    pub fn debug_patterns<P: AsRef<Path>>(&self, path: P) -> Vec<String> {
        let path = path.as_ref();
        let mut debug_info = Vec::new();

        for (i, pattern) in self.patterns.iter().enumerate() {
            let matches = self.pattern_matches_path(pattern, path);
            let prefix = if pattern.is_include { "!" } else { "" };

            debug_info.push(format!(
                "Pattern {}: {}{} -> {}",
                i,
                prefix,
                pattern.original,
                if matches { "MATCH" } else { "no match" }
            ));
        }

        debug_info
    }

    /// Check if a pattern matches a path using .gitignore-style rules
    fn pattern_matches_path(&self, pattern: &FilterPattern, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Handle different pattern types
        if pattern.original.ends_with('/') {
            // Directory pattern - only match directories
            if !path.is_dir() {
                return false;
            }
            // Remove trailing slash and match
            let dir_pattern = pattern.original.trim_end_matches('/');
            return glob::Pattern::new(dir_pattern).map(|p| p.matches(&path_str)).unwrap_or(false);
        }

        if pattern.original.starts_with('/') {
            // Absolute pattern from root - remove leading slash and match from beginning
            let absolute_pattern = pattern.original.strip_prefix('/').unwrap_or(&pattern.original);
            return glob::Pattern::new(absolute_pattern)
                .map(|p| p.matches(&path_str))
                .unwrap_or(false);
        }

        if pattern.original.contains('/') {
            // Pattern contains slash - match full path
            return pattern.pattern.matches(&path_str);
        } else {
            // No slash - match filename only
            if let Some(filename) = path.file_name() {
                return pattern.pattern.matches(&filename.to_string_lossy());
            }
        }

        false
    }
}

/// Architecture-compliant validation functions for integration testing
#[cfg(test)]
#[allow(dead_code)]
pub mod validation {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Validate basic pattern matching functionality - designed for integration testing
    pub fn validate_basic_pattern_matching() -> GuardianResult<()> {
        let filter = PathFilter::new(
            vec![
                "target/**".to_string(), // Exclude target directory
                "*.md".to_string(),      // Exclude markdown files
            ],
            None,
        )?;

        if !filter.should_analyze(Path::new("src/lib.rs"))? {
            return Err(GuardianError::pattern(
                "Basic pattern validation failed - should analyze src files",
            ));
        }

        if filter.should_analyze(Path::new("target/debug/lib.rs"))? {
            return Err(GuardianError::pattern(
                "Basic pattern validation failed - should exclude target files",
            ));
        }

        if filter.should_analyze(Path::new("README.md"))? {
            return Err(GuardianError::pattern(
                "Basic pattern validation failed - should exclude markdown files",
            ));
        }

        Ok(())
    }

    /// Validate include override functionality - designed for integration testing
    pub fn validate_include_override() -> GuardianResult<()> {
        let filter = PathFilter::new(
            vec![
                "target/**".to_string(),          // Exclude target
                "!target/special/**".to_string(), // But include target/special
            ],
            None,
        )?;

        if filter.should_analyze(Path::new("target/debug/lib.rs"))? {
            return Err(GuardianError::pattern(
                "Include override validation failed - should exclude target/debug",
            ));
        }

        if !filter.should_analyze(Path::new("target/special/lib.rs"))? {
            return Err(GuardianError::pattern(
                "Include override validation failed - should include target/special",
            ));
        }

        Ok(())
    }

    /// Validate pattern order functionality - designed for integration testing
    pub fn validate_pattern_order() -> GuardianResult<()> {
        let filter = PathFilter::new(
            vec![
                "tests/**".to_string(),            // Exclude tests
                "!tests/important.rs".to_string(), // But include important test
                "!*.rs".to_string(),               // And include all .rs files (overrides excludes)
            ],
            None,
        )?;

        if !filter.should_analyze(Path::new("src/lib.rs"))? {
            return Err(GuardianError::pattern(
                "Pattern order validation failed - should analyze src files",
            ));
        }

        if !filter.should_analyze(Path::new("tests/unit.rs"))? {
            return Err(GuardianError::pattern(
                "Pattern order validation failed - should analyze test files with overrides",
            ));
        }

        if !filter.should_analyze(Path::new("tests/important.rs"))? {
            return Err(GuardianError::pattern(
                "Pattern order validation failed - should analyze important test files",
            ));
        }

        Ok(())
    }

    /// Validate guardianignore file functionality - designed for integration testing
    pub fn validate_guardianignore_file() -> GuardianResult<()> {
        let temp_dir = TempDir::new()
            .map_err(|e| GuardianError::config(format!("Failed to create temp dir: {}", e)))?;
        let root = temp_dir.path();

        // Create directory structure
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("tests"))?;

        // Create .guardianignore file
        fs::write(root.join(".guardianignore"), "*.tmp\ntests/**\n!tests/important.rs\n")?;

        // Create test files
        fs::write(root.join("src/lib.rs"), "")?;
        fs::write(root.join("temp.tmp"), "")?;
        fs::write(root.join("tests/unit.rs"), "")?;
        fs::write(root.join("tests/important.rs"), "")?;

        let filter = PathFilter::new(vec![], Some(".guardianignore".to_string()))?;

        if !filter.should_analyze(root.join("src/lib.rs"))? {
            return Err(GuardianError::pattern(
                "Guardianignore validation failed - should analyze src files",
            ));
        }

        if filter.should_analyze(root.join("temp.tmp"))? {
            return Err(GuardianError::pattern(
                "Guardianignore validation failed - should exclude tmp files",
            ));
        }

        if filter.should_analyze(root.join("tests/unit.rs"))? {
            return Err(GuardianError::pattern(
                "Guardianignore validation failed - should exclude test files",
            ));
        }

        if !filter.should_analyze(root.join("tests/important.rs"))? {
            return Err(GuardianError::pattern(
                "Guardianignore validation failed - should include important files",
            ));
        }

        Ok(())
    }

    /// Validate invalid pattern handling - designed for integration testing
    pub fn validate_invalid_pattern_handling() -> GuardianResult<()> {
        let result = PathFilter::new(vec!["[invalid".to_string()], None);
        if result.is_ok() {
            return Err(GuardianError::pattern(
                "Invalid pattern validation failed - should reject invalid patterns",
            ));
        }

        Ok(())
    }

    /// Validate default filter functionality - designed for integration testing
    pub fn validate_default_filter() -> GuardianResult<()> {
        let filter = PathFilter::with_defaults()?;

        if filter.should_analyze(Path::new("target/debug/lib.rs"))? {
            return Err(GuardianError::pattern(
                "Default filter validation failed - should exclude target directory",
            ));
        }

        if !filter.should_analyze(Path::new("src/lib.rs"))? {
            return Err(GuardianError::pattern(
                "Default filter validation failed - should include source files",
            ));
        }

        Ok(())
    }
}
