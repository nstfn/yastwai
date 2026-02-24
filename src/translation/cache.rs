/*!
 * Translation caching functionality.
 *
 * This module provides a two-tier caching system for translations:
 * - L1: In-memory cache for fast access during current session
 * - L2: Database-backed cache for cross-session persistence
 *
 * This avoids redundant API calls and improves performance significantly,
 * especially for repeated translations of common phrases.
 */

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use log::debug;

use crate::database::models::CacheRecord;
use crate::database::repository::Repository;

/// Cache key combining source text, source language, and target language
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    /// Source text to translate
    source_text: String,

    /// Source language code
    source_language: String,

    /// Target language code
    target_language: String,
}

impl CacheKey {
    /// Create a new cache key
    pub fn new(source_text: &str, source_language: &str, target_language: &str) -> Self {
        Self {
            source_text: source_text.to_string(),
            source_language: source_language.to_string(),
            target_language: target_language.to_string(),
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// L1 (in-memory) cache hits
    pub l1_hits: usize,
    /// L1 (in-memory) cache misses
    pub l1_misses: usize,
    /// L2 (database) cache hits
    pub l2_hits: usize,
    /// L2 (database) cache misses
    pub l2_misses: usize,
    /// Total entries in L1 cache
    pub l1_entries: usize,
    /// Total entries in L2 cache
    pub l2_entries: i64,
}

impl CacheStats {
    /// Calculate total hit rate
    pub fn hit_rate(&self) -> f64 {
        let total_requests = self.l1_hits + self.l1_misses;
        if total_requests == 0 {
            return 0.0;
        }
        let total_hits = self.l1_hits + self.l2_hits;
        (total_hits as f64 / total_requests as f64) * 100.0
    }

    /// Get summary string
    pub fn summary(&self) -> String {
        format!(
            "Cache: L1 {}/{} hits, L2 {}/{} hits, {:.1}% overall hit rate",
            self.l1_hits,
            self.l1_hits + self.l1_misses,
            self.l2_hits,
            self.l2_hits + self.l2_misses,
            self.hit_rate()
        )
    }
}

/// Configuration for the translation cache
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Whether in-memory caching is enabled
    pub l1_enabled: bool,
    /// Whether database caching is enabled
    pub l2_enabled: bool,
    /// Maximum entries in L1 cache (0 = unlimited)
    pub l1_max_entries: usize,
    /// Provider name for cache key differentiation
    pub provider: String,
    /// Model name for cache key differentiation
    pub model: String,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_enabled: true,
            l2_enabled: true,
            l1_max_entries: 10000,
            provider: String::new(),
            model: String::new(),
        }
    }
}

/// Two-tier translation cache for storing and retrieving translations
pub struct TranslationCache {
    /// L1: In-memory cache storage
    l1_cache: Arc<RwLock<HashMap<CacheKey, String>>>,

    /// L2: Database repository (optional)
    l2_repo: Option<Repository>,

    /// Cache statistics
    stats: Arc<RwLock<CacheStats>>,

    /// Cache configuration
    config: CacheConfig,
}

impl TranslationCache {
    /// Create a new translation cache with just L1 (in-memory) caching
    pub fn new(enabled: bool) -> Self {
        Self {
            l1_cache: Arc::new(RwLock::new(HashMap::new())),
            l2_repo: None,
            stats: Arc::new(RwLock::new(CacheStats::default())),
            config: CacheConfig {
                l1_enabled: enabled,
                l2_enabled: false,
                ..Default::default()
            },
        }
    }

    /// Create a cache with both L1 and L2 tiers
    pub fn new_with_db(config: CacheConfig, repo: Repository) -> Self {
        Self {
            l1_cache: Arc::new(RwLock::new(HashMap::new())),
            l2_repo: Some(repo),
            stats: Arc::new(RwLock::new(CacheStats::default())),
            config,
        }
    }

    /// Create a cache with custom configuration but no L2
    pub fn with_config(config: CacheConfig) -> Self {
        Self {
            l1_cache: Arc::new(RwLock::new(HashMap::new())),
            l2_repo: None,
            stats: Arc::new(RwLock::new(CacheStats::default())),
            config,
        }
    }

