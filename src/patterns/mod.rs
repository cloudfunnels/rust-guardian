//! Pattern engine for detecting code quality violations
//!
//! Architectural Principle: Service Layer - Pattern matching orchestrates complex analysis operations
//! - PatternEngine coordinates different types of pattern matching (regex, AST, semantic)
//! - Each pattern type implements the PatternMatcher trait for clean polymorphism
//! - Pattern results are translated to quality violations at the boundary

pub mod path_filter;

use crate::config::{ExcludeConditions, PatternRule, RuleType};
use crate::domain::violations::{GuardianError, GuardianResult, Severity, Violation};
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;

pub use path_filter::PathFilter;

/// Core pattern engine that coordinates different types of pattern matching
#[derive(Debug)]
pub struct PatternEngine {
    /// Compiled regex patterns for fast matching
    regex_patterns: HashMap<String, CompiledRegex>,
    /// AST patterns for semantic analysis
    ast_patterns: HashMap<String, AstPattern>,
}

/// A compiled regex pattern with metadata
#[derive(Debug)]
struct CompiledRegex {
    regex: Regex,
    rule_id: String,
    message_template: String,
    severity: Severity,
    exclude_conditions: Option<ExcludeConditions>,
}

/// An AST pattern for structural code analysis
#[derive(Debug)]
struct AstPattern {
    pattern_type: AstPatternType,
    rule_id: String,
    message_template: String,
    severity: Severity,
    exclude_conditions: Option<ExcludeConditions>,
}

/// Types of AST patterns we can detect
#[derive(Debug, Clone)]
enum AstPatternType {
    /// Look for specific macro calls (unimplemented!, todo!, panic!)
    MacroCall(Vec<String>),
    /// Look for functions that return Ok(()) with no meaningful implementation
    EmptyOkReturn,
    /// Look for missing architectural headers in files
    MissingArchitecturalHeader,
    /// Look for functions with empty bodies
    EmptyFunctionBody,
    /// Look for unwrap() or expect() calls without meaningful error messages
    UnwrapOrExpectWithoutMessage,
    /// Look for abstraction layer violations (semantic pattern)
    AbstractionLayerViolation(regex::Regex),
    /// Advanced semantic patterns
    CyclomaticComplexity(u32),
    PublicWithoutDocs,
    FunctionLinesGt(u32),
    NestingDepthGt(u32),
    FunctionArgsGt(u32),
    BlockingCallInAsync,
    FutureNotAwaited,
    SelectWithoutBiased,
    GenericWithoutBounds,
    TestFnWithoutAssertion,
    ImplWithoutTrait,
    UnsafeBlock,
    IgnoredTestAttribute,
}

/// A match found by a pattern
#[derive(Debug)]
pub struct PatternMatch {
    pub rule_id: String,
    pub file_path: PathBuf,
    pub line_number: Option<u32>,
    pub column_number: Option<u32>,
    pub matched_text: String,
    pub message: String,
    pub severity: Severity,
    pub context: Option<String>,
}

impl PatternEngine {
    /// Create a new pattern engine
    pub fn new() -> Self {
        Self { regex_patterns: HashMap::new(), ast_patterns: HashMap::new() }
    }

    /// Add a pattern rule to the engine
    pub fn add_rule(
        &mut self,
        rule: &PatternRule,
        effective_severity: Severity,
    ) -> GuardianResult<()> {
        tracing::debug!(
            "Adding rule '{}' of type {:?} with pattern '{}' and severity {:?}",
            rule.id,
            rule.rule_type,
            rule.pattern,
            effective_severity
        );

        match rule.rule_type {
            RuleType::Regex => {
                tracing::debug!(
                    "Compiling regex pattern '{}' for rule '{}'",
                    rule.pattern,
                    rule.id
                );
                let regex = if rule.case_sensitive {
                    Regex::new(&rule.pattern)
                } else {
                    RegexBuilder::new(&rule.pattern).case_insensitive(true).build()
                }
                .map_err(|e| {
                    GuardianError::pattern(format!("Invalid regex '{}': {}", rule.pattern, e))
                })?;

                self.regex_patterns.insert(
                    rule.id.clone(),
                    CompiledRegex {
                        regex,
                        rule_id: rule.id.clone(),
                        message_template: rule.message.clone(),
                        severity: effective_severity,
                        exclude_conditions: rule.exclude_if.clone(),
                    },
                );
            }
            RuleType::Ast => {
                let pattern_type = self.parse_ast_pattern(&rule.pattern, &rule.id)?;

                self.ast_patterns.insert(
                    rule.id.clone(),
                    AstPattern {
                        pattern_type,
                        rule_id: rule.id.clone(),
                        message_template: rule.message.clone(),
                        severity: effective_severity,
                        exclude_conditions: rule.exclude_if.clone(),
                    },
                );
            }
            RuleType::Semantic | RuleType::ImportAnalysis => {
                let pattern_type = self.parse_semantic_pattern(&rule.pattern, &rule.id)?;

                self.ast_patterns.insert(
                    rule.id.clone(),
                    AstPattern {
                        pattern_type,
                        rule_id: rule.id.clone(),
                        message_template: rule.message.clone(),
                        severity: effective_severity,
                        exclude_conditions: rule.exclude_if.clone(),
                    },
                );
            }
        }

        Ok(())
    }

    /// Parse AST pattern string into typed pattern
    fn parse_ast_pattern(&self, pattern: &str, rule_id: &str) -> GuardianResult<AstPatternType> {
        if pattern.starts_with("macro_call:") {
            let macros = pattern
                .strip_prefix("macro_call:")
                .expect("pattern starts with 'macro_call:' - prefix strip should not fail")
                .split('|')
                .map(|s| s.trim().to_string())
                .collect();
            Ok(AstPatternType::MacroCall(macros))
        } else if pattern == "return_ok_unit_with_no_logic" {
            Ok(AstPatternType::EmptyOkReturn)
        } else if pattern.contains("Architectural Principle:") {
            Ok(AstPatternType::MissingArchitecturalHeader)
        } else if pattern == "empty_function_body" {
            Ok(AstPatternType::EmptyFunctionBody)
        } else if pattern == "unwrap_or_expect_without_message" {
            Ok(AstPatternType::UnwrapOrExpectWithoutMessage)
        } else if pattern == "unsafe_block" {
            Ok(AstPatternType::UnsafeBlock)
        } else if pattern == "ignored_test_attribute" {
            Ok(AstPatternType::IgnoredTestAttribute)
        } else {
            Err(GuardianError::pattern(format!(
                "Unknown AST pattern type in rule '{rule_id}': {pattern}"
            )))
        }
    }

