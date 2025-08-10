//! Rust Guardian CLI - Command-line interface for code quality enforcement
//! 
//! CDD Principle: Application Layer - CLI coordinates user interactions with domain services
//! - Translates user commands to domain operations  
//! - Handles external concerns like file I/O, process exit codes, and terminal output
//! - Provides clean separation between user interface and business logic

use rust_guardian::{
    GuardianValidator, GuardianConfig, ValidationOptions, AnalysisOptions, 
    OutputFormat, ReportOptions, Severity, GuardianResult, GuardianError
};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};
use std::process;


/// Rust Guardian - Dynamic code quality enforcement
#[derive(Parser)]
#[command(name = "rust-guardian")]
#[command(version = "0.1.0")]
#[command(about = "Dynamic code quality enforcement preventing incomplete or placeholder code")]
#[command(long_about = "Rust Guardian analyzes code for quality violations, placeholder implementations, and architectural compliance. Designed for autonomous agent workflows and CI/CD integration.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
    
    /// Configuration file path
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,
    
    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Check files for code quality violations
    Check {
        /// Paths to analyze (files or directories)
        paths: Vec<PathBuf>,
        
        /// Output format
        #[arg(short, long, value_enum, default_value = "human")]
        format: OutputFormatArg,
        
        /// Minimum severity level to report
        #[arg(short, long, value_enum)]
        severity: Option<SeverityArg>,
        
        /// Maximum number of violations to report
        #[arg(long)]
        max_violations: Option<usize>,
        
        /// Additional exclude patterns
        #[arg(long, action = clap::ArgAction::Append)]
        exclude: Vec<String>,
        
        /// Ignore .guardianignore files
        #[arg(long)]
        no_ignore: bool,
        
        /// Custom .guardianignore file
        #[arg(long)]
        guardianignore: Option<PathBuf>,
        
        /// Disable parallel processing
        #[arg(long)]
        no_parallel: bool,
        
        /// Fail on first error
        #[arg(long)]
        fail_fast: bool,
        
        /// Enable caching for better performance
        #[arg(long)]
        cache: bool,
        
        /// Custom cache file path
        #[arg(long)]
        cache_file: Option<PathBuf>,
    },
    
    /// Watch for file changes and run checks automatically
    Watch {
        /// Path to watch (defaults to current directory)
        path: Option<PathBuf>,
        
        /// File patterns to watch (glob patterns)
        #[arg(short, long, action = clap::ArgAction::Append)]
        pattern: Vec<String>,
        
        /// Debounce delay in milliseconds
        #[arg(long, default_value = "500")]
        delay: u64,
    },
    
    /// Validate configuration file
    ValidateConfig {
        /// Configuration file to validate
        config_file: Option<PathBuf>,
    },
    
    /// Explain what a specific rule does
    Explain {
        /// Rule ID to explain
        rule_id: String,
    },
    
    /// Show cache statistics
    Cache {
        #[command(subcommand)]
        action: CacheCommands,
    },
    
    /// List available rules and patterns
    Rules {
        /// Show only enabled rules
        #[arg(long)]
        enabled_only: bool,
        
        /// Filter by category
        #[arg(long)]
        category: Option<String>,
    },
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Show cache statistics
    Stats {
        /// Cache file path
        #[arg(long)]
        cache_file: Option<PathBuf>,
    },
    
    /// Clear the cache
    Clear {
        /// Cache file path
        #[arg(long)]
        cache_file: Option<PathBuf>,
    },
    
    /// Clean up stale cache entries
    Cleanup {
        /// Cache file path
        #[arg(long)]
        cache_file: Option<PathBuf>,
    },
}

#[derive(Copy, Clone, ValueEnum, PartialEq)]
enum OutputFormatArg {
    Human,
    Json,
    Junit,
    Sarif,
    Github,
}

impl From<OutputFormatArg> for OutputFormat {
    fn from(arg: OutputFormatArg) -> Self {
        match arg {
            OutputFormatArg::Human => OutputFormat::Human,
            OutputFormatArg::Json => OutputFormat::Json,
            OutputFormatArg::Junit => OutputFormat::Junit,
            OutputFormatArg::Sarif => OutputFormat::Sarif,
            OutputFormatArg::Github => OutputFormat::GitHub,
        }
    }
}

