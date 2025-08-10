//! Rust-specific code analysis using syn for AST parsing
//! 
//! CDD Principle: Specialized Domain Services - Rust analyzer provides deep syntax understanding
//! - Implements FileAnalyzer trait for clean polymorphism
//! - Focuses on Rust-specific patterns like macro usage and function signatures
//! - Translates syn AST structures to domain violation objects

use crate::analyzer::FileAnalyzer;
use crate::domain::violations::{Violation, Severity, GuardianResult};
use std::path::Path;
use syn::visit::Visit;
use syn::spanned::Spanned;
use quote::ToTokens;

/// Specialized analyzer for Rust source files
#[derive(Debug, Default)]
pub struct RustAnalyzer {
    /// Whether to analyze test files
    pub analyze_tests: bool,
    /// Whether to check for CDD compliance
    pub check_cdd_compliance: bool,
}

impl RustAnalyzer {
    /// Create a new Rust analyzer with default settings
    pub fn new() -> Self {
        Self {
            analyze_tests: false,
            check_cdd_compliance: true,
        }
    }
    
    /// Create a Rust analyzer that also analyzes test files
    pub fn with_tests() -> Self {
        Self {
            analyze_tests: true,
            check_cdd_compliance: true,
        }
    }
    
    /// Find all unimplemented macros in the file
    fn find_unimplemented_macros(&self, syntax_tree: &syn::File, content: &str) -> Vec<Violation> {
        let mut visitor = UnimplementedMacroVisitor {
            violations: Vec::new(),
            content,
            should_skip_tests: !self.analyze_tests && self.is_test_file_content(content),
        };
        
        visitor.visit_file(syntax_tree);
        visitor.violations
    }
    
    /// Find functions that return Ok(()) with minimal implementation
    fn find_empty_ok_returns(&self, syntax_tree: &syn::File, content: &str, file_path: &Path) -> Vec<Violation> {
        let mut visitor = EmptyOkReturnVisitor {
            violations: Vec::new(),
            content,
            file_path: file_path.to_path_buf(),
            should_skip_tests: !self.analyze_tests && self.is_test_file_content(content),
        };
        
        visitor.visit_file(syntax_tree);
        visitor.violations
    }
    
    /// Check if content indicates this is a test file
    fn is_test_file_content(&self, content: &str) -> bool {
        content.contains("#[cfg(test)]") || 
        content.contains("#[test]") ||
        content.contains("mod tests")
    }
    
    /// Check for CDD compliance (header comments)
    fn check_cdd_compliance(&self, content: &str, file_path: &Path) -> Vec<Violation> {
        let mut violations = Vec::new();
        
        if !self.check_cdd_compliance {
            return violations;
        }
        
        // Skip test files, examples, and benchmarks
        if self.is_excluded_from_cdd_check(file_path) {
            return violations;
        }
        
        // Look for CDD principle header
        if !content.contains("CDD Principle:") {
            violations.push(
                Violation::new(
                    "cdd_header_missing",
                    Severity::Info,
                    file_path.to_path_buf(),
                    "File missing CDD principle header comment",
                )
                .with_position(1, 1)
                .with_suggestion("Add a header comment explaining which CDD principle this file exemplifies")
            );
        }
        
        violations
    }
    
    /// Check if file should be excluded from CDD compliance checks
    fn is_excluded_from_cdd_check(&self, file_path: &Path) -> bool {
        let path_str = file_path.to_string_lossy();
        
        path_str.contains("/tests/") ||
        path_str.contains("/test/") ||
        path_str.contains("/benches/") ||
        path_str.contains("/examples/") ||
        file_path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("test_") || name.contains("test") || name == "lib.rs" && path_str.contains("/tests/"))
            .unwrap_or(false)
    }
    
    /// Find potential architectural violations
    fn find_architectural_violations(&self, syntax_tree: &syn::File, file_path: &Path) -> Vec<Violation> {
        let mut visitor = ArchitecturalViolationVisitor {
            violations: Vec::new(),
            file_path: file_path.to_path_buf(),
        };
        
        visitor.visit_file(syntax_tree);
        visitor.violations
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
        violations.extend(self.check_cdd_compliance(content, file_path));
        
        Ok(violations)
    }
    
    fn handles_file(&self, file_path: &Path) -> bool {
        file_path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == "rs")
            .unwrap_or(false)
    }
}

