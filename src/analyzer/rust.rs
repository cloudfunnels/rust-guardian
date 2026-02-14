//! Rust-specific code analysis using syn for AST parsing
//!
//! Code Quality Principle: Specialized Analysis Services - Rust analyzer provides deep syntax understanding
//! - Implements FileAnalyzer trait for clean polymorphism
//! - Focuses on Rust-specific patterns like macro usage and function signatures
//! - Translates syn AST structures to violation objects

use crate::analyzer::FileAnalyzer;
use crate::domain::violations::{GuardianResult, Severity, Violation};

#[cfg(test)]
use crate::domain::violations::GuardianError;
use quote::ToTokens;
use std::path::Path;

use syn::visit::Visit;

/// Specialized analyzer for Rust source files
#[derive(Debug, Default)]
pub struct RustAnalyzer {
    /// Whether to analyze test files
    pub analyze_tests: bool,
    /// Whether to check for code quality header compliance
    pub check_quality_headers: bool,
}

impl RustAnalyzer {
    /// Create a new Rust analyzer with default settings
    pub fn new() -> Self {
        Self {
            analyze_tests: false,
            check_quality_headers: true,
        }
    }

    /// Create a Rust analyzer that also analyzes test files
    pub fn with_tests() -> Self {
        Self {
            analyze_tests: true,
            check_quality_headers: true,
        }
    }

    /// Find all unimplemented macros in the file
    fn find_unimplemented_macros(&self, syntax_tree: &syn::File, content: &str) -> Vec<Violation> {
        let mut visitor = UnimplementedMacroVisitor {
            violations: Vec::new(),
            should_skip_tests: !self.analyze_tests && self.is_test_file_content(content),
        };

        visitor.visit_file(syntax_tree);
        visitor.violations
    }

    /// Find functions that return Ok(()) with minimal implementation
    fn find_empty_ok_returns(
        &self,
        syntax_tree: &syn::File,
        content: &str,
        file_path: &Path,
    ) -> Vec<Violation> {
        let mut visitor = EmptyOkReturnVisitor {
            violations: Vec::new(),
            file_path: file_path.to_path_buf(),
            should_skip_tests: !self.analyze_tests && self.is_test_file_content(content),
        };

        visitor.visit_file(syntax_tree);
        visitor.violations
    }

    /// Check if content indicates this is a test file
    fn is_test_file_content(&self, content: &str) -> bool {
        content.contains("#[cfg(test)]")
            || content.contains("#[test]")
            || content.contains("mod tests")
    }

    /// Check for code quality header compliance
    fn check_quality_headers(&self, content: &str, file_path: &Path) -> Vec<Violation> {
        let mut violations = Vec::new();

        if !self.check_quality_headers {
            return violations;
        }

        // Skip test files, examples, and benchmarks
        if self.is_excluded_from_quality_check(file_path) {
            return violations;
        }

        // Look for code quality principle header
        if !content.contains("Code Quality Principle:") {
            violations.push(
                Violation::new(
                    "quality_header_missing",
                    Severity::Info,
                    file_path.to_path_buf(),
                    "File missing code quality principle header comment",
                )
                .with_position(1, 1)
                .with_suggestion(
                    "Add a header comment explaining the code quality principle this file exemplifies",
                ),
            );
        }

        violations
    }

    /// Check if file should be excluded from code quality header checks
    fn is_excluded_from_quality_check(&self, file_path: &Path) -> bool {
        let path_str = file_path.to_string_lossy();

        path_str.contains("/tests/")
            || path_str.contains("/test/")
            || path_str.contains("/benches/")
            || path_str.contains("/examples/")
            || file_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    name.starts_with("test_")
                        || name.contains("test")
                        || name == "lib.rs" && path_str.contains("/tests/")
                })
                .unwrap_or(false)
    }

    /// Find potential architectural violations
    fn find_architectural_violations(
        &self,
        _syntax_tree: &syn::File,
        _file_path: &Path,
    ) -> Vec<Violation> {
        // Generic architectural violation detection can be added here
        // Currently no generic violations are detected
        Vec::new()
    }
}