#[derive(Clone, ValueEnum)]
enum SeverityArg {
    Info,
    Warning,
    Error,
}

impl From<SeverityArg> for Severity {
    fn from(arg: SeverityArg) -> Self {
        match arg {
            SeverityArg::Info => Severity::Info,
            SeverityArg::Warning => Severity::Warning,
            SeverityArg::Error => Severity::Error,
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    
    // Initialize logging
    init_logging(cli.verbose);
    
    // Run the command and handle the result
    let result = run_command(cli).await;
    
    match result {
        Ok(exit_code) => {
            process::exit(exit_code);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

async fn run_command(cli: Cli) -> GuardianResult<i32> {
    match cli.command {
        Commands::Check {
            paths,
            format,
            severity,
            max_violations,
            exclude,
            no_ignore,
            guardianignore: _guardianignore,
            no_parallel,
            fail_fast,
            cache,
            cache_file,
        } => {
            run_check(
                cli.config,
                paths,
                format,
                severity,
                max_violations,
                exclude,
                no_ignore,
                no_parallel,
                fail_fast,
                cache,
                cache_file,
                !cli.no_color,
            ).await
        }
        Commands::Watch { path, pattern, delay } => {
            run_watch(path, pattern, delay).await
        }
        Commands::ValidateConfig { config_file } => {
            run_validate_config(config_file.or(cli.config))
        }
        Commands::Explain { rule_id } => {
            run_explain(rule_id)
        }
        Commands::Cache { action } => {
            run_cache_command(action).await
        }
        Commands::Rules { enabled_only, category } => {
            run_list_rules(cli.config, enabled_only, category)
        }
    }
}

async fn run_check(
    config_path: Option<PathBuf>,
    paths: Vec<PathBuf>,
    format: OutputFormatArg,
    severity: Option<SeverityArg>,
    max_violations: Option<usize>,
    exclude_patterns: Vec<String>,
    no_ignore: bool,
    no_parallel: bool,
    fail_fast: bool,
    use_cache: bool,
    cache_file: Option<PathBuf>,
    use_colors: bool,
) -> GuardianResult<i32> {
    // Load configuration
    let config = if let Some(config_path) = config_path {
        GuardianConfig::load_from_file(config_path)?
    } else {
        // Try to find default config file
        let default_configs = ["rust_guardian.yaml", "rust_guardian.yml", ".rust_guardian.yaml"];
        let mut config = None;
        
        for config_name in &default_configs {
            if Path::new(config_name).exists() {
                config = Some(GuardianConfig::load_from_file(config_name)?);
                break;
            }
        }
        
        config.unwrap_or_else(|| GuardianConfig::default())
    };
    
    // Create validator
    let mut validator = GuardianValidator::new_with_config(config)?;
    
    // Enable cache if requested
    if use_cache {
        let cache_path = cache_file.unwrap_or_else(|| {
            PathBuf::from(".rust").join("guardian_cache.json")
        });
        validator = validator.with_cache(cache_path)?;
    }
    
    // Use current directory if no paths specified
    let paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths
    };
    
    // Set up validation options
    let validation_options = ValidationOptions {
        use_cache,
        output_format: format.clone().into(),
        report_options: ReportOptions {
            use_colors,
            max_violations,
            min_severity: severity.map(|s| s.into()),
            ..Default::default()
        },
        analysis_options: AnalysisOptions {
            parallel: !no_parallel,
            fail_fast,
            exclude_patterns,
            ignore_ignore_files: no_ignore,
            ..Default::default()
        },
        ..Default::default()
    };
    
    // Run validation
    let report = validator.validate_with_options(paths, &validation_options).await?;
    
    // Format and output results
    let formatted = validator.format_report(&report, format.into())?;
    println!("{}", formatted);
    
    // Print cache statistics if caching is enabled
    if use_cache {
        if let Some(stats) = validator.cache_statistics() {
            if format == OutputFormatArg::Human {
                eprintln!("\n{}", stats.format_display());
            }
        }
    }
    
    // Save cache if enabled
    if use_cache {
        validator.save_cache()?;
    }
    
    // Return appropriate exit code
    if report.has_errors() {
        Ok(1) // Exit code 1 for errors
    } else {
        Ok(0) // Exit code 0 for success
    }
}

async fn run_watch(
    path: Option<PathBuf>,
    patterns: Vec<String>,
    delay_ms: u64,
) -> GuardianResult<i32> {
    use notify::{Watcher, RecursiveMode, Result as NotifyResult, Event};
    use std::sync::mpsc;
    use std::time::Duration;
    use std::thread;
    use std::io::{self, Write};
    
    let watch_path = path.unwrap_or_else(|| PathBuf::from("."));
    
    println!("🔍 Starting Rust Guardian watch mode...");
    println!("📂 Watching: {}", watch_path.display());
    
    // Set up file patterns to watch (default to Rust files if none specified)
    let watch_patterns = if patterns.is_empty() {
        vec!["**/*.rs".to_string()]
    } else {
        patterns
    };
    
    println!("🎯 Patterns: {}", watch_patterns.join(", "));
    println!("⏱️  Debounce delay: {}ms", delay_ms);
    println!("Press Ctrl+C to stop watching\\n");
    
    // Create a channel for file system events
    let (tx, rx) = mpsc::channel();
    
    // Create a watcher
    let mut watcher = notify::recommended_watcher(move |res: NotifyResult<Event>| {
        match res {
            Ok(event) => {
                if let Err(e) = tx.send(event) {
                    eprintln!("Error sending event: {}", e);
                }
            }
            Err(e) => eprintln!("Watch error: {}", e),
        }
    }).map_err(|e| GuardianError::config(format!("Failed to create file watcher: {}", e)))?;
    
    // Start watching the path
    watcher.watch(&watch_path, RecursiveMode::Recursive)
        .map_err(|e| GuardianError::config(format!("Failed to watch path '{}': {}", watch_path.display(), e)))?;
    
    // Track last run to implement debouncing
    let mut last_run = std::time::Instant::now();
    let debounce_duration = Duration::from_millis(delay_ms);
    
    // Run initial check
    println!("🚀 Running initial analysis...");
    run_watch_analysis_with_config(&watch_path, &watch_patterns, None).await?;
    
    // Main event loop
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                // Check for config file changes first
                if let Some(config_path) = is_config_change(&event) {
                    println!("🔄 Configuration file changed: {}", config_path.display());
                    println!("📝 Reloading configuration and running analysis...");
                    
                    // Clear terminal and run analysis with new config
                    print!("\\x1B[2J\\x1B[H"); // Clear screen and move cursor to top
                    io::stdout().flush().unwrap();
                    
                    if let Err(e) = run_watch_analysis_with_config(&watch_path, &watch_patterns, Some(&config_path)).await {
                        eprintln!("❌ Config reload and analysis failed: {}", e);
                    }
                    last_run = std::time::Instant::now();
                }
                // Otherwise check for regular file changes
                else if should_trigger_analysis(&event, &watch_patterns) {
                    let now = std::time::Instant::now();
                    if now.duration_since(last_run) >= debounce_duration {
                        // Clear terminal and run analysis
                        print!("\\x1B[2J\\x1B[H"); // Clear screen and move cursor to top
                        io::stdout().flush().unwrap();
                        
                        println!("📝 File changes detected, running analysis...");
                        if let Err(e) = run_watch_analysis_with_config(&watch_path, &watch_patterns, None).await {
                            eprintln!("❌ Analysis failed: {}", e);
                        }
                        last_run = now;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No events - continue watching
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("File watcher disconnected");
                break;
            }
        }
        
        // Small delay to prevent excessive CPU usage
        thread::sleep(Duration::from_millis(10));
    }
    
    Ok(0)
}

/// Check if an event should trigger analysis or config reload
fn should_trigger_analysis(event: &notify::Event, patterns: &[String]) -> bool {
    use notify::EventKind;
    
    // Only trigger on write/create/rename events
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {},
        _ => return false,
    }
    
    // Check if any affected path matches our patterns
    for path in &event.paths {
        let path_str = path.to_string_lossy();
        
        for pattern in patterns {
            if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                if glob_pattern.matches(&path_str) {
                    return true;
                }
            }
        }
    }
    
    false
}

/// Check if an event indicates a config file change
fn is_config_change(event: &notify::Event) -> Option<PathBuf> {
    use notify::EventKind;
    
    // Only trigger on write/create/rename events
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {},
        _ => return None,
    }
    