/// Visitor for finding unimplemented macros
struct UnimplementedMacroVisitor<'a> {
    violations: Vec<Violation>,
    content: &'a str,
    should_skip_tests: bool,
}

impl<'a> Visit<'_> for UnimplementedMacroVisitor<'a> {
    fn visit_macro(&mut self, mac: &syn::Macro) {
        if let Some(ident) = mac.path.get_ident() {
            let macro_name = ident.to_string();
            
            // Check for placeholder macros
            if ["unimplemented", "todo", "panic"].contains(&macro_name.as_str()) {
                let severity = match macro_name.as_str() {
                    "panic" => Severity::Warning, // panic! might be intentional
                    _ => Severity::Error,
                };
                
                let message = match macro_name.as_str() {
                    "unimplemented" => "Unimplemented macro found - function needs implementation".to_string(),
                    "todo" => "TODO macro found - incomplete implementation".to_string(),
                    "panic" => format!("Panic macro found: {}", macro_name),
                    _ => format!("Placeholder macro found: {}", macro_name),
                };
                
                let violation = Violation::new(
                    format!("{}_macro", macro_name),
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

impl<'a> UnimplementedMacroVisitor<'a> {
    fn is_test_function(&self, func: &syn::ItemFn) -> bool {
        func.attrs.iter().any(|attr| {
            attr.path().is_ident("test") || 
            attr.path().to_token_stream().to_string().contains("test")
        })
    }
}

/// Visitor for finding functions that return Ok(()) with no real implementation
struct EmptyOkReturnVisitor<'a> {
    violations: Vec<Violation>,
    content: &'a str,
    file_path: std::path::PathBuf,
    should_skip_tests: bool,
}

impl<'a> Visit<'_> for EmptyOkReturnVisitor<'a> {
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
                        format!("Function '{}' returns Ok(()) with no meaningful implementation", func.sig.ident),
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

impl<'a> EmptyOkReturnVisitor<'a> {
    fn is_test_function(&self, func: &syn::ItemFn) -> bool {
        func.attrs.iter().any(|attr| {
            attr.path().is_ident("test") || 
            attr.path().to_token_stream().to_string().contains("test")
        })
    }
    
    fn is_result_type(&self, ty: &syn::Type) -> bool {
        match ty {
            syn::Type::Path(type_path) => {
                type_path.path.segments.last()
                    .map(|seg| seg.ident == "Result")
                    .unwrap_or(false)
            }
            _ => false
        }
    }
    
    fn is_option_type(&self, ty: &syn::Type) -> bool {
        match ty {
            syn::Type::Path(type_path) => {
                type_path.path.segments.last()
                    .map(|seg| seg.ident == "Option")
                    .unwrap_or(false)
            }
            _ => false
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
        match expr {
            syn::Expr::Call(call) => {
                // Check if it's Ok(...) with trivial arguments
                if let syn::Expr::Path(path) = &*call.func {
                    if path.path.segments.last().map(|seg| seg.ident == "Ok").unwrap_or(false) {
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
            _ => {}
        }
        false
    }
    
    fn is_trivial_some_expr(&self, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Call(call) => {
                // Check if it's Some(...) with trivial arguments
                if let syn::Expr::Path(path) = &*call.func {
                    if path.path.segments.last().map(|seg| seg.ident == "Some").unwrap_or(false) {
                        // Some(()) with unit type is trivial
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

/// Visitor for finding architectural violations
struct ArchitecturalViolationVisitor {
    violations: Vec<Violation>,
    file_path: std::path::PathBuf,
}

impl Visit<'_> for ArchitecturalViolationVisitor {
    fn visit_item_use(&mut self, use_item: &syn::ItemUse) {
        // Check for direct substrate access violations
        let use_tree_str = quote::quote!(#use_item).to_string();
        
        if use_tree_str.contains("substrate") && !self.is_allowed_substrate_access() {
            let _span = use_item.span();
            // Use a simple line-based location since proc_macro2::Span doesn't have start() method
            
            let violation = Violation::new(
                "direct_substrate_access",
                Severity::Warning,
                self.file_path.clone(),
                "Direct substrate access may violate architectural boundaries",
            )
            .with_position(1, 1)
            .with_suggestion("Consider accessing substrate through designated services or repositories");
            
            self.violations.push(violation);
        }
        
        syn::visit::visit_item_use(self, use_item);
    }
}

impl ArchitecturalViolationVisitor {
    fn is_allowed_substrate_access(&self) -> bool {
        // Allow substrate access in certain contexts
        let path_str = self.file_path.to_string_lossy();
        
        path_str.contains("tests/") ||
        path_str.contains("examples/")
    }
}

/// Helper function to extract location information from proc_macro2 span
fn get_location_from_span(content: &str, line: usize, column: usize) -> (u32, u32, String) {
    let lines: Vec<&str> = content.lines().collect();
    
    if line > 0 && line <= lines.len() {
        let context = lines[line - 1].trim().to_string();
        (line as u32, column as u32, context)
    } else {
        (line as u32, column as u32, "".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Test imports
    
    #[test]
    fn test_rust_analyzer_handles_rust_files() {
        let analyzer = RustAnalyzer::new();
        
        assert!(analyzer.handles_file(Path::new("src/lib.rs")));
        assert!(analyzer.handles_file(Path::new("main.rs")));
        assert!(!analyzer.handles_file(Path::new("README.md")));
        assert!(!analyzer.handles_file(Path::new("config.toml")));
    }
    
    #[test]
    fn test_find_unimplemented_macros() {
        let analyzer = RustAnalyzer::new();
        let content = r#"
fn test() {
    unimplemented!()
}

fn other() {
    todo!("implement this")
}
"#;
        
        let violations = analyzer.analyze(Path::new("test.rs"), content).unwrap();
        
        // Should find both macros
        assert_eq!(violations.len(), 2);
        assert!(violations.iter().any(|v| v.rule_id.contains("unimplemented")));
        assert!(violations.iter().any(|v| v.rule_id.contains("todo")));
    }
    
    #[test]
    fn test_find_empty_ok_returns() {
        let analyzer = RustAnalyzer::new();
        let content = r#"
fn empty_function() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn proper_function() -> Result<(), Box<dyn std::error::Error>> {
    println!("doing work");
    Ok(())
}
"#;
        
        let violations = analyzer.analyze(Path::new("test.rs"), content).unwrap();
        
        // Should find the empty function but not the proper one
        let empty_violations: Vec<_> = violations.iter()
            .filter(|v| v.rule_id == "empty_ok_return")
            .collect();
        
        assert_eq!(empty_violations.len(), 1);
        assert!(empty_violations[0].message.contains("empty_function"));
    }
    
    #[test]
    fn test_skip_test_files() {
        let analyzer = RustAnalyzer::new(); // analyze_tests = false
        let content = r#"
fn regular_function() {
    unimplemented!() // This should be caught
}

#[test]
fn test_something() {
    unimplemented!() // This should be ignored
}
"#;
        
        let violations = analyzer.analyze(Path::new("test.rs"), content).unwrap();
        
        // Should find the macro in regular function but not in test
        let macro_violations: Vec<_> = violations.iter()
            .filter(|v| v.rule_id.contains("unimplemented"))
            .collect();
        
        assert_eq!(macro_violations.len(), 1);
    }
    
    #[test]
    fn test_cdd_compliance_check() {
        let analyzer = RustAnalyzer::new();
        
        // File without CDD header
        let content_without_header = "fn main() {}";
        let violations = analyzer.analyze(Path::new("src/main.rs"), content_without_header).unwrap();
        assert!(violations.iter().any(|v| v.rule_id == "cdd_header_missing"));
        
        // File with CDD header
        let content_with_header = r#"
//! This module does something
//! 
//! CDD Principle: Domain Model - This exemplifies clean domain logic
//! - Specific implementation details
//! - Key design decisions

fn main() {}
"#;
        let violations = analyzer.analyze(Path::new("src/main.rs"), content_with_header).unwrap();
        assert!(!violations.iter().any(|v| v.rule_id == "cdd_header_missing"));
    }
    
    #[test]
    fn test_invalid_rust_syntax() {
        let analyzer = RustAnalyzer::new();
        let invalid_content = "this is not valid rust syntax {{{";
        
        // Should not panic and return empty violations
        let violations = analyzer.analyze(Path::new("invalid.rs"), invalid_content).unwrap();
        assert!(violations.is_empty());
    }
}