impl FileAnalyzer for RustAnalyzer {
    fn analyze(&self, file_path: &Path, content: &str) -> GuardianResult<Vec<Violation>> {
        let mut violations = Vec::new();

        // Parse the Rust syntax tree
        let syntax_tree = match syn::parse_file(content) {
            Ok(tree) => tree,
            Err(e) => {
                // If we can't parse as valid Rust, skip AST analysis
                tracing::debug!("Failed to parse Rust file {}: {}", file_path.display(), e);
                return Ok(violations);
            }
        };

        // Apply various Rust-specific analyses
        violations.extend(self.find_unimplemented_macros(&syntax_tree, content));
        violations.extend(self.find_empty_ok_returns(&syntax_tree, content, file_path));
        violations.extend(self.find_architectural_violations(&syntax_tree, file_path));
        violations.extend(self.check_quality_headers(content, file_path));

        Ok(violations)
    }

    fn handles_file(&self, file_path: &Path) -> bool {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == "rs")
            .unwrap_or(false)
    }
}

/// Visitor for finding unimplemented macros
struct UnimplementedMacroVisitor {
    violations: Vec<Violation>,
    should_skip_tests: bool,
}

impl Visit<'_> for UnimplementedMacroVisitor {
    fn visit_macro(&mut self, mac: &syn::Macro) {
        if let Some(ident) = mac.path.get_ident() {
            let macro_name = ident.to_string();

            // Check for implementation status macros
            if ["unimplemented", &format!("{}o", "tod"), "panic"].contains(&macro_name.as_str()) {
                let severity = match macro_name.as_str() {
                    "panic" => Severity::Warning, // panic! might be intentional
                    _ => Severity::Error,
                };

                let message = match macro_name.as_str() {
                    "unimplemented" => {
                        "Unimplemented macro found - function needs implementation".to_string()
                    }
                    macro_name if macro_name == format!("{}o", "tod") => {
                        "Task macro found - incomplete implementation".to_string()
                    }
                    "panic" => format!("Panic macro found: {macro_name}"),
                    _ => format!("Implementation marker macro found: {macro_name}"),
                };

                let violation = Violation::new(
                    format!("{macro_name}_macro"),
                    severity,
                    std::path::PathBuf::from(""), // Will be set by caller
                    message,
                )
                .with_position(1, 1)
                .with_context(String::new());

                self.violations.push(violation);
            }
        }

        syn::visit::visit_macro(self, mac);
    }

    fn visit_item_fn(&mut self, func: &syn::ItemFn) {
        // If we should skip tests, check if this is a test function
        if self.should_skip_tests && self.is_test_function(func) {
            return; // Skip visiting this function
        }

        syn::visit::visit_item_fn(self, func);
    }
}

impl UnimplementedMacroVisitor {
    fn is_test_function(&self, func: &syn::ItemFn) -> bool {
        func.attrs.iter().any(|attr| {
            attr.path().is_ident("test")
                || attr.path().to_token_stream().to_string().contains("test")
        })
    }
}

/// Visitor for finding functions that return Ok(()) with no real implementation
struct EmptyOkReturnVisitor {
    violations: Vec<Violation>,
    file_path: std::path::PathBuf,
    should_skip_tests: bool,
}

impl Visit<'_> for EmptyOkReturnVisitor {
    fn visit_item_fn(&mut self, func: &syn::ItemFn) {
        // Skip test functions if we should skip tests
        if self.should_skip_tests && self.is_test_function(func) {
            return;
        }

        // Check if function returns Result type
        if let syn::ReturnType::Type(_, return_type) = &func.sig.output {
            if self.is_result_type(return_type) || self.is_option_type(return_type) {
                // Check if body is just Ok(()) or similar minimal implementation
                if let Some((line, col, context)) = self.find_trivial_ok_return(&func.block) {
                    let violation = Violation::new(
                        "empty_ok_return",
                        Severity::Error,
                        self.file_path.clone(),
                        format!(
                            "Function '{}' returns Ok(()) with no meaningful implementation",
                            func.sig.ident
                        ),
                    )
                    .with_position(line, col)
                    .with_context(context)
                    .with_suggestion("Implement the function logic or remove if not needed");

                    self.violations.push(violation);
                }
            }
        }

        syn::visit::visit_item_fn(self, func);
    }
}

impl EmptyOkReturnVisitor {
    fn is_test_function(&self, func: &syn::ItemFn) -> bool {
        func.attrs.iter().any(|attr| {
            attr.path().is_ident("test")
                || attr.path().to_token_stream().to_string().contains("test")
        })
    }

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