    // Check if any affected path is a config file
    for path in &event.paths {
        let file_name = path.file_name()?.to_str()?;
        
        // Check for common config file names
        if matches!(file_name, 
            "rust_guardian.yaml" | 
            "rust_guardian.yml" | 
            ".rust_guardian.yaml" | 
            ".rust_guardian.yml"
        ) {
            return Some(path.clone());
        }
    }
    
    None
}

/// Run analysis for watch mode with optional config file
async fn run_watch_analysis_with_config(
    watch_path: &Path, 
    _patterns: &[String], 
    config_path: Option<&Path>
) -> GuardianResult<()> {
    // Load configuration
    let config = if let Some(config_path) = config_path {
        match GuardianConfig::load_from_file(config_path) {
            Ok(config) => {
                println!("✅ Configuration reloaded from: {}", config_path.display());
                config
            }
            Err(e) => {
                eprintln!("⚠️  Failed to reload config from {}: {}", config_path.display(), e);
                eprintln!("   Using default configuration instead...");
                GuardianConfig::default()
            }
        }
    } else {
        // Try to find default config file
        let default_configs = ["rust_guardian.yaml", "rust_guardian.yml", ".rust_guardian.yaml"];
        let mut config = None;
        
        for config_name in &default_configs {
            if Path::new(config_name).exists() {
                match GuardianConfig::load_from_file(config_name) {
                    Ok(loaded_config) => {
                        config = Some(loaded_config);
                        break;
                    }
                    Err(e) => {
                        eprintln!("⚠️  Failed to load config from {}: {}", config_name, e);
                        continue;
                    }
                }
            }
        }
        
        config.unwrap_or_else(|| GuardianConfig::default())
    };
    
    // Create validator
    let mut validator = GuardianValidator::new_with_config(config)?;
    
    // Set up validation options for watch mode
    let validation_options = ValidationOptions {
        analysis_options: AnalysisOptions {
            parallel: true,
            fail_fast: false,
            exclude_patterns: vec![], // Use default exclusions
            ..Default::default()
        },
        report_options: ReportOptions {
            use_colors: true,
            show_suggestions: true,
            ..Default::default()
        },
        output_format: OutputFormat::Human,
        ..Default::default()
    };
    
    // Run validation
    match validator.validate_with_options(vec![watch_path], &validation_options).await {
        Ok(report) => {
            if report.has_violations() {
                let formatted = validator.format_report(&report, OutputFormat::Human)?;
                println!("{}", formatted);
                
                let error_count = report.summary.violations_by_severity.error;
                let warning_count = report.summary.violations_by_severity.warning;
                let info_count = report.summary.violations_by_severity.info;
                
                if error_count > 0 {
                    println!("\\n❌ Found {} error{}, {} warning{}, {} info", 
                        error_count, if error_count == 1 { "" } else { "s" },
                        warning_count, if warning_count == 1 { "" } else { "s" },
                        info_count);
                } else if warning_count > 0 {
                    println!("\\n⚠️  Found {} warning{}, {} info", 
                        warning_count, if warning_count == 1 { "" } else { "s" },
                        info_count);
                } else {
                    println!("\\n✅ Found {} info message{}", 
                        info_count, if info_count == 1 { "" } else { "s" });
                }
            } else {
                println!("✅ No code quality violations found");
            }
            
            println!("📊 Analyzed {} files in {:.1}s", 
                report.summary.total_files, 
                report.summary.execution_time_ms as f64 / 1000.0);
            println!("⌚ Watching for changes... (Press Ctrl+C to stop)\\n");
        }
        Err(e) => {
            eprintln!("❌ Analysis error: {}", e);
        }
    }
    
    Ok(())
}

