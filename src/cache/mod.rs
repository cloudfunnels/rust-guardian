//! File hash caching for incremental checks
//!
//! Architecture: Infrastructure Layer - Cache provides performance optimization without affecting domain logic
//! - FileCache acts as a repository for file metadata and analysis results
//! - Hash-based validation ensures cache coherence with minimal overhead
//! - Domain objects remain pure while infrastructure handles caching concerns

use crate::domain::violations::{GuardianError, GuardianResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Cache for storing file analysis results and metadata
#[derive(Debug)]
pub struct FileCache {
    /// Path to the cache file
    cache_path: PathBuf,
    /// In-memory cache data
    data: CacheData,
    /// Whether the cache has been modified
    dirty: bool,
}

/// Serializable cache data structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CacheData {
    /// Cache format version for migration support
    version: u32,
    /// Configuration fingerprint when cache was created
    config_fingerprint: Option<String>,
    /// Cached file entries
    files: HashMap<PathBuf, FileEntry>,
    /// Cache metadata
    metadata: CacheMetadata,
}

/// Metadata about the cache itself
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheMetadata {
    /// When the cache was created
    created_at: u64,
    /// When the cache was last updated
    updated_at: u64,
    /// Number of cache hits since creation
    hits: u64,
    /// Number of cache misses since creation
    misses: u64,
}

/// Cached information about a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// SHA-256 hash of file content
    pub content_hash: String,
    /// File size in bytes
    pub size: u64,
    /// Last modified timestamp
    pub modified_at: u64,
    /// Number of violations found in this file
    pub violation_count: usize,
    /// When this file was last analyzed
    pub analyzed_at: u64,
    /// Configuration fingerprint when analysis was done
    pub config_fingerprint: String,
}

impl FileCache {
    /// Create a new file cache with the given cache file path
    pub fn new<P: AsRef<Path>>(cache_path: P) -> Self {
        Self {
            cache_path: cache_path.as_ref().to_path_buf(),
            data: CacheData::default(),
            dirty: false,
        }
    }

    /// Load cache from disk, creating it if it doesn't exist
    pub fn load(&mut self) -> GuardianResult<()> {
        if self.cache_path.exists() {
            let content = fs::read_to_string(&self.cache_path)
                .map_err(|e| GuardianError::cache(format!("Failed to read cache file: {e}")))?;

            self.data = serde_json::from_str(&content)
                .map_err(|e| GuardianError::cache(format!("Failed to parse cache file: {e}")))?;

            // Migrate cache format if needed
            self.migrate_if_needed()?;
        } else {
            // Create new cache
            self.data = CacheData {
                version: 1,
                config_fingerprint: None,
                files: HashMap::new(),
                metadata: CacheMetadata {
                    created_at: current_timestamp(),
                    updated_at: current_timestamp(),
                    hits: 0,
                    misses: 0,
                },
            };
            self.dirty = true;
        }

        // Self-validation after load
        self.verify_integrity_on_operation()?;
        Ok(())
    }