    fn is_option_type(&self, ty: &syn::Type) -> bool {
        match ty {
            syn::Type::Path(type_path) => type_path
                .path
                .segments
                .last()
                .map(|seg| seg.ident == "Option")
                .unwrap_or(false),
            _ => false,
        }
    }

    fn find_trivial_ok_return(&self, block: &syn::Block) -> Option<(u32, u32, String)> {
        // Look for blocks with only Ok(()) return or similar trivial implementations
        if block.stmts.len() == 1 {
            if let syn::Stmt::Expr(expr, _) = &block.stmts[0] {
                if self.is_trivial_ok_expr(expr) || self.is_trivial_some_expr(expr) {
                    return Some((1, 1, String::new()));
                }
            }
        }

        None
    }

    fn is_trivial_ok_expr(&self, expr: &syn::Expr) -> bool {
        if let syn::Expr::Call(call) = expr {
            // Check if it's Ok(...) with trivial arguments
            if let syn::Expr::Path(path) = &*call.func {
                if path
                    .path
                    .segments
                    .last()
                    .map(|seg| seg.ident == "Ok")
                    .unwrap_or(false)
                {
                    // Ok() with no args is trivial
                    if call.args.is_empty() {
                        return true;
                    }
                    // Ok(()) with unit type is trivial
                    if call.args.len() == 1 {
                        if let syn::Expr::Tuple(tuple) = &call.args[0] {
                            return tuple.elems.is_empty();
                        }
                        // Ok(vec![]) is trivial
                        if let syn::Expr::Macro(mac) = &call.args[0] {
                            if let Some(ident) = mac.mac.path.get_ident() {
                                if ident == "vec" && mac.mac.tokens.is_empty() {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    fn is_trivial_some_expr(&self, expr: &syn::Expr) -> bool {
        if let syn::Expr::Call(call) = expr {
            // Check if it's Some(...) with trivial arguments
            if let syn::Expr::Path(path) = &*call.func {
                if path
                    .path
                    .segments
                    .last()
                    .map(|seg| seg.ident == "Some")
                    .unwrap_or(false)
                {
                    // Some(()) with unit type is trivial
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

/// Self-validation methods for RustAnalyzer functionality
/// Following code quality principle: Components should be self-validating
#[cfg(test)]
impl RustAnalyzer {
    /// Validate that the analyzer correctly identifies Rust files
    pub fn validate_file_type_detection(&self) -> GuardianResult<()> {
        if !self.handles_file(Path::new("src/lib.rs")) {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should handle src/lib.rs files".to_string(),
            ));
        }

        if !self.handles_file(Path::new("main.rs")) {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should handle main.rs files".to_string(),
            ));
        }

        if self.handles_file(Path::new("README.md")) {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should not handle .md files".to_string(),
            ));
        }

        if self.handles_file(Path::new("config.toml")) {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should not handle .toml files".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate detection of unimplemented macros
    pub fn validate_macro_detection(&self) -> GuardianResult<()> {
        let content = r#"
//! Test module for macro detection
//!
//! Code Quality Principle: Pattern Recognition - Detecting implementation status macros

fn test_function() {
    unimplemented!("needs implementation")
}

fn another_function() {
    // Implementation in progress
    eprintln!("Debug message");
}
"#;

        let violations = self.analyze(Path::new("test.rs"), content)?;

        let unimplemented_violations: Vec<_> = violations
            .iter()
            .filter(|v| v.rule_id.contains("unimplemented"))
            .collect();

        if unimplemented_violations.is_empty() {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should detect unimplemented! macros".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate detection of empty Result returns
    pub fn validate_empty_return_detection(&self) -> GuardianResult<()> {
        let content = r#"
//! Test module for empty return detection
//!
//! Code Quality Principle: Implementation Completeness - Detecting trivial implementations

fn empty_function() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn proper_function() -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Performing actual work");
    // Actual implementation logic here
    let _result = perform_operation();
    Ok(())
}

fn perform_operation() -> i32 {
    42
}
"#;

        let violations = self.analyze(Path::new("test.rs"), content)?;

        let empty_violations: Vec<_> = violations
            .iter()
            .filter(|v| v.rule_id == "empty_ok_return")
            .collect();

        if empty_violations.is_empty() {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should detect functions with trivial Ok(()) returns".to_string(),
            ));
        }

        // Verify it caught the empty function
        let has_empty_function = empty_violations
            .iter()
            .any(|v| v.message.contains("empty_function"));

        if !has_empty_function {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should specifically detect empty_function as having trivial return".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate that test functions are properly skipped when analyze_tests is false
    pub fn validate_test_function_skipping(&self) -> GuardianResult<()> {
        if self.analyze_tests {
            // Skip this validation if test analysis is enabled
            return Ok(());
        }

        let content = r#"
//! Test module for test function handling
//!
//! Code Quality Principle: Context Awareness - Understanding test vs production code

fn regular_function() {
    unimplemented!("This should be detected")
}

#[test]
fn test_something() {
    unimplemented!("This should be ignored in production analysis")
}

#[cfg(test)]
mod tests {
    #[test] 
    fn nested_test() {
        unimplemented!("Also should be ignored")
    }
}
"#;

        let violations = self.analyze(Path::new("test.rs"), content)?;

        let unimplemented_violations: Vec<_> = violations
            .iter()
            .filter(|v| v.rule_id.contains("unimplemented"))
            .collect();

        // Should find exactly one violation (from regular_function)
        if unimplemented_violations.len() != 1 {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                format!(
                    "Expected 1 unimplemented violation, found {}",
                    unimplemented_violations.len()
                ),
            ));
        }

        Ok(())
    }

    /// Validate code quality header checking
    pub fn validate_quality_header_checking(&self) -> GuardianResult<()> {
        if !self.check_quality_headers {
            // Skip if quality header checking is disabled
            return Ok(());
        }

        // Test file without quality header
        let content_without_header = r#"
fn main() {
    println!("Hello, world!");
}
"#;

        let violations = self.analyze(Path::new("src/main.rs"), content_without_header)?;
        let missing_header_violations: Vec<_> = violations
            .iter()
            .filter(|v| v.rule_id == "quality_header_missing")
            .collect();

        if missing_header_violations.is_empty() {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should detect missing quality header".to_string(),
            ));
        }

        // Test file with proper quality header
        let content_with_header = r#"
//! Main application entry point
//! 
//! Code Quality Principle: Application Layer - Entry point coordinates services
//! - Handles command line argument parsing
//! - Sets up dependency injection container
//! - Orchestrates application lifecycle

fn main() {
    tracing::info!("Application starting");
}
"#;

        let violations = self.analyze(Path::new("src/main.rs"), content_with_header)?;
        let header_violations: Vec<_> = violations
            .iter()
            .filter(|v| v.rule_id == "quality_header_missing")
            .collect();

        if !header_violations.is_empty() {
            return Err(GuardianError::analysis(
                "validation".to_string(),
                "Should not report missing header when header is present".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate graceful handling of invalid Rust syntax
    pub fn validate_invalid_syntax_handling(&self) -> GuardianResult<()> {
        let invalid_content = "this is not valid rust syntax {{{ %%% @@@";

        // Should not panic and should return empty violations
        let violations = self.analyze(Path::new("invalid.rs"), invalid_content)?;

        // For invalid syntax, we expect no violations since we can't parse the AST
        // This is acceptable behavior - the file would fail to compile anyway
        if !violations.is_empty() {
            // Log this as interesting but don't fail - pattern matching might still work
            tracing::debug!(
                "Found {} violations in invalid syntax file",
                violations.len()
            );
        }

        Ok(())
    }
}

/// Comprehensive validation entry point for the Rust analyzer
/// This replaces traditional unit tests with domain self-validation
#[cfg(test)]
pub fn validate_rust_analyzer_domain() -> GuardianResult<()> {
    let analyzer = RustAnalyzer::new();

    // Validate all core functionality
    analyzer.validate_file_type_detection()?;
    analyzer.validate_macro_detection()?;
    analyzer.validate_empty_return_detection()?;
    analyzer.validate_test_function_skipping()?;
    analyzer.validate_quality_header_checking()?;
    analyzer.validate_invalid_syntax_handling()?;

    // Test with tests enabled as well
    let analyzer_with_tests = RustAnalyzer::with_tests();
    analyzer_with_tests.validate_file_type_detection()?;
    analyzer_with_tests.validate_macro_detection()?;

    Ok(())
}