/// Legacy wrapper for backward compatibility
async fn run_watch_analysis(watch_path: &Path, patterns: &[String]) -> GuardianResult<()> {
    run_watch_analysis_with_config(watch_path, patterns, None).await
}

fn run_validate_config(config_path: Option<PathBuf>) -> GuardianResult<i32> {
    let config_path = config_path.unwrap_or_else(|| PathBuf::from("rust_guardian.yaml"));
    
    println!("Validating configuration: {}", config_path.display());
    
    match GuardianConfig::load_from_file(&config_path) {
        Ok(config) => {
            println!("✅ Configuration is valid");
            
            // Show some statistics
            let total_categories = config.patterns.len();
            let enabled_categories = config.patterns.values().filter(|c| c.enabled).count();
            let total_rules: usize = config.patterns.values().map(|c| c.rules.len()).sum();
            let enabled_rules: usize = config.patterns.values()
                .filter(|c| c.enabled)
                .map(|c| c.rules.iter().filter(|r| r.enabled).count())
                .sum();
            
            println!("📊 Configuration summary:");
            println!("  Categories: {} total, {} enabled", total_categories, enabled_categories);
            println!("  Rules: {} total, {} enabled", total_rules, enabled_rules);
            println!("  Path patterns: {}", config.paths.patterns.len());
            
            Ok(0)
        }
        Err(e) => {
            eprintln!("❌ Configuration validation failed: {}", e);
            Ok(1)
        }
    }
}