    /// Parse semantic pattern string into typed pattern
    fn parse_semantic_pattern(
        &self,
        pattern: &str,
        rule_id: &str,
    ) -> GuardianResult<AstPatternType> {
        // Handle parametric patterns first
        if let Some(param) = pattern.strip_prefix("cyclomatic_complexity_gt:") {
            let threshold = param.parse::<u32>().map_err(|_| {
                GuardianError::pattern(format!("Invalid threshold in rule '{rule_id}': {param}"))
            })?;
            return Ok(AstPatternType::CyclomaticComplexity(threshold));
        }

        if let Some(param) = pattern.strip_prefix("function_lines_gt:") {
            let threshold = param.parse::<u32>().map_err(|_| {
                GuardianError::pattern(format!("Invalid threshold in rule '{rule_id}': {param}"))
            })?;
            return Ok(AstPatternType::FunctionLinesGt(threshold));
        }

        if let Some(param) = pattern.strip_prefix("nesting_depth_gt:") {
            let threshold = param.parse::<u32>().map_err(|_| {
                GuardianError::pattern(format!("Invalid threshold in rule '{rule_id}': {param}"))
            })?;
            return Ok(AstPatternType::NestingDepthGt(threshold));
        }

        if let Some(param) = pattern.strip_prefix("function_args_gt:") {
            let threshold = param.parse::<u32>().map_err(|_| {
                GuardianError::pattern(format!("Invalid threshold in rule '{rule_id}': {param}"))
            })?;
            return Ok(AstPatternType::FunctionArgsGt(threshold));
        }

        // Handle non-parametric semantic patterns
        match pattern {
            "public_without_docs" => Ok(AstPatternType::PublicWithoutDocs),
            "blocking_call_in_async" => Ok(AstPatternType::BlockingCallInAsync),
            "future_not_awaited" => Ok(AstPatternType::FutureNotAwaited),
            "select_without_biased" => Ok(AstPatternType::SelectWithoutBiased),
            "generic_without_bounds" => Ok(AstPatternType::GenericWithoutBounds),
            "test_fn_without_assertion" => Ok(AstPatternType::TestFnWithoutAssertion),
            "impl_without_trait" => Ok(AstPatternType::ImplWithoutTrait),
            _ => {
                // For unrecognized semantic patterns, check if they look like import patterns
                // This allows users to define custom layering import checking
                if pattern.starts_with("use")
                    || pattern.starts_with("import:")
                    || pattern.contains("_access")
                {
                    // Handle patterns like "use.*concrete" or "use.*implementation"
                    let import_pattern = if pattern.starts_with("use") {
                        pattern.to_string()
                    } else if pattern.starts_with("import:") {
                        pattern.replace("import:", "")
                    } else {
                        // Handle patterns like "direct_X_access"
                        format!(
                            r"use\s+.*{}",
                            pattern.replace("direct_", "").replace("_access", "")
                        )
                    };

                    if let Ok(regex) = regex::Regex::new(&import_pattern) {
                        Ok(AstPatternType::AbstractionLayerViolation(regex))
                    } else {
                        Err(GuardianError::pattern(format!(
                            "Invalid import pattern in rule '{rule_id}': {pattern}"
                        )))
                    }
                } else {
                    // Unknown pattern - could be a future extension
                    Err(GuardianError::pattern(format!(
                        "Unknown semantic pattern type in rule '{rule_id}': {pattern}"
                    )))
                }
            }
        }
    }

    /// Analyze a file and return all pattern matches
    pub fn analyze_file<P: AsRef<Path>>(
        &self,
        file_path: P,
        content: &str,
    ) -> GuardianResult<Vec<PatternMatch>> {
        let file_path = file_path.as_ref();
        let mut matches = Vec::new();

        tracing::debug!(
            "Analyzing file '{}' with {} regex patterns and {} AST patterns",
            file_path.display(),
            self.regex_patterns.len(),
            self.ast_patterns.len()
        );

        // Apply regex patterns
        for pattern in self.regex_patterns.values() {
            tracing::debug!("Processing regex pattern '{}'", pattern.rule_id);
            let pattern_matches = self.apply_regex_pattern(pattern, file_path, content)?;
            tracing::debug!(
                "Pattern '{}' found {} matches",
                pattern.rule_id,
                pattern_matches.len()
            );
            matches.extend(pattern_matches);
        }

        // Apply AST patterns for Rust files
        if file_path.extension().and_then(|s| s.to_str()) == Some("rs") {
            for pattern in self.ast_patterns.values() {
                let pattern_matches = self.apply_ast_pattern(pattern, file_path, content)?;
                matches.extend(pattern_matches);
            }
        }

        Ok(matches)
    }

    /// Apply a regex pattern to file content
    fn apply_regex_pattern(
        &self,
        pattern: &CompiledRegex,
        file_path: &Path,
        content: &str,
    ) -> GuardianResult<Vec<PatternMatch>> {
        tracing::debug!(
            "Applying regex pattern '{}' to file '{}'",
            pattern.rule_id,
            file_path.display()
        );
        tracing::debug!("Pattern regex: '{}'", pattern.regex.as_str());
        tracing::debug!("Content length: {} characters", content.len());

        let mut matches = Vec::new();

        // Find all matches in the content
        for regex_match in pattern.regex.find_iter(content) {
            tracing::debug!(
                "Found regex match: '{}' at offset {}",
                regex_match.as_str(),
                regex_match.start()
            );
            let matched_text = regex_match.as_str().to_string();
            let (line_num, col_num, context) =
                self.get_match_location(content, regex_match.start());

            // Check exclude conditions
            if self.should_exclude_match(
                pattern.exclude_conditions.as_ref(),
                file_path,
                &matched_text,
                content,
                regex_match.start(),
            ) {
                tracing::debug!("Match '{}' excluded by conditions", matched_text);
                continue;
            }

            let message = pattern.message_template.replace("{match}", &matched_text);

            matches.push(PatternMatch {
                rule_id: pattern.rule_id.clone(),
                file_path: file_path.to_path_buf(),
                line_number: Some(line_num),
                column_number: Some(col_num),
                matched_text,
                message,
                severity: pattern.severity,
                context: Some(context),
            });
        }

        Ok(matches)
    }