    /// Save cache to disk if it has been modified
    pub fn save(&mut self) -> GuardianResult<()> {
        if !self.dirty {
            return Ok(());
        }

        // Self-validation before save
        self.verify_integrity_on_operation()?;

        // Update metadata
        self.data.metadata.updated_at = current_timestamp();

        // Ensure cache directory exists
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                GuardianError::cache(format!("Failed to create cache directory: {e}"))
            })?;
        }

        // Serialize and write cache
        let content = serde_json::to_string_pretty(&self.data)
            .map_err(|e| GuardianError::cache(format!("Failed to serialize cache: {e}")))?;

        fs::write(&self.cache_path, content)
            .map_err(|e| GuardianError::cache(format!("Failed to write cache file: {e}")))?;

        self.dirty = false;
        Ok(())
    }

    /// Check if a file needs to be re-analyzed
    pub fn needs_analysis<P: AsRef<Path>>(
        &mut self,
        file_path: P,
        config_fingerprint: &str,
    ) -> GuardianResult<bool> {
        let file_path = file_path.as_ref();

        // Get current file metadata
        let metadata = fs::metadata(file_path).map_err(|e| {
            GuardianError::cache(format!(
                "Failed to get file metadata for {}: {}",
                file_path.display(),
                e
            ))
        })?;

        let current_size = metadata.len();
        let current_modified = metadata
            .modified()
            .map_err(|e| GuardianError::cache(format!("Failed to get modification time: {e}")))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                GuardianError::cache(format!("Invalid system time before Unix epoch: {e}"))
            })?
            .as_secs();

        // Check if we have a cache entry
        if let Some(entry) = self.data.files.get(file_path) {
            // Check if file has been modified
            if entry.size != current_size || entry.modified_at != current_modified {
                self.data.metadata.misses += 1;
                self.dirty = true;
                return Ok(true);
            }

            // Check if configuration has changed
            if entry.config_fingerprint != config_fingerprint {
                self.data.metadata.misses += 1;
                self.dirty = true;
                return Ok(true);
            }

            // Verify content hash to be absolutely sure
            let current_hash = self.calculate_file_hash(file_path)?;
            if entry.content_hash != current_hash {
                self.data.metadata.misses += 1;
                self.dirty = true;
                return Ok(true);
            }

            // Cache hit!
            self.data.metadata.hits += 1;
            self.dirty = true;
            Ok(false)
        } else {
            // No cache entry - needs analysis
            self.data.metadata.misses += 1;
            self.dirty = true;
            Ok(true)
        }
    }

    /// Update cache entry for a file after analysis
    pub fn update_entry<P: AsRef<Path>>(
        &mut self,
        file_path: P,
        violation_count: usize,
        config_fingerprint: &str,
    ) -> GuardianResult<()> {
        let file_path = file_path.as_ref();

        // Get current file metadata
        let metadata = fs::metadata(file_path)
            .map_err(|e| GuardianError::cache(format!("Failed to get file metadata: {e}")))?;

        let content_hash = self.calculate_file_hash(file_path)?;

        let entry = FileEntry {
            content_hash,
            size: metadata.len(),
            modified_at: metadata
                .modified()
                .map_err(|e| GuardianError::cache(format!("Failed to get modification time: {e}")))?
                .duration_since(UNIX_EPOCH)
                .map_err(|e| {
                    GuardianError::cache(format!("Invalid system time before Unix epoch: {e}"))
                })?
                .as_secs(),
            violation_count,
            analyzed_at: current_timestamp(),
            config_fingerprint: config_fingerprint.to_string(),
        };

        self.data.files.insert(file_path.to_path_buf(), entry);
        self.dirty = true;

        Ok(())
    }

    /// Get cache statistics
    pub fn statistics(&self) -> CacheStatistics {
        CacheStatistics {
            total_files: self.data.files.len(),
            cache_hits: self.data.metadata.hits,
            cache_misses: self.data.metadata.misses,
            hit_rate: if self.data.metadata.hits + self.data.metadata.misses > 0 {
                (self.data.metadata.hits as f64)
                    / ((self.data.metadata.hits + self.data.metadata.misses) as f64)
            } else {
                0.0
            },
            created_at: self.data.metadata.created_at,
            updated_at: self.data.metadata.updated_at,
        }
    }

    /// Clear the entire cache
    pub fn clear(&mut self) -> GuardianResult<()> {
        self.data.files.clear();
        self.data.metadata.hits = 0;
        self.data.metadata.misses = 0;
        self.data.metadata.updated_at = current_timestamp();
        self.dirty = true;

        // Remove cache file if it exists
        if self.cache_path.exists() {
            fs::remove_file(&self.cache_path)
                .map_err(|e| GuardianError::cache(format!("Failed to remove cache file: {e}")))?;
        }

        Ok(())
    }

    /// Remove cache entries for files that no longer exist
    pub fn cleanup(&mut self) -> GuardianResult<usize> {
        let mut removed = 0;
        let mut to_remove = Vec::new();

        for file_path in self.data.files.keys() {
            if !file_path.exists() {
                to_remove.push(file_path.clone());
            }
        }

        for file_path in to_remove {
            self.data.files.remove(&file_path);
            removed += 1;
        }

        if removed > 0 {
            self.dirty = true;
        }

        Ok(removed)
    }

    /// Update configuration fingerprint
    pub fn set_config_fingerprint(&mut self, fingerprint: String) {
        if self.data.config_fingerprint.as_ref() != Some(&fingerprint) {
            self.data.config_fingerprint = Some(fingerprint);
            self.dirty = true;
        }
    }

    /// Calculate SHA-256 hash of file content
    fn calculate_file_hash<P: AsRef<Path>>(&self, file_path: P) -> GuardianResult<String> {
        let mut file = File::open(&file_path)
            .map_err(|e| GuardianError::cache(format!("Failed to open file for hashing: {e}")))?;

        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];

        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| {
                GuardianError::cache(format!("Failed to read file for hashing: {e}"))
            })?;

            if bytes_read == 0 {
                break;
            }

            hasher.update(&buffer[..bytes_read]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Migrate cache format if needed
    fn migrate_if_needed(&mut self) -> GuardianResult<()> {
        const CURRENT_VERSION: u32 = 1;

        if self.data.version < CURRENT_VERSION {
            tracing::info!(
                "Migrating cache from version {} to {}",
                self.data.version,
                CURRENT_VERSION
            );

            match self.data.version {
                0 => {
                    // Migration from version 0 to 1
                    // Add any migration logic here
                    self.data.version = 1;
                    self.dirty = true;
                }
                _ => {
                    return Err(GuardianError::cache(format!(
                        "Unsupported cache version: {}. Please delete the cache file.",
                        self.data.version
                    )));
                }
            }
        }

        Ok(())
    }
}