fn run_explain(rule_id: String) -> GuardianResult<i32> {
    let config = GuardianConfig::default();
    
    // Find the rule in the configuration
    for (category_name, category) in &config.patterns {
        for rule in &category.rules {
            if rule.id == rule_id {
                println!("📖 Rule: {}", rule.id);
                println!("📂 Category: {}", category_name);
                println!("⚠️ Severity: {:?}", rule.severity.unwrap_or(category.severity));
                println!("🔍 Type: {:?}", rule.rule_type);
                println!("✅ Enabled: {}", rule.enabled);
                println!();
                println!("📝 Description:");
                println!("   {}", rule.message);
                println!();
                println!("🔎 Pattern:");
                println!("   {}", rule.pattern);
                
                if let Some(exclude) = &rule.exclude_if {
                    println!();
                    println!("🚫 Exclusions:");
                    if let Some(attr) = &exclude.attribute {
                        println!("   Attribute: {}", attr);
                    }
                    if exclude.in_tests {
                        println!("   Excluded in test files");
                    }
                    if let Some(patterns) = &exclude.file_patterns {
                        println!("   File patterns: {}", patterns.join(", "));
                    }
                }
                
                return Ok(0);
            }
        }
    }
    
    eprintln!("❌ Rule '{}' not found", rule_id);
    println!();
    println!("Available rules:");
    
    for (category_name, category) in &config.patterns {
        println!("  {}:", category_name);
        for rule in &category.rules {
            println!("    - {}", rule.id);
        }
    }
    
    Ok(1)
}