    /// Apply an AST pattern to Rust source code
    fn apply_ast_pattern(
        &self,
        pattern: &AstPattern,
        file_path: &Path,
        content: &str,
    ) -> GuardianResult<Vec<PatternMatch>> {
        let mut matches = Vec::new();

        // Parse Rust syntax
        let syntax_tree = match syn::parse_file(content) {
            Ok(tree) => tree,
            Err(e) => {
                // If we can't parse the file, skip AST analysis but don't fail
                tracing::debug!("Failed to parse Rust file {}: {}", file_path.display(), e);
                return Ok(matches);
            }
        };

        match &pattern.pattern_type {
            AstPatternType::MacroCall(macro_names) => {
                let found_matches = self.find_macro_calls(&syntax_tree, macro_names);
                for (line, col, macro_name, context) in found_matches {
                    // Check exclude conditions
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    let message = pattern.message_template.replace("{macro_name}", &macro_name);

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("{macro_name}!()"),
                        message,
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::CyclomaticComplexity(threshold) => {
                let found_matches = self.find_cyclomatic_complexity(&syntax_tree, *threshold);
                for (line, col, fn_name, complexity, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    let message =
                        pattern.message_template.replace("{value}", &complexity.to_string());

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("fn {}", fn_name),
                        message,
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::PublicWithoutDocs => {
                let found_matches = self.find_public_without_docs(&syntax_tree);
                for (line, col, item_name, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: item_name,
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::FunctionLinesGt(threshold) => {
                let found_matches = self.find_long_functions(&syntax_tree, content, *threshold);
                for (line, col, fn_name, line_count, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    let message =
                        pattern.message_template.replace("{lines}", &line_count.to_string());

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("fn {}", fn_name),
                        message,
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::NestingDepthGt(threshold) => {
                let found_matches = self.find_deep_nesting(&syntax_tree, *threshold);
                for (line, col, depth, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    let message = pattern.message_template.replace("{depth}", &depth.to_string());

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: "nested block".to_string(),
                        message,
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::FunctionArgsGt(threshold) => {
                let found_matches = self.find_functions_with_many_args(&syntax_tree, *threshold);
                for (line, col, fn_name, arg_count, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    let message =
                        pattern.message_template.replace("{count}", &arg_count.to_string());

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("fn {}", fn_name),
                        message,
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::BlockingCallInAsync => {
                let found_matches = self.find_blocking_in_async(&syntax_tree);
                for (line, col, call_name, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: call_name,
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::FutureNotAwaited => {
                let found_matches = self.find_futures_not_awaited(&syntax_tree);
                for (line, col, expr, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: expr,
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::SelectWithoutBiased => {
                let found_matches = self.find_select_without_biased(&syntax_tree);
                for (line, col, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: "tokio::select!".to_string(),
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::GenericWithoutBounds => {
                let found_matches = self.find_generics_without_bounds(&syntax_tree);
                for (line, col, generic_name, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: generic_name,
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::TestFnWithoutAssertion => {
                let found_matches = self.find_test_functions_without_assertions(&syntax_tree);
                for (line, col, fn_name, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("fn {}", fn_name),
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::ImplWithoutTrait => {
                let found_matches = self.find_impl_without_trait(&syntax_tree);
                for (line, col, impl_name, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("impl {}", impl_name),
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::UnsafeBlock => {
                let found_matches = self.find_unsafe_blocks(&syntax_tree);
                for (line, col, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: "unsafe".to_string(),
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::IgnoredTestAttribute => {
                let found_matches = self.find_ignored_tests(&syntax_tree);
                for (line, col, fn_name, context) in found_matches {
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("#[ignore] fn {}", fn_name),
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }

            AstPatternType::EmptyOkReturn => {
                let found_matches = self.find_empty_ok_returns(&syntax_tree);
                for (line, col, context) in found_matches {
                    // Check exclude conditions
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: "Ok(())".to_string(),
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::MissingArchitecturalHeader => {
                if !content.contains("Architectural Principle:") {
                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(1),
                        column_number: Some(1),
                        matched_text: "".to_string(),
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: None,
                    });
                }
            }
            AstPatternType::EmptyFunctionBody => {
                let found_matches = self.find_empty_function_bodies(&syntax_tree);
                for (line, col, fn_name, context) in found_matches {
                    // Check exclude conditions
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    let message = pattern.message_template.replace("{function_name}", &fn_name);

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("fn {}", fn_name),
                        message,
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::UnwrapOrExpectWithoutMessage => {
                let found_matches = self.find_unwrap_without_message(&syntax_tree);
                for (line, col, method_name, context) in found_matches {
                    // Check exclude conditions
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    let message = pattern.message_template.replace("{method}", &method_name);

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!(".{}()", method_name),
                        message,
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
            AstPatternType::AbstractionLayerViolation(regex) => {
                let found_matches = self.find_import_pattern_matches(&syntax_tree, content, regex);
                for (line, col, import_text, context) in found_matches {
                    // Check exclude conditions
                    if self.should_exclude_ast_match(
                        pattern.exclude_conditions.as_ref(),
                        file_path,
                        &syntax_tree,
                        line,
                    ) {
                        continue;
                    }

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: import_text,
                        message: pattern.message_template.clone(),
                        severity: pattern.severity,
                        context: Some(context),
                    });
                }
            }
        }

        Ok(matches)
    }

    /// Find macro calls in the syntax tree
    fn find_macro_calls(
        &self,
        syntax_tree: &syn::File,
        target_macros: &[String],
    ) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct MacroVisitor<'a> {
            target_macros: &'a [String],
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for MacroVisitor<'_> {
            fn visit_macro(&mut self, mac: &syn::Macro) {
                if let Some(ident) = mac.path.get_ident() {
                    let macro_name = ident.to_string();
                    if self.target_macros.contains(&macro_name) {
                        let _span = mac.path.span();
                        // proc_macro2::Span doesn't provide direct line/column access in stable Rust
                        // Use line 1 with improved context for macro location
                        let context = format!("{}!()", macro_name);
                        self.matches.push((1, 1, macro_name, context));
                    }
                }
                syn::visit::visit_macro(self, mac);
            }
        }

        let mut visitor = MacroVisitor { target_macros, matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }
    /// Find functions that return empty Ok(()) responses
    fn find_empty_ok_returns(&self, syntax_tree: &syn::File) -> Vec<(u32, u32, String)> {
        use syn::visit::Visit;

        struct EmptyOkVisitor {
            matches: Vec<(u32, u32, String)>,
        }

        impl Visit<'_> for EmptyOkVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                // Check if function returns Result type
                if let syn::ReturnType::Type(_, return_type) = &func.sig.output {
                    if self.is_result_type(return_type) {
                        // Check if body is just Ok(()) or similar
                        if let Some(ok_expr) = self.find_ok_unit_return(&func.block) {
                            let _span = ok_expr.span();
                            // Use a simple line-based location since proc_macro2::Span doesn't have start() method
                            // Use a simple line-based location since proc_macro2::Span doesn't have start() method
                            let (line, col, context) = (1, 1, String::new());
                            self.matches.push((line, col, context));
                        }
                    }
                }
                syn::visit::visit_item_fn(self, func);
            }
        }

        impl EmptyOkVisitor {
            fn is_result_type(&self, ty: &syn::Type) -> bool {
                match ty {
                    syn::Type::Path(type_path) => type_path
                        .path
                        .segments
                        .last()
                        .map(|seg| seg.ident == "Result")
                        .unwrap_or(false),
                    _ => false,
                }
            }

            fn find_ok_unit_return<'b>(&self, block: &'b syn::Block) -> Option<&'b syn::Expr> {
                // Look for a block with just one statement that returns Ok(())
                if block.stmts.len() == 1 {
                    if let syn::Stmt::Expr(expr, _) = &block.stmts[0] {
                        if self.is_ok_unit_expr(expr) {
                            return Some(expr);
                        }
                    }
                }
                None
            }

            fn is_ok_unit_expr(&self, expr: &syn::Expr) -> bool {
                if let syn::Expr::Call(call) = expr {
                    // Check if it's Ok(())
                    if let syn::Expr::Path(path) = &*call.func {
                        if path.path.segments.last().map(|seg| seg.ident == "Ok").unwrap_or(false) {
                            // Check if argument is unit type ()
                            if call.args.len() == 1 {
                                if let syn::Expr::Tuple(tuple) = &call.args[0] {
                                    return tuple.elems.is_empty();
                                }
                            }
                        }
                    }
                }
                false
            }
        }

        let mut visitor = EmptyOkVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find functions with empty bodies
    fn find_empty_function_bodies(
        &self,
        syntax_tree: &syn::File,
    ) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct EmptyBodyVisitor {
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for EmptyBodyVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                let fn_name = func.sig.ident.to_string();

                // Check if function body is empty or has only comments/whitespace
                if func.block.stmts.is_empty() {
                    // Function has completely empty body
                    let (line, col, context) = (1, 1, format!("fn {} {{ }}", fn_name));
                    self.matches.push((line, col, fn_name, context));
                } else if func.block.stmts.len() == 1 {
                    // Check if the single statement is just a comment or empty expression
                    if let syn::Stmt::Expr(expr, _) = &func.block.stmts[0] {
                        if matches!(expr, syn::Expr::Tuple(tuple) if tuple.elems.is_empty()) {
                            // Function body contains only ()
                            let (line, col, context) = (1, 1, format!("fn {} {{ () }}", fn_name));
                            self.matches.push((line, col, fn_name, context));
                        }
                    }
                }

                syn::visit::visit_item_fn(self, func);
            }
        }

        let mut visitor = EmptyBodyVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find unwrap() or expect() calls without meaningful error messages
    fn find_unwrap_without_message(
        &self,
        syntax_tree: &syn::File,
    ) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct UnwrapVisitor {
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for UnwrapVisitor {
            fn visit_expr_method_call(&mut self, method_call: &syn::ExprMethodCall) {
                let method_name = method_call.method.to_string();

                match method_name.as_str() {
                    "unwrap" => {
                        // unwrap() calls are always problematic
                        let (line, col, context) = (1, 1, ".unwrap()".to_string());
                        self.matches.push((line, col, "unwrap".to_string(), context));
                    }
                    "expect" => {
                        // Check if expect() has a meaningful message
                        if method_call.args.is_empty() {
                            // expect() without any message
                            let (line, col, context) = (1, 1, ".expect()".to_string());
                            self.matches.push((line, col, "expect".to_string(), context));
                        } else if let syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(lit_str),
                            ..
                        }) = &method_call.args[0]
                        {
                            let message = lit_str.value();
                            // Check for generic/unhelpful messages
                            if message.is_empty()
                                || message.len() < 5
                                || message.to_lowercase().contains("error") && message.len() < 10
                            {
                                let (line, col, context) =
                                    (1, 1, format!(".expect(\"{}\")", message));
                                self.matches.push((line, col, "expect".to_string(), context));
                            }
                        }
                    }
                    _ => {}
                }

                syn::visit::visit_expr_method_call(self, method_call);
            }
        }

        let mut visitor = UnwrapVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find import patterns using regex matching on use statements
    fn find_import_pattern_matches(
        &self,
        syntax_tree: &syn::File,
        _content: &str,
        regex: &regex::Regex,
    ) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct ImportVisitor<'a> {
            regex: &'a regex::Regex,
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for ImportVisitor<'_> {
            fn visit_item_use(&mut self, use_item: &syn::ItemUse) {
                // Convert the use statement back to string for regex matching
                let use_string = format!(
                    "use {};",
                    quote::quote!(#use_item).to_string().trim_start_matches("use ")
                );

                if self.regex.is_match(&use_string) {
                    // Extract line information from the use statement
                    // Use simple line tracking for AST span location
                    // we'd use syn span information for precise location
                    let (line, col, context) = (1, 1, use_string.clone());
                    self.matches.push((line, col, use_string, context));
                }

                syn::visit::visit_item_use(self, use_item);
            }
        }

        let mut visitor = ImportVisitor { regex, matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Get line and column number from byte offset in content
    fn get_match_location(&self, content: &str, byte_offset: usize) -> (u32, u32, String) {
        let mut line = 1;
        let mut col = 1;
        let mut line_start = 0;

        for (i, ch) in content.char_indices() {
            if i >= byte_offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 1;
                line_start = i + 1;
            } else {
                col += 1;
            }
        }

        // Extract context line
        let line_end =
            content[line_start..].find('\n').map(|pos| line_start + pos).unwrap_or(content.len());

        let context = content[line_start..line_end].trim().to_string();

        (line, col, context)
    }

    /// Check if a regex match should be excluded based on conditions
    fn should_exclude_match(
        &self,
        conditions: Option<&ExcludeConditions>,
        file_path: &Path,
        matched_text: &str,
        _content: &str,
        _offset: usize,
    ) -> bool {
        if let Some(conditions) = conditions {
            tracing::debug!(
                "Checking exclude conditions for match '{}' in file '{}'",
                matched_text,
                file_path.display()
            );

            // Check if in test files
            if conditions.in_tests && self.is_test_file(file_path) {
                tracing::debug!("Match excluded: in_tests=true and file is test file");
                return true;
            }

            // Check file patterns
            if let Some(patterns) = &conditions.file_patterns {
                for pattern in patterns {
                    if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                        if glob_pattern.matches_path(file_path) {
                            tracing::debug!("Match excluded: file matches pattern '{}'", pattern);
                            return true;
                        }
                    }
                }
            }

            // Additional condition checks can be added here when AST context is available
            tracing::debug!("Match not excluded by any conditions");
        } else {
            tracing::debug!("No exclude conditions to check");
        }

        false
    }

    /// Check if an AST match should be excluded
    fn should_exclude_ast_match(
        &self,
        conditions: Option<&ExcludeConditions>,
        file_path: &Path,
        _syntax_tree: &syn::File,
        _line: u32,
    ) -> bool {
        if let Some(conditions) = conditions {
            // Check if in test files
            if conditions.in_tests && self.is_test_file(file_path) {
                return true;
            }

            // Check file patterns
            if let Some(patterns) = &conditions.file_patterns {
                for pattern in patterns {
                    if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                        if glob_pattern.matches_path(file_path) {
                            return true;
                        }
                    }
                }
            }

            // Future enhancement: Check for specific attributes like #[test] on functions
        }

        false
    }

    /// Check if a file path indicates it's a test file
    fn is_test_file(&self, file_path: &Path) -> bool {
        file_path.components().any(|component| {
            component.as_os_str().to_str().map(|s| s == "tests" || s == "test").unwrap_or(false)
        }) || file_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.contains("test") || name.starts_with("test_"))
            .unwrap_or(false)
    }

    /// Find functions with high cyclomatic complexity
    fn find_cyclomatic_complexity(
        &self,
        syntax_tree: &syn::File,
        threshold: u32,
    ) -> Vec<(u32, u32, String, u32, String)> {
        use syn::visit::Visit;

        struct ComplexityVisitor {
            threshold: u32,
            matches: Vec<(u32, u32, String, u32, String)>,
        }

        impl Visit<'_> for ComplexityVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                let fn_name = func.sig.ident.to_string();
                let complexity = self.calculate_complexity(&func.block);

                if complexity > self.threshold {
                    let (line, col, context) =
                        (1, 1, format!("fn {} (complexity: {})", fn_name, complexity));
                    self.matches.push((line, col, fn_name, complexity, context));
                }

                syn::visit::visit_item_fn(self, func);
            }
        }

        impl ComplexityVisitor {
            fn calculate_complexity(&self, block: &syn::Block) -> u32 {
                use syn::visit::Visit;

                struct ComplexityCalculator {
                    complexity: u32,
                }

                impl Visit<'_> for ComplexityCalculator {
                    fn visit_expr_if(&mut self, expr: &syn::ExprIf) {
                        self.complexity += 1;
                        syn::visit::visit_expr_if(self, expr);
                    }

                    fn visit_expr_while(&mut self, expr: &syn::ExprWhile) {
                        self.complexity += 1;
                        syn::visit::visit_expr_while(self, expr);
                    }

                    fn visit_expr_for_loop(&mut self, expr: &syn::ExprForLoop) {
                        self.complexity += 1;
                        syn::visit::visit_expr_for_loop(self, expr);
                    }

                    fn visit_expr_loop(&mut self, expr: &syn::ExprLoop) {
                        self.complexity += 1;
                        syn::visit::visit_expr_loop(self, expr);
                    }

                    fn visit_expr_match(&mut self, expr_match: &syn::ExprMatch) {
                        self.complexity += expr_match.arms.len() as u32;
                        syn::visit::visit_expr_match(self, expr_match);
                    }

                    fn visit_expr_method_call(&mut self, method_call: &syn::ExprMethodCall) {
                        // Check for ? operator (error propagation)
                        if let syn::Expr::Try(_) = &*method_call.receiver {
                            self.complexity += 1;
                        }
                        syn::visit::visit_expr_method_call(self, method_call);
                    }
                }

                let mut calculator = ComplexityCalculator { complexity: 1 }; // Base complexity
                calculator.visit_block(block);
                calculator.complexity
            }
        }

        let mut visitor = ComplexityVisitor { threshold, matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find public items without documentation
    fn find_public_without_docs(&self, syntax_tree: &syn::File) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct PublicDocsVisitor {
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for PublicDocsVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                if matches!(func.vis, syn::Visibility::Public(_))
                    && !self.has_doc_comment(&func.attrs)
                {
                    let fn_name = func.sig.ident.to_string();
                    let (line, col, context) = (1, 1, format!("pub fn {}", fn_name));
                    self.matches.push((line, col, format!("fn {}", fn_name), context));
                }
                syn::visit::visit_item_fn(self, func);
            }

            fn visit_item_struct(&mut self, item_struct: &syn::ItemStruct) {
                if matches!(item_struct.vis, syn::Visibility::Public(_))
                    && !self.has_doc_comment(&item_struct.attrs)
                {
                    let struct_name = item_struct.ident.to_string();
                    let (line, col, context) = (1, 1, format!("pub struct {}", struct_name));
                    self.matches.push((line, col, format!("struct {}", struct_name), context));
                }
                syn::visit::visit_item_struct(self, item_struct);
            }

            fn visit_item_enum(&mut self, item_enum: &syn::ItemEnum) {
                if matches!(item_enum.vis, syn::Visibility::Public(_))
                    && !self.has_doc_comment(&item_enum.attrs)
                {
                    let enum_name = item_enum.ident.to_string();
                    let (line, col, context) = (1, 1, format!("pub enum {}", enum_name));
                    self.matches.push((line, col, format!("enum {}", enum_name), context));
                }
                syn::visit::visit_item_enum(self, item_enum);
            }

            fn visit_item_trait(&mut self, item_trait: &syn::ItemTrait) {
                if matches!(item_trait.vis, syn::Visibility::Public(_))
                    && !self.has_doc_comment(&item_trait.attrs)
                {
                    let trait_name = item_trait.ident.to_string();
                    let (line, col, context) = (1, 1, format!("pub trait {}", trait_name));
                    self.matches.push((line, col, format!("trait {}", trait_name), context));
                }
                syn::visit::visit_item_trait(self, item_trait);
            }
        }

        impl PublicDocsVisitor {
            fn has_doc_comment(&self, attrs: &[syn::Attribute]) -> bool {
                attrs.iter().any(|attr| {
                    attr.path().is_ident("doc")
                        || (attr.path().segments.len() == 1
                            && attr
                                .path()
                                .segments
                                .first()
                                .expect("segments.len() == 1 - first element must exist")
                                .ident
                                == "doc")
                })
            }
        }

        let mut visitor = PublicDocsVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find functions that are too long
    fn find_long_functions(
        &self,
        syntax_tree: &syn::File,
        _content: &str,
        threshold: u32,
    ) -> Vec<(u32, u32, String, u32, String)> {
        use syn::visit::Visit;

        struct LongFunctionVisitor {
            threshold: u32,
            matches: Vec<(u32, u32, String, u32, String)>,
        }

        impl Visit<'_> for LongFunctionVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                let fn_name = func.sig.ident.to_string();

                // Calculate function line count
                let line_count = self.count_function_lines(&func.block);

                if line_count > self.threshold {
                    let (line, col, context) =
                        (1, 1, format!("fn {} ({} lines)", fn_name, line_count));
                    self.matches.push((line, col, fn_name, line_count, context));
                }

                syn::visit::visit_item_fn(self, func);
            }
        }

        impl LongFunctionVisitor {
            fn count_function_lines(&self, block: &syn::Block) -> u32 {
                // Simple line counting - count non-empty, non-comment lines
                let block_str = format!("{}", quote::quote!(#block));
                block_str
                    .lines()
                    .filter(|line| !line.trim().is_empty() && !line.trim().starts_with("//"))
                    .count() as u32
            }
        }

        let mut visitor = LongFunctionVisitor { threshold, matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find code with deep nesting
    fn find_deep_nesting(
        &self,
        syntax_tree: &syn::File,
        threshold: u32,
    ) -> Vec<(u32, u32, u32, String)> {
        use syn::visit::Visit;

        struct NestingVisitor {
            threshold: u32,
            current_depth: u32,
            matches: Vec<(u32, u32, u32, String)>,
        }

        impl Visit<'_> for NestingVisitor {
            fn visit_block(&mut self, block: &syn::Block) {
                self.current_depth += 1;

                if self.current_depth > self.threshold {
                    let (line, col, context) =
                        (1, 1, format!("nested block at depth {}", self.current_depth));
                    self.matches.push((line, col, self.current_depth, context));
                }

                syn::visit::visit_block(self, block);
                self.current_depth -= 1;
            }

            fn visit_expr_if(&mut self, expr_if: &syn::ExprIf) {
                self.current_depth += 1;

                if self.current_depth > self.threshold {
                    let (line, col, context) =
                        (1, 1, format!("if statement at depth {}", self.current_depth));
                    self.matches.push((line, col, self.current_depth, context));
                }

                syn::visit::visit_expr_if(self, expr_if);
                self.current_depth -= 1;
            }

            fn visit_expr_match(&mut self, expr_match: &syn::ExprMatch) {
                self.current_depth += 1;

                if self.current_depth > self.threshold {
                    let (line, col, context) =
                        (1, 1, format!("match statement at depth {}", self.current_depth));
                    self.matches.push((line, col, self.current_depth, context));
                }

                syn::visit::visit_expr_match(self, expr_match);
                self.current_depth -= 1;
            }
        }

        let mut visitor = NestingVisitor { threshold, current_depth: 0, matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find functions with too many arguments
    fn find_functions_with_many_args(
        &self,
        syntax_tree: &syn::File,
        threshold: u32,
    ) -> Vec<(u32, u32, String, u32, String)> {
        use syn::visit::Visit;

        struct ManyArgsVisitor {
            threshold: u32,
            matches: Vec<(u32, u32, String, u32, String)>,
        }

        impl Visit<'_> for ManyArgsVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                let fn_name = func.sig.ident.to_string();
                let arg_count = func.sig.inputs.len() as u32;

                if arg_count > self.threshold {
                    let (line, col, context) =
                        (1, 1, format!("fn {} ({} args)", fn_name, arg_count));
                    self.matches.push((line, col, fn_name, arg_count, context));
                }

                syn::visit::visit_item_fn(self, func);
            }
        }

        let mut visitor = ManyArgsVisitor { threshold, matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find blocking calls in async functions
    fn find_blocking_in_async(&self, syntax_tree: &syn::File) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct BlockingInAsyncVisitor {
            in_async_fn: bool,
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for BlockingInAsyncVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                let was_async = self.in_async_fn;
                self.in_async_fn = func.sig.asyncness.is_some();

                syn::visit::visit_item_fn(self, func);
                self.in_async_fn = was_async;
            }

            fn visit_expr_method_call(&mut self, method_call: &syn::ExprMethodCall) {
                if self.in_async_fn {
                    let method_name = method_call.method.to_string();

                    // Common blocking operations
                    if [
                        "read_to_string",
                        "write_all",
                        "flush",
                        "recv",
                        "send",
                        "lock",
                        "read",
                        "write",
                    ]
                    .contains(&method_name.as_str())
                    {
                        // Check if it's not awaited
                        let (line, col, context) = (1, 1, format!(".{}()", method_name));
                        self.matches.push((line, col, method_name, context));
                    }
                }

                syn::visit::visit_expr_method_call(self, method_call);
            }

            fn visit_expr_call(&mut self, call: &syn::ExprCall) {
                if self.in_async_fn {
                    if let syn::Expr::Path(path) = &*call.func {
                        if let Some(segment) = path.path.segments.last() {
                            let fn_name = segment.ident.to_string();

                            // Common blocking functions
                            if ["thread::sleep", "std::thread::sleep", "sleep"]
                                .contains(&fn_name.as_str())
                            {
                                let (line, col, context) = (1, 1, format!("{}()", fn_name));
                                self.matches.push((line, col, fn_name, context));
                            }
                        }
                    }
                }

                syn::visit::visit_expr_call(self, call);
            }
        }

        let mut visitor = BlockingInAsyncVisitor { in_async_fn: false, matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find futures that are not awaited
    fn find_futures_not_awaited(&self, syntax_tree: &syn::File) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct FutureNotAwaitedVisitor {
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for FutureNotAwaitedVisitor {
            fn visit_expr_call(&mut self, call: &syn::ExprCall) {
                // Look for function calls that return futures but aren't awaited
                if let syn::Expr::Path(path) = &*call.func {
                    if let Some(segment) = path.path.segments.last() {
                        let fn_name = segment.ident.to_string();

                        // Common async functions that return futures
                        if fn_name.ends_with("_async")
                            || ["spawn", "spawn_blocking", "timeout", "sleep"]
                                .contains(&fn_name.as_str())
                        {
                            let (line, col, context) = (1, 1, format!("{}() not awaited", fn_name));
                            self.matches.push((line, col, format!("{}()", fn_name), context));
                        }
                    }
                }

                syn::visit::visit_expr_call(self, call);
            }
        }

        let mut visitor = FutureNotAwaitedVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find tokio::select! without biased
    fn find_select_without_biased(&self, syntax_tree: &syn::File) -> Vec<(u32, u32, String)> {
        use syn::visit::Visit;

        struct SelectVisitor {
            matches: Vec<(u32, u32, String)>,
        }

        impl Visit<'_> for SelectVisitor {
            fn visit_macro(&mut self, mac: &syn::Macro) {
                if let Some(ident) = mac.path.get_ident() {
                    if ident == "select" {
                        // Check if it's tokio::select!
                        let macro_str = format!("{}", quote::quote!(#mac));
                        if macro_str.contains("select!") && !macro_str.contains("biased") {
                            let (line, col, context) =
                                (1, 1, "tokio::select! without biased".to_string());
                            self.matches.push((line, col, context));
                        }
                    }
                }
                syn::visit::visit_macro(self, mac);
            }
        }

        let mut visitor = SelectVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find generics without trait bounds
    fn find_generics_without_bounds(
        &self,
        syntax_tree: &syn::File,
    ) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct GenericBoundsVisitor {
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for GenericBoundsVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                for param in &func.sig.generics.params {
                    if let syn::GenericParam::Type(type_param) = param {
                        if type_param.bounds.is_empty() {
                            let generic_name = type_param.ident.to_string();
                            let (line, col, context) = (1, 1, format!("<{}>", generic_name));
                            self.matches.push((line, col, generic_name, context));
                        }
                    }
                }

                syn::visit::visit_item_fn(self, func);
            }

            fn visit_item_struct(&mut self, item_struct: &syn::ItemStruct) {
                for param in &item_struct.generics.params {
                    if let syn::GenericParam::Type(type_param) = param {
                        if type_param.bounds.is_empty() {
                            let generic_name = type_param.ident.to_string();
                            let (line, col, context) =
                                (1, 1, format!("struct {}<{}>", item_struct.ident, generic_name));
                            self.matches.push((line, col, generic_name, context));
                        }
                    }
                }

                syn::visit::visit_item_struct(self, item_struct);
            }
        }

        let mut visitor = GenericBoundsVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find test functions without assertions
    fn find_test_functions_without_assertions(
        &self,
        syntax_tree: &syn::File,
    ) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct TestAssertionVisitor {
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for TestAssertionVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                // Check if function has #[test] attribute
                let is_test = func.attrs.iter().any(|attr| attr.path().is_ident("test"));

                if is_test {
                    let fn_name = func.sig.ident.to_string();

                    // Check if function body contains assertions
                    if !self.has_assertions(&func.block) {
                        let (line, col, context) = (1, 1, format!("#[test] fn {}", fn_name));
                        self.matches.push((line, col, fn_name, context));
                    }
                }

                syn::visit::visit_item_fn(self, func);
            }
        }

        impl TestAssertionVisitor {
            fn has_assertions(&self, block: &syn::Block) -> bool {
                use syn::visit::Visit;

                struct AssertionFinder {
                    found: bool,
                }

                impl Visit<'_> for AssertionFinder {
                    fn visit_expr_macro(&mut self, expr_macro: &syn::ExprMacro) {
                        if let Some(ident) = expr_macro.mac.path.get_ident() {
                            let macro_name = ident.to_string();
                            if macro_name.starts_with("assert") {
                                self.found = true;
                            }
                        }
                        syn::visit::visit_expr_macro(self, expr_macro);
                    }

                    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
                        if let syn::Expr::Path(path) = &*call.func {
                            if let Some(segment) = path.path.segments.last() {
                                let fn_name = segment.ident.to_string();
                                if fn_name.starts_with("assert") || fn_name == "panic" {
                                    self.found = true;
                                }
                            }
                        }
                        syn::visit::visit_expr_call(self, call);
                    }
                }

                let mut finder = AssertionFinder { found: false };
                finder.visit_block(block);
                finder.found
            }
        }

        let mut visitor = TestAssertionVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find impl blocks without traits
    fn find_impl_without_trait(&self, syntax_tree: &syn::File) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct ImplTraitVisitor {
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for ImplTraitVisitor {
            fn visit_item_impl(&mut self, impl_item: &syn::ItemImpl) {
                // Check if this is an inherent impl (no trait)
                if impl_item.trait_.is_none() {
                    let type_name = match &*impl_item.self_ty {
                        syn::Type::Path(type_path) => type_path
                            .path
                            .segments
                            .last()
                            .map(|s| s.ident.to_string())
                            .unwrap_or_else(|| "Unknown".to_string()),
                        _ => "Unknown".to_string(),
                    };

                    let (line, col, context) = (1, 1, format!("impl {}", type_name));
                    self.matches.push((line, col, type_name, context));
                }

                syn::visit::visit_item_impl(self, impl_item);
            }
        }

        let mut visitor = ImplTraitVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find unsafe blocks
    fn find_unsafe_blocks(&self, syntax_tree: &syn::File) -> Vec<(u32, u32, String)> {
        use syn::visit::Visit;

        struct UnsafeVisitor {
            matches: Vec<(u32, u32, String)>,
        }

        impl Visit<'_> for UnsafeVisitor {
            fn visit_expr_unsafe(&mut self, expr: &syn::ExprUnsafe) {
                let (line, col, context) = (1, 1, "unsafe block".to_string());
                self.matches.push((line, col, context));

                syn::visit::visit_expr_unsafe(self, expr);
            }

            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                if func.sig.unsafety.is_some() {
                    let fn_name = func.sig.ident.to_string();
                    let (line, col, context) = (1, 1, format!("unsafe fn {}", fn_name));
                    self.matches.push((line, col, context));
                }

                syn::visit::visit_item_fn(self, func);
            }
        }

        let mut visitor = UnsafeVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Find ignored test functions
    fn find_ignored_tests(&self, syntax_tree: &syn::File) -> Vec<(u32, u32, String, String)> {
        use syn::visit::Visit;

        struct IgnoredTestVisitor {
            matches: Vec<(u32, u32, String, String)>,
        }

        impl Visit<'_> for IgnoredTestVisitor {
            fn visit_item_fn(&mut self, func: &syn::ItemFn) {
                // Check if function has both #[test] and #[ignore] attributes
                let is_test = func.attrs.iter().any(|attr| attr.path().is_ident("test"));
                let is_ignored = func.attrs.iter().any(|attr| attr.path().is_ident("ignore"));

                if is_test && is_ignored {
                    let fn_name = func.sig.ident.to_string();
                    let (line, col, context) = (1, 1, format!("#[ignore] #[test] fn {}", fn_name));
                    self.matches.push((line, col, fn_name, context));
                }

                syn::visit::visit_item_fn(self, func);
            }
        }

        let mut visitor = IgnoredTestVisitor { matches: Vec::new() };

        visitor.visit_file(syntax_tree);
        visitor.matches
    }

    /// Convert pattern matches to violations
    pub fn matches_to_violations(&self, matches: Vec<PatternMatch>) -> Vec<Violation> {
        matches
            .into_iter()
            .map(|m| {
                let mut violation = Violation::new(m.rule_id, m.severity, m.file_path, m.message);

                if let Some(line) = m.line_number {
                    if let Some(col) = m.column_number {
                        violation = violation.with_position(line, col);
                    }
                }

                if let Some(context) = m.context {
                    violation = violation.with_context(context);
                }

                violation
            })
            .collect()
    }
}

impl Default for PatternEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Architecture-compliant validation functions for integration testing
#[allow(dead_code)]
pub mod validation {
    use super::*;
    use crate::config::PatternRule;

    /// Validate regex pattern functionality - designed for integration testing
    pub fn validate_regex_pattern_functionality() -> crate::domain::violations::GuardianResult<()> {
        let mut engine = PatternEngine::new();

        let rule = PatternRule {
            id: "todo_validation".to_string(),
            rule_type: RuleType::Regex,
            pattern: r"\bTODO\b".to_string(),
            message: "TODO found: {match}".to_string(),
            severity: None,
            enabled: true,
            case_sensitive: true,
            exclude_if: None,
        };

        engine.add_rule(&rule, Severity::Warning)?;

        let content = "// TODO: implement this\nlet x = 5;";
        let matches = engine.analyze_file(Path::new("validation.rs"), content)?;

        if matches.len() != 1
            || matches[0].rule_id != "todo_validation"
            || matches[0].matched_text != "TODO"
        {
            return Err(crate::domain::violations::GuardianError::pattern(
                "Regex pattern validation failed - incorrect match results",
            ));
        }

        Ok(())
    }

    /// Validate AST pattern functionality - designed for integration testing
    pub fn validate_ast_pattern_functionality() -> crate::domain::violations::GuardianResult<()> {
        let mut engine = PatternEngine::new();

        let rule = PatternRule {
            id: "unimplemented_validation".to_string(),
            rule_type: RuleType::Ast,
            pattern: "macro_call:unimplemented|todo".to_string(),
            message: "Unfinished macro: {macro_name}".to_string(),
            severity: None,
            enabled: true,
            case_sensitive: true,
            exclude_if: None,
        };

        engine.add_rule(&rule, Severity::Error)?;

        let content = "fn validation() {\n    unimplemented!()\n}";
        let matches = engine.analyze_file(Path::new("validation.rs"), content)?;

        if matches.len() != 1
            || matches[0].rule_id != "unimplemented_validation"
            || !matches[0].message.contains("unimplemented")
        {
            return Err(crate::domain::violations::GuardianError::pattern(
                "AST pattern validation failed - incorrect match results",
            ));
        }

        Ok(())
    }

    /// Validate exclude conditions functionality - designed for integration testing
    pub fn validate_exclude_conditions_functionality()
    -> crate::domain::violations::GuardianResult<()> {
        let mut engine = PatternEngine::new();

        let rule = PatternRule {
            id: "todo_exclusion_validation".to_string(),
            rule_type: RuleType::Regex,
            pattern: r"\bTODO\b".to_string(),
            message: "TODO found: {match}".to_string(),
            severity: None,
            enabled: true,
            case_sensitive: true,
            exclude_if: Some(ExcludeConditions {
                attribute: None,
                in_tests: true,
                file_patterns: None,
            }),
        };

        engine.add_rule(&rule, Severity::Warning)?;

        let content = "// TODO: implement this";

        // Should match in regular file
        let matches = engine.analyze_file(Path::new("src/lib.rs"), content)?;
        if matches.len() != 1 {
            return Err(crate::domain::violations::GuardianError::pattern(
                "Exclude conditions validation failed - should match in regular file",
            ));
        }

        // Should be excluded in test file
        let matches = engine.analyze_file(Path::new("tests/unit.rs"), content)?;
        if !matches.is_empty() {
            return Err(crate::domain::violations::GuardianError::pattern(
                "Exclude conditions validation failed - should be excluded in test file",
            ));
        }

        Ok(())
    }
}
