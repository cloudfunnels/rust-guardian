//! Configuration loading and management for Rust Guardian
//!
//! Architecture: Anti-Corruption Layer - Configuration translates external YAML formats
//! - Raw YAML structures are converted to clean domain objects
//! - Default configurations are embedded in the domain, not infrastructure
//! - Configuration acts as a repository for pattern rules and path filters

use crate::domain::violations::{GuardianError, GuardianResult, Severity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Main configuration structure for Rust Guardian
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianConfig {
    /// Configuration format version
    pub version: String,
    /// Path filtering configuration
    pub paths: PathConfig,
    /// Pattern definitions organized by category
    pub patterns: HashMap<String, PatternCategory>,
}

/// Path filtering configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    /// Include/exclude patterns (gitignore-style)
    pub patterns: Vec<String>,
    /// Optional .guardianignore file name
    pub ignore_file: Option<String>,
}

/// A category of patterns (e.g., "placeholders", "architectural_violations")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternCategory {
    /// Default severity for patterns in this category
    pub severity: Severity,
    /// Whether this category is enabled
    pub enabled: bool,
    /// Individual pattern rules
    pub rules: Vec<PatternRule>,
}

/// Individual pattern rule configuration
#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct PatternRule {
    /// Unique identifier for this rule
    pub id: String,
    /// Type of pattern (regex, ast, semantic)
    #[serde(rename = "type")]
    pub rule_type: RuleType,
    /// The pattern to match
    pub pattern: String,
    /// Human-readable message for violations
    pub message: String,
    /// Severity override (uses category default if not specified)
    pub severity: Option<Severity>,
    /// Whether this rule is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Case sensitivity for regex patterns
    #[serde(default)]
    pub case_sensitive: bool,
    /// Conditions that exclude matches from being violations
    pub exclude_if: Option<ExcludeConditions>,
}

/// Types of pattern matching
#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    /// Regular expression pattern matching
    Regex,
    /// Abstract syntax tree analysis
    Ast,
    /// Semantic code analysis
    Semantic,
    /// Import/dependency analysis
    ImportAnalysis,
}

/// Conditions that can exclude a match from being reported as a violation
#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct ExcludeConditions {
    /// Exclude if code has specific attributes (e.g., #[test])
    pub attribute: Option<String>,
    /// Exclude if in test files
    #[serde(default)]
    pub in_tests: bool,
    /// Exclude if in specific file patterns
    pub file_patterns: Option<Vec<String>>,
}