impl Default for CacheMetadata {
    fn default() -> Self {
        let now = current_timestamp();
        Self { created_at: now, updated_at: now, hits: 0, misses: 0 }
    }
}

/// Cache performance statistics
#[derive(Debug, Clone)]
pub struct CacheStatistics {
    pub total_files: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub hit_rate: f64,
    pub created_at: u64,
    pub updated_at: u64,
}

impl CacheStatistics {
    /// Format statistics for display
    pub fn format_display(&self) -> String {
        format!(
            "Cache: {} files, {:.1}% hit rate ({} hits, {} misses)",
            self.total_files,
            self.hit_rate * 100.0,
            self.cache_hits,
            self.cache_misses
        )
    }
}

/// Get current timestamp as seconds since Unix epoch
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect(
            "System time should be after Unix epoch - this indicates a serious system clock issue",
        )
        .as_secs()
}

impl FileCache {
    /// Self-validation - Infrastructure validates its own coherence
    ///
    /// Architecture Principle: Self-validating infrastructure - Components ensure their own correctness
    /// - Cache validates its invariants during normal operation
    /// - File hash verification maintains cache coherence
    /// - Metadata consistency checked on each operation
    /// - No external test dependencies - system is self-aware
    pub fn validate_cache_coherence(&self) -> GuardianResult<()> {
        // Validate cache metadata invariants
        if self.data.metadata.hits + self.data.metadata.misses > 0 {
            let calculated_hit_rate = (self.data.metadata.hits as f64)
                / ((self.data.metadata.hits + self.data.metadata.misses) as f64);

            if !(0.0..=1.0).contains(&calculated_hit_rate) {
                return Err(GuardianError::cache(
                    "Cache hit rate coherence violation - cache integrity compromised".to_string(),
                ));
            }
        }

        // Validate temporal coherence - created_at should be <= updated_at
        if self.data.metadata.created_at > self.data.metadata.updated_at {
            return Err(GuardianError::cache(
                "Temporal coherence violation - cache timeline is inconsistent".to_string(),
            ));
        }

        // Validate file entry coherence for existing files
        for (file_path, entry) in &self.data.files {
            if file_path.exists() {
                // Verify file metadata coherence if file still exists
                if let Ok(metadata) = std::fs::metadata(file_path) {
                    if entry.size != metadata.len() {
                        tracing::warn!(
                            "File size mismatch detected for {}: cached {} vs actual {}",
                            file_path.display(),
                            entry.size,
                            metadata.len()
                        );
                    }
                }
            }
        }

        tracing::debug!(
            "Cache coherence validated: {} files, {:.1}% hit rate",
            self.data.files.len(),
            if self.data.metadata.hits + self.data.metadata.misses > 0 {
                (self.data.metadata.hits as f64)
                    / ((self.data.metadata.hits + self.data.metadata.misses) as f64)
                    * 100.0
            } else {
                0.0
            }
        );

        Ok(())
    }

    /// Integrity verification during normal operations
    ///
    /// Architecture Principle: Continuous self-monitoring - Infrastructure monitors its own health
    fn verify_integrity_on_operation(&self) -> GuardianResult<()> {
        // Quick integrity checks that run during normal operations
        if self.data.version == 0 {
            return Err(GuardianError::cache(
                "Cache version coherence violation - invalid version state".to_string(),
            ));
        }

        // Ensure statistics are coherent
        if self.data.metadata.hits > u64::MAX / 2 || self.data.metadata.misses > u64::MAX / 2 {
            tracing::warn!("Cache statistics approaching overflow - cache may need reset");
        }

        Ok(())
    }
}