    /// Get a translation from the cache
    ///
    /// First checks L1 (in-memory), then L2 (database) if configured.
    pub async fn get(
        &self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
    ) -> Option<String> {
        // Check L1 first
        if self.config.l1_enabled {
            let key = CacheKey::new(source_text, source_language, target_language);
            let cache = self.l1_cache.read().await;

            if let Some(translation) = cache.get(&key) {
                // L1 hit
                let mut stats = self.stats.write().await;
                stats.l1_hits += 1;

                debug!(
                    "L1 cache hit for '{}' ({} -> {})",
                    truncate_text(source_text, 30),
                    source_language,
                    target_language
                );

                return Some(translation.clone());
            }

            // L1 miss
            let mut stats = self.stats.write().await;
            stats.l1_misses += 1;
        }

        // Check L2 if enabled
        if self.config.l2_enabled {
            if let Some(ref repo) = self.l2_repo {
                match repo
                    .get_cached_translation(
                        source_text,
                        source_language,
                        target_language,
                        &self.config.provider,
                        &self.config.model,
                    )
                    .await
                {
                    Ok(Some(translation)) => {
                        // L2 hit - also store in L1 for faster future access
                        let mut stats = self.stats.write().await;
                        stats.l2_hits += 1;

                        debug!(
                            "L2 cache hit for '{}' ({} -> {})",
                            truncate_text(source_text, 30),
                            source_language,
                            target_language
                        );

                        // Promote to L1
                        if self.config.l1_enabled {
                            let key = CacheKey::new(source_text, source_language, target_language);
                            let mut cache = self.l1_cache.write().await;
                            cache.insert(key, translation.clone());
                        }

                        return Some(translation);
                    }
                    Ok(None) => {
                        // L2 miss
                        let mut stats = self.stats.write().await;
                        stats.l2_misses += 1;
                    }
                    Err(e) => {
                        debug!("L2 cache lookup error: {}", e);
                    }
                }
            }
        }

        debug!(
            "Cache miss for '{}' ({} -> {})",
            truncate_text(source_text, 30),
            source_language,
            target_language
        );

        None
    }

    /// Store a translation in the cache
    ///
    /// Stores in both L1 and L2 if configured.
    pub async fn store(
        &self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
        translation: &str,
    ) {
        // Store in L1
        if self.config.l1_enabled {
            let key = CacheKey::new(source_text, source_language, target_language);
            let mut cache = self.l1_cache.write().await;

            // Check size limit
            if self.config.l1_max_entries > 0 && cache.len() >= self.config.l1_max_entries {
                // Simple eviction: remove a random entry
                // In a production system, you'd use LRU or similar
                if let Some(old_key) = cache.keys().next().cloned() {
                    cache.remove(&old_key);
                }
            }

            cache.insert(key, translation.to_string());

            debug!(
                "L1 cached translation for '{}' ({} -> {})",
                truncate_text(source_text, 30),
                source_language,
                target_language
            );
        }

        // Store in L2
        if self.config.l2_enabled {
            if let Some(ref repo) = self.l2_repo {
                let hash = Repository::hash_text(source_text);
                let record = CacheRecord::new(
                    hash,
                    source_text.to_string(),
                    source_language.to_string(),
                    target_language.to_string(),
                    translation.to_string(),
                    self.config.provider.clone(),
                    self.config.model.clone(),
                );

                if let Err(e) = repo.cache_translation(&record).await {
                    debug!("L2 cache store error: {}", e);
                } else {
                    debug!(
                        "L2 cached translation for '{}' ({} -> {})",
                        truncate_text(source_text, 30),
                        source_language,
                        target_language
                    );
                }
            }
        }
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let mut stats = self.stats.read().await.clone();

        // Update L1 entry count
        stats.l1_entries = self.l1_cache.read().await.len();

        // Update L2 entry count if available
        if let Some(ref repo) = self.l2_repo {
            if let Ok(cache_stats) = repo.get_cache_stats().await {
                stats.l2_entries = cache_stats.total_entries;
            }
        }

        stats
    }

    /// Clear the L1 cache
    pub async fn clear_l1(&self) {
        let mut cache = self.l1_cache.write().await;
        cache.clear();

        let mut stats = self.stats.write().await;
        stats.l1_hits = 0;
        stats.l1_misses = 0;

        debug!("L1 cache cleared");
    }

    /// Clear both L1 and L2 caches
    pub async fn clear_all(&self) {
        self.clear_l1().await;

        if let Some(ref repo) = self.l2_repo {
            if let Err(e) = repo.clear_cache().await {
                debug!("Failed to clear L2 cache: {}", e);
            } else {
                debug!("L2 cache cleared");
            }
        }

        let mut stats = self.stats.write().await;
        *stats = CacheStats::default();
    }

    /// Check if caching is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.l1_enabled || self.config.l2_enabled
    }

    /// Check if L2 (database) caching is enabled
    pub fn has_l2(&self) -> bool {
        self.config.l2_enabled && self.l2_repo.is_some()
    }

    /// Warm L1 cache from L2 for a given language pair
    ///
    /// Loads the most frequently used translations from the database into
    /// the in-memory cache for faster access during translation.
    ///
    /// Returns the number of entries loaded into L1.
    pub async fn warm_from_l2(
        &self,
        source_language: &str,
        target_language: &str,
        limit: usize,
    ) -> usize {
        if !self.config.l1_enabled || !self.config.l2_enabled {
            return 0;
        }

        let repo = match &self.l2_repo {
            Some(r) => r,
            None => return 0,
        };

        let entries = match repo
            .get_recent_cache_entries(
                source_language,
                target_language,
                &self.config.provider,
                &self.config.model,
                limit,
            )
            .await
        {
            Ok(e) => e,
            Err(e) => {
                debug!("Failed to fetch L2 entries for warming: {}", e);
                return 0;
            }
        };

        let count = entries.len();
        if count == 0 {
            return 0;
        }

        let mut cache = self.l1_cache.write().await;
        for entry in entries {
            let key = CacheKey::new(
                &entry.source_text,
                &entry.source_language,
                &entry.target_language,
            );
            cache.insert(key, entry.translated_text);
        }

        debug!(
            "Warmed L1 cache with {} entries for {} -> {}",
            count, source_language, target_language
        );

        count
    }
}