async fn run_cache_command(action: CacheCommands) -> GuardianResult<i32> {
    match action {
        CacheCommands::Stats { cache_file } => {
            let cache_path = cache_file.unwrap_or_else(|| {
                PathBuf::from(".rust").join("guardian_cache.json")
            });
            
            if !cache_path.exists() {
                println!("No cache file found at {}", cache_path.display());
                return Ok(1);
            }
            
            let mut cache = rust_guardian::FileCache::new(&cache_path);
            cache.load()?;
            
            let stats = cache.statistics();
            println!("📊 Cache Statistics");
            println!("   File: {}", cache_path.display());
            println!("   {}", stats.format_display());
            println!("   Created: {}", format_timestamp(stats.created_at));
            println!("   Updated: {}", format_timestamp(stats.updated_at));
            
            Ok(0)
        }
        CacheCommands::Clear { cache_file } => {
            let cache_path = cache_file.unwrap_or_else(|| {
                PathBuf::from(".rust").join("guardian_cache.json")
            });
            
            let mut cache = rust_guardian::FileCache::new(&cache_path);
            cache.load()?;
            cache.clear()?;
            
            println!("✅ Cache cleared: {}", cache_path.display());
            Ok(0)
        }
        CacheCommands::Cleanup { cache_file } => {
            let cache_path = cache_file.unwrap_or_else(|| {
                PathBuf::from(".rust").join("guardian_cache.json")
            });
            
            if !cache_path.exists() {
                println!("No cache file found at {}", cache_path.display());
                return Ok(1);
            }
            
            let mut cache = rust_guardian::FileCache::new(&cache_path);
            cache.load()?;
            let removed = cache.cleanup()?;
            cache.save()?;
            
            println!("✅ Cleaned up {} stale cache entries", removed);
            Ok(0)
        }
    }
}

fn run_list_rules(
    config_path: Option<PathBuf>,
    enabled_only: bool,
    category_filter: Option<String>,
) -> GuardianResult<i32> {
    let config = if let Some(path) = config_path {
        GuardianConfig::load_from_file(path)?
    } else {
        GuardianConfig::default()
    };
    
    println!("📋 Available Rules\n");
    
    for (category_name, category) in &config.patterns {
        // Apply category filter
        if let Some(ref filter) = category_filter {
            if category_name != filter {
                continue;
            }
        }
        
        // Skip disabled categories if enabled_only is true
        if enabled_only && !category.enabled {
            continue;
        }
        
        let status = if category.enabled { "✅" } else { "❌" };
        println!("{}📂 {} ({})", status, category_name, category.severity.as_str());
        
        for rule in &category.rules {
            // Skip disabled rules if enabled_only is true
            if enabled_only && !rule.enabled {
                continue;
            }
            
            let rule_status = if rule.enabled { "✅" } else { "❌" };
            let severity = rule.severity.unwrap_or(category.severity);
            
            println!("  {}🔍 {} [{}] - {}", 
                rule_status, 
                rule.id, 
                severity.as_str(),
                rule.message
            );
        }
        println!();
    }
    
    Ok(0)
}

fn init_logging(verbose: bool) {
    let level = if verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::WARN
    };
    
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();
}

fn format_timestamp(timestamp: u64) -> String {
    use chrono::{Utc, TimeZone};
    
    let dt = Utc.timestamp_opt(timestamp as i64, 0)
        .single()
        .unwrap_or_else(|| Utc::now());
    
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;
    
    #[tokio::test]
    async fn test_check_command() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        
        fs::write(&test_file, "// TODO: implement this\nfn main() {}").unwrap();
        
        // Test basic check
        let result = run_check(
            None,
            vec![test_file],
            OutputFormatArg::Json,
            None,
            None,
            vec![],
            false,
            false,
            false,
            false,
            None,
            false,
        ).await;
        
        // Should find violations (exit code 1)
        assert_eq!(result.unwrap(), 1);
    }
    
    #[test]
    fn test_validate_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("test_config.yaml");
        
        // Create a valid config file
        let config = GuardianConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        fs::write(&config_file, yaml).unwrap();
        
        let result = run_validate_config(Some(config_file));
        assert_eq!(result.unwrap(), 0);
    }
    
    #[test]
    fn test_explain_rule() {
        let result = run_explain("todo_comments".to_string());
        assert_eq!(result.unwrap(), 0);
        
        let result = run_explain("nonexistent_rule".to_string());
        assert_eq!(result.unwrap(), 1);
    }
    
    #[test]
    fn test_list_rules() {
        let result = run_list_rules(None, false, None);
        assert_eq!(result.unwrap(), 0);
        
        let result = run_list_rules(None, true, Some("placeholders".to_string()));
        assert_eq!(result.unwrap(), 0);
    }
}