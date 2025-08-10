//! Pattern engine for detecting code quality violations
//!
//! CDD Principle: Domain Services - Pattern matching orchestrates complex analysis operations
//! - PatternEngine coordinates different types of pattern matching (regex, AST, semantic)
//! - Each pattern type implements the PatternMatcher trait for clean polymorphism
//! - Pattern results are translated to domain violations at the boundary

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
    /// Look for missing CDD headers in files
    MissingCddHeader,
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
        Self {
            regex_patterns: HashMap::new(),
            ast_patterns: HashMap::new(),
        }
    }

    /// Add a pattern rule to the engine
    pub fn add_rule(
        &mut self,
        rule: &PatternRule,
        effective_severity: Severity,
    ) -> GuardianResult<()> {
        match rule.rule_type {
            RuleType::Regex => {
                let regex = if rule.case_sensitive {
                    Regex::new(&rule.pattern)
                } else {
                    RegexBuilder::new(&rule.pattern)
                        .case_insensitive(true)
                        .build()
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
                // TODO: Implement semantic and import analysis patterns
                tracing::warn!(
                    "Semantic and import analysis patterns not yet implemented: {}",
                    rule.id
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
                .unwrap()
                .split('|')
                .map(|s| s.trim().to_string())
                .collect();
            Ok(AstPatternType::MacroCall(macros))
        } else if pattern == "return_ok_unit_with_no_logic" {
            Ok(AstPatternType::EmptyOkReturn)
        } else if pattern.contains("CDD Principle:") {
            Ok(AstPatternType::MissingCddHeader)
        } else {
            Err(GuardianError::pattern(format!(
                "Unknown AST pattern type in rule '{}': {}",
                rule_id, pattern
            )))
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

        // Apply regex patterns
        for pattern in self.regex_patterns.values() {
            let pattern_matches = self.apply_regex_pattern(pattern, file_path, content)?;
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
        let mut matches = Vec::new();

        // Find all matches in the content
        for regex_match in pattern.regex.find_iter(content) {
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

                    let message = pattern
                        .message_template
                        .replace("{macro_name}", &macro_name);

                    matches.push(PatternMatch {
                        rule_id: pattern.rule_id.clone(),
                        file_path: file_path.to_path_buf(),
                        line_number: Some(line),
                        column_number: Some(col),
                        matched_text: format!("{}!()", macro_name),
                        message,
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
            AstPatternType::MissingCddHeader => {
                if !content.contains("CDD Principle:") {
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

        impl<'a> Visit<'_> for MacroVisitor<'a> {
            fn visit_macro(&mut self, mac: &syn::Macro) {
                if let Some(ident) = mac.path.get_ident() {
                    let macro_name = ident.to_string();
                    if self.target_macros.contains(&macro_name) {
                        let _span = mac.path.span();
                        // Use a simple line-based location since proc_macro2::Span doesn't have start() method
                        // Use a simple line-based location since proc_macro2::Span doesn't have start() method
                        let (line, col, context) = (1, 1, String::new());
                        self.matches.push((line, col, macro_name, context));
                    }
                }
                syn::visit::visit_macro(self, mac);
            }
        }

        let mut visitor = MacroVisitor {
            target_macros,
            matches: Vec::new(),
        };

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
                match expr {
                    syn::Expr::Call(call) => {
                        // Check if it's Ok(())
                        if let syn::Expr::Path(path) = &*call.func {
                            if path
                                .path
                                .segments
                                .last()
                                .map(|seg| seg.ident == "Ok")
                                .unwrap_or(false)
                            {
                                // Check if argument is unit type ()
                                if call.args.len() == 1 {
                                    if let syn::Expr::Tuple(tuple) = &call.args[0] {
                                        return tuple.elems.is_empty();
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                false
            }
        }

        let mut visitor = EmptyOkVisitor {
            matches: Vec::new(),
        };

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
        let line_end = content[line_start..]
            .find('\n')
            .map(|pos| line_start + pos)
            .unwrap_or(content.len());

        let context = content[line_start..line_end].trim().to_string();

        (line, col, context)
    }

    /// Check if a regex match should be excluded based on conditions
    fn should_exclude_match(
        &self,
        conditions: Option<&ExcludeConditions>,
        file_path: &Path,
        _matched_text: &str,
        _content: &str,
        _offset: usize,
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

            // TODO: Check attributes and other conditions when we have AST context
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

            // TODO: Check for specific attributes like #[test] on functions
        }

        false
    }

    /// Check if a file path indicates it's a test file
    fn is_test_file(&self, file_path: &Path) -> bool {
        file_path.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .map(|s| s == "tests" || s == "test")
                .unwrap_or(false)
        }) || file_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.contains("test") || name.starts_with("test_"))
            .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PatternRule;

    #[test]
    fn test_regex_pattern() {
        let mut engine = PatternEngine::new();

        let rule = PatternRule {
            id: "todo_test".to_string(),
            rule_type: RuleType::Regex,
            pattern: r"\bTODO\b".to_string(),
            message: "TODO found: {match}".to_string(),
            severity: None,
            enabled: true,
            case_sensitive: true,
            exclude_if: None,
        };

        engine.add_rule(&rule, Severity::Warning).unwrap();

        let content = "// TODO: implement this\nlet x = 5;";
        let matches = engine.analyze_file(Path::new("test.rs"), content).unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].rule_id, "todo_test");
        assert_eq!(matches[0].matched_text, "TODO");
        assert_eq!(matches[0].line_number, Some(1));
    }

    #[test]
    fn test_macro_ast_pattern() {
        let mut engine = PatternEngine::new();

        let rule = PatternRule {
            id: "unimplemented_test".to_string(),
            rule_type: RuleType::Ast,
            pattern: "macro_call:unimplemented|todo".to_string(),
            message: "Unfinished macro: {macro_name}".to_string(),
            severity: None,
            enabled: true,
            case_sensitive: true,
            exclude_if: None,
        };

        engine.add_rule(&rule, Severity::Error).unwrap();

        let content = "fn test() {\n    unimplemented!()\n}";
        let matches = engine.analyze_file(Path::new("test.rs"), content).unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].rule_id, "unimplemented_test");
        assert!(matches[0].message.contains("unimplemented"));
    }

    #[test]
    fn test_exclude_conditions() {
        let mut engine = PatternEngine::new();

        let rule = PatternRule {
            id: "todo_test".to_string(),
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

        engine.add_rule(&rule, Severity::Warning).unwrap();

        let content = "// TODO: implement this";

        // Should match in regular file
        let matches = engine
            .analyze_file(Path::new("src/lib.rs"), content)
            .unwrap();
        assert_eq!(matches.len(), 1);

        // Should be excluded in test file
        let matches = engine
            .analyze_file(Path::new("tests/unit.rs"), content)
            .unwrap();
        assert_eq!(matches.len(), 0);
    }
}