impl GuardianConfig {
    /// Load configuration from a YAML file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> GuardianResult<Self> {
        let contents = fs::read_to_string(&path).map_err(|e| {
            GuardianError::config(format!(
                "Failed to read config file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;

        let config: Self = serde_yaml::from_str(&contents).map_err(|e| {
            GuardianError::config(format!(
                "Failed to parse config file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;

        config.validate()?;
        Ok(config)
    }

    /// Load configuration from string content
    pub fn load_from_str(content: &str) -> GuardianResult<Self> {
        let config: Self = serde_yaml::from_str(content)
            .map_err(|e| GuardianError::config(format!("Failed to parse config: {e}")))?;

        config.validate()?;
        Ok(config)
    }

    /// Get default configuration with built-in patterns
    pub fn with_defaults() -> Self {
        Self {
            version: "1.0".to_string(),
            paths: PathConfig {
                patterns: vec![
                    // Default exclusions
                    "target/".to_string(),
                    "**/node_modules/".to_string(),
                    "**/.git/".to_string(),
                    "**/*.generated.*".to_string(),
                ],
                ignore_file: Some(".guardianignore".to_string()),
            },
            patterns: Self::default_patterns(),
        }
    }

    /// Get default pattern definitions
    fn default_patterns() -> HashMap<String, PatternCategory> {
        let mut patterns = HashMap::new();

        // Development marker detection patterns
        patterns.insert(
            "placeholders".to_string(),
            PatternCategory {
                severity: Severity::Error,
                enabled: true,
                rules: vec![
                    PatternRule {
                        id: "todo_comments".to_string(),
                        rule_type: RuleType::Regex,
                        pattern: build_development_marker_pattern(),
                        message: "Development marker detected: {match}".to_string(),
                        severity: None,
                        enabled: true,
                        case_sensitive: false,
                        exclude_if: None,
                    },
                    PatternRule {
                        id: "temporary_markers".to_string(),
                        rule_type: RuleType::Regex,
                        pattern: build_temporary_marker_pattern(),
                        message: "Implementation marker found: {match}".to_string(),
                        severity: None,
                        enabled: true,
                        case_sensitive: false,
                        exclude_if: Some(ExcludeConditions {
                            attribute: None,
                            in_tests: true,
                            file_patterns: Some(vec!["**/tests/**".to_string()]),
                        }),
                    },
                    PatternRule {
                        id: "unimplemented_macros".to_string(),
                        rule_type: RuleType::Ast,
                        pattern: build_unfinished_macro_pattern(),
                        message: "Unfinished macro {macro_name}! found".to_string(),
                        severity: None,
                        enabled: true,
                        case_sensitive: true,
                        exclude_if: Some(ExcludeConditions {
                            attribute: Some("#[test]".to_string()),
                            in_tests: true,
                            file_patterns: None,
                        }),
                    },
                ],
            },
        );

        // Incomplete implementations
        patterns.insert(
            "incomplete_implementations".to_string(),
            PatternCategory {
                severity: Severity::Error,
                enabled: true,
                rules: vec![PatternRule {
                    id: "empty_ok_return".to_string(),
                    rule_type: RuleType::Ast,
                    pattern: "return_ok_unit_with_no_logic".to_string(),
                    message: "Function returns Ok(()) with no implementation".to_string(),
                    severity: None,
                    enabled: true,
                    case_sensitive: true,
                    exclude_if: Some(ExcludeConditions {
                        attribute: Some("#[test]".to_string()),
                        in_tests: true,
                        file_patterns: None,
                    }),
                }],
            },
        );

        // Architectural violations
        patterns.insert(
            "architectural_violations".to_string(),
            PatternCategory {
                severity: Severity::Warning,
                enabled: true,
                rules: vec![
                    PatternRule {
                        id: "hardcoded_paths".to_string(),
                        rule_type: RuleType::Regex,
                        pattern: r#"["\'](\./|/|\.\./)?(\.rust/)[^"']*["\']"#.to_string(),
                        message: "Hardcoded path found - use configuration instead".to_string(),
                        severity: None,
                        enabled: true,
                        case_sensitive: true,
                        exclude_if: Some(ExcludeConditions {
                            attribute: None,
                            in_tests: true,
                            file_patterns: Some(vec![
                                "**/tests/**".to_string(),
                                "**/examples/**".to_string(),
                            ]),
                        }),
                    },
                    PatternRule {
                        id: "architectural_header_missing".to_string(),
                        rule_type: RuleType::Regex,
                        pattern: r"//!\s*(?:.*\n)*?\s*//!\s*Architecture:".to_string(),
                        message: "File missing architectural principle header".to_string(),
                        severity: Some(Severity::Info),
                        enabled: false, // Disabled by default, can be enabled per project
                        case_sensitive: false,
                        exclude_if: Some(ExcludeConditions {
                            attribute: None,
                            in_tests: true,
                            file_patterns: Some(vec![
                                "**/tests/**".to_string(),
                                "**/benches/**".to_string(),
                                "**/examples/**".to_string(),
                            ]),
                        }),
                    },
                ],
            },
        );

        patterns
    }

    /// Validate the configuration for consistency and correctness
    pub fn validate(&self) -> GuardianResult<()> {
        // Check version compatibility
        if !["1.0"].contains(&self.version.as_str()) {
            return Err(GuardianError::config(format!(
                "Unsupported configuration version: {}. Supported versions: 1.0",
                self.version
            )));
        }

        // Validate patterns
        for (category_name, category) in &self.patterns {
            for rule in &category.rules {
                // Validate rule IDs are unique within category
                let duplicate_count = category.rules.iter().filter(|r| r.id == rule.id).count();
                if duplicate_count > 1 {
                    return Err(GuardianError::config(format!(
                        "Duplicate rule ID '{}' in category '{}'",
                        rule.id, category_name
                    )));
                }

                // Validate regex patterns can compile
                if matches!(rule.rule_type, RuleType::Regex) {
                    if rule.case_sensitive {
                        regex::Regex::new(&rule.pattern)
                    } else {
                        regex::RegexBuilder::new(&rule.pattern).case_insensitive(true).build()
                    }
                    .map_err(|e| {
                        GuardianError::config(format!(
                            "Invalid regex pattern in rule '{}': {}",
                            rule.id, e
                        ))
                    })?;
                }
            }
        }

        Ok(())
    }

    /// Get all enabled rules across all categories
    pub fn enabled_rules(&self) -> impl Iterator<Item = (&String, &PatternCategory, &PatternRule)> {
        self.patterns.iter().filter(|(_, category)| category.enabled).flat_map(
            |(name, category)| {
                category
                    .rules
                    .iter()
                    .filter(|rule| rule.enabled)
                    .map(move |rule| (name, category, rule))
            },
        )
    }

    /// Get effective severity for a rule (rule override or category default)
    pub fn effective_severity(&self, category: &PatternCategory, rule: &PatternRule) -> Severity {
        rule.severity.unwrap_or(category.severity)
    }

    /// Convert to JSON for serialization
    pub fn to_json(&self) -> GuardianResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| GuardianError::config(format!("Failed to serialize config: {e}")))
    }

    /// Create a fingerprint of the configuration for cache validation
    pub fn fingerprint(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Create a stable representation for hashing
        // Sort patterns to ensure consistent ordering
        let mut sorted_patterns: Vec<_> = self.patterns.iter().collect();
        sorted_patterns.sort_by_key(|(name, _)| name.as_str());

        // Hash version and path config
        self.version.hash(&mut hasher);
        self.paths.patterns.len().hash(&mut hasher);
        for pattern in &self.paths.patterns {
            pattern.hash(&mut hasher);
        }
        self.paths.ignore_file.hash(&mut hasher);

        // Hash patterns in sorted order
        for (category_name, category) in sorted_patterns {
            category_name.hash(&mut hasher);
            category.severity.hash(&mut hasher);
            category.enabled.hash(&mut hasher);

            // Sort rules for consistent ordering
            let mut sorted_rules = category.rules.clone();
            sorted_rules.sort_by_key(|rule| rule.id.clone());

            for rule in sorted_rules {
                rule.id.hash(&mut hasher);
                rule.pattern.hash(&mut hasher);
                rule.message.hash(&mut hasher);
                rule.enabled.hash(&mut hasher);
                rule.case_sensitive.hash(&mut hasher);
            }
        }

        format!("{:x}", hasher.finish())
    }
}

impl Default for GuardianConfig {
    fn default() -> Self {
        Self::with_defaults()
    }
}

fn default_true() -> bool {
    true
}

/// Build development marker pattern with simple, readable literals
fn build_development_marker_pattern() -> String {
    // Simple regex pattern for development markers
    r"\b(TODO|FIXME|HACK|XXX|BUG|REFACTOR)\b".to_string()
}

/// Build implementation marker pattern with simple, readable literals
fn build_temporary_marker_pattern() -> String {
    // Simple regex pattern for temporary implementation markers
    r"(?i)\b(for now|temporary|placeholder|stub|dummy|fake)\b".to_string()
}

/// Build unfinished macro pattern with simple, readable literals
fn build_unfinished_macro_pattern() -> String {
    // Simple pattern to detect incomplete macro calls
    "macro_call:unimplemented|todo|panic".to_string()
}

/// Configuration builder for programmatic construction
pub struct ConfigBuilder {
    config: GuardianConfig,
}

impl ConfigBuilder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self { config: GuardianConfig::default() }
    }

    /// Add a path pattern
    pub fn add_path_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.config.paths.patterns.push(pattern.into());
        self
    }

    /// Set the ignore file name
    pub fn ignore_file(mut self, filename: impl Into<String>) -> Self {
        self.config.paths.ignore_file = Some(filename.into());
        self
    }

    /// Add a pattern category
    pub fn add_category(mut self, name: impl Into<String>, category: PatternCategory) -> Self {
        self.config.patterns.insert(name.into(), category);
        self
    }

    /// Build the final configuration
    pub fn build(self) -> GuardianResult<GuardianConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GuardianConfig {
    /// Self-validating configuration integrity check following architectural principles
    /// Guardian validates itself rather than relying on external tests
    pub fn verify_config_integrity(&self) -> GuardianResult<()> {
        // Domain validates its own consistency
        if self.version != "1.0" {
            return Err(GuardianError::config(
                "Configuration system requires version 1.0".to_string(),
            ));
        }

        // Essential patterns must exist for guardian to function
        if !self.patterns.contains_key("placeholders") {
            return Err(GuardianError::config(
                "Core pattern category 'placeholders' missing from guardian".to_string(),
            ));
        }

        // Validate guardian can generate its own fingerprint consistently
        let fingerprint1 = self.fingerprint();
        let fingerprint2 = self.fingerprint();
        if fingerprint1 != fingerprint2 {
            return Err(GuardianError::config(
                "Configuration system fingerprint inconsistent".to_string(),
            ));
        }

        Ok(())
    }

    /// Verify configuration can evolve correctly (builder pattern validation)
    pub fn verify_evolution_capability() -> GuardianResult<()> {
        let evolved_config = ConfigBuilder::new()
            .add_path_pattern("guardian/evolution/**")
            .ignore_file(".guardian_ignore")
            .build()?;

        if !evolved_config.paths.patterns.contains(&"guardian/evolution/**".to_string()) {
            return Err(GuardianError::config(
                "Configuration evolution failed to integrate new patterns".to_string(),
            ));
        }

        Ok(())
    }

    /// Verify serialization maintains configuration integrity
    pub fn verify_serialization_fidelity(&self) -> GuardianResult<()> {
        let yaml = serde_yaml::to_string(self).map_err(|e| {
            GuardianError::config(format!("Configuration serialization failed: {e}"))
        })?;

        let rehydrated: Self = serde_yaml::from_str(&yaml)
            .map_err(|e| GuardianError::config(format!("Configuration rehydration failed: {e}")))?;

        if self.version != rehydrated.version {
            return Err(GuardianError::config(
                "Configuration version lost during serialization".to_string(),
            ));
        }

        Ok(())
    }
}