impl Default for TranslationCache {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Clone for TranslationCache {
    fn clone(&self) -> Self {
        Self {
            l1_cache: self.l1_cache.clone(),
            l2_repo: self.l2_repo.clone(),
            stats: self.stats.clone(),
            config: self.config.clone(),
        }
    }
}

/// Truncate text to a maximum length with ellipsis
fn truncate_text(text: &str, max_length: usize) -> String {
    if text.len() <= max_length {
        text.to_string()
    } else {
        format!("{}...", crate::utils::truncate_utf8(text, max_length))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_l1_cache_store_and_get_should_work() {
        let cache = TranslationCache::new(true);

        cache.store("Hello", "en", "fr", "Bonjour").await;

        let result = cache.get("Hello", "en", "fr").await;

        assert!(result.is_some());
        assert_eq!(result.unwrap(), "Bonjour");
    }

    #[tokio::test]
    async fn test_l1_cache_get_missing_should_return_none() {
        let cache = TranslationCache::new(true);

        let result = cache.get("NotCached", "en", "fr").await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_disabled_cache_should_not_store() {
        let cache = TranslationCache::new(false);

        cache.store("Hello", "en", "fr", "Bonjour").await;

        let result = cache.get("Hello", "en", "fr").await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats_should_track_hits_and_misses() {
        let cache = TranslationCache::new(true);

        // Store a translation
        cache.store("Hello", "en", "fr", "Bonjour").await;

        // Hit
        cache.get("Hello", "en", "fr").await;

        // Miss
        cache.get("World", "en", "fr").await;

        let stats = cache.stats().await;

        assert_eq!(stats.l1_hits, 1);
        assert_eq!(stats.l1_misses, 1);
    }

    #[tokio::test]
    async fn test_clear_l1_should_empty_cache() {
        let cache = TranslationCache::new(true);

        cache.store("Hello", "en", "fr", "Bonjour").await;
        cache.clear_l1().await;

        let result = cache.get("Hello", "en", "fr").await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_with_l2_should_work() {
        let repo = Repository::new_in_memory().expect("Failed to create test repo");

        let config = CacheConfig {
            l1_enabled: true,
            l2_enabled: true,
            l1_max_entries: 100,
            provider: "test".to_string(),
            model: "test-model".to_string(),
        };

        let cache = TranslationCache::new_with_db(config, repo);

        // Store
        cache.store("Hello", "en", "fr", "Bonjour").await;

        // Clear L1 to force L2 lookup
        cache.clear_l1().await;

        // Should still find it in L2
        let result = cache.get("Hello", "en", "fr").await;

        assert!(result.is_some());
        assert_eq!(result.unwrap(), "Bonjour");
    }

    #[tokio::test]
    async fn test_l2_hit_should_promote_to_l1() {
        let repo = Repository::new_in_memory().expect("Failed to create test repo");

        let config = CacheConfig {
            l1_enabled: true,
            l2_enabled: true,
            l1_max_entries: 100,
            provider: "test".to_string(),
            model: "test-model".to_string(),
        };

        let cache = TranslationCache::new_with_db(config, repo);

        // Store (goes to L1 and L2)
        cache.store("Hello", "en", "fr", "Bonjour").await;

        // Clear L1
        cache.clear_l1().await;

        // First access - L2 hit, should promote to L1
        let _ = cache.get("Hello", "en", "fr").await;

        // Second access - should be L1 hit
        let _ = cache.get("Hello", "en", "fr").await;

        let stats = cache.stats().await;

        // Second access should have been L1 hit
        assert!(stats.l1_hits >= 1);
    }

    #[tokio::test]
    async fn test_l1_max_entries_should_evict() {
        let config = CacheConfig {
            l1_enabled: true,
            l2_enabled: false,
            l1_max_entries: 2,
            provider: String::new(),
            model: String::new(),
        };

        let cache = TranslationCache::with_config(config);

        // Store 3 entries (max is 2)
        cache.store("A", "en", "fr", "A_fr").await;
        cache.store("B", "en", "fr", "B_fr").await;
        cache.store("C", "en", "fr", "C_fr").await;

        let stats = cache.stats().await;

        // Should only have 2 entries (one was evicted)
        assert!(stats.l1_entries <= 2);
    }

    #[test]
    fn test_cache_stats_hit_rate_should_calculate_correctly() {
        let stats = CacheStats {
            l1_hits: 3,
            l1_misses: 7,
            l2_hits: 2,
            l2_misses: 0,
            ..Default::default()
        };

        // Total requests = 3 + 7 = 10
        // Total hits = 3 + 2 = 5
        // Hit rate = 50%
        assert!((stats.hit_rate() - 50.0).abs() < 0.01);
    }
}
