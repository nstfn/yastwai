/*!
 * Batch translation processing.
 *
 * This module contains functionality for processing translations in batches,
 * with support for concurrency, progress tracking, error handling, and
 * quality validation.
 *
 * ## Performance Optimization
 *
 * This module uses a parallel entry-level translation strategy:
 * - Instead of sending many entries in ONE API request, we send multiple smaller
 *   concurrent requests (each with 1-5 entries)
 * - This maximizes LLM throughput by keeping multiple inference requests in flight
 * - Configurable via `parallel_entries_per_request` and `max_concurrent_requests`
 */

use anyhow::{anyhow, Result};
use log::{debug, error};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use futures::stream::{self, StreamExt};
use futures::future::join_all;

use crate::subtitle_processor::SubtitleEntry;
use crate::validation::MarkerValidator;

use super::core::{LogEntry, TokenUsageStats, TranslationService};
use super::formatting::FormatPreserver;

/// Configuration for parallel translation
#[derive(Clone, Debug)]
pub struct ParallelTranslationConfig {
    /// Maximum number of concurrent API requests
    pub max_concurrent_requests: usize,
    /// Number of entries to include in each API request (1-10 recommended)
    pub entries_per_request: usize,
    /// Whether to use the legacy combined-batch mode (false = use parallel mode)
    pub use_legacy_batch_mode: bool,
    /// Number of previous entries to include as context for consistency (0 = disabled)
    pub context_entries_count: usize,
}

impl Default for ParallelTranslationConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 5,
            entries_per_request: 3,
            use_legacy_batch_mode: false,
            context_entries_count: 3,
        }
    }
}

/// Batch translator for processing subtitle entries in batches
pub struct BatchTranslator {
    /// The translation service to use
    service: TranslationService,

    /// Maximum number of concurrent requests
    max_concurrent_requests: usize,

    /// Whether to retry individual entries on batch failure
    retry_individual_entries: bool,

    /// Parallel translation configuration
    parallel_config: ParallelTranslationConfig,
}

impl BatchTranslator {
    /// Create a new batch translator
    pub fn new(service: TranslationService) -> Self {
        // Determine optimal parallel config based on service settings
        let max_concurrent = service.options.max_concurrent_requests.max(5);

        Self {
            max_concurrent_requests: max_concurrent,
            retry_individual_entries: service.options.retry_individual_entries,
            parallel_config: ParallelTranslationConfig {
                max_concurrent_requests: max_concurrent,
                entries_per_request: 3, // Sweet spot for most LLMs
                use_legacy_batch_mode: false, // Use new parallel mode by default
                context_entries_count: 3, // Default: include 3 previous entries as context
            },
            service,
        }
    }

    /// Create a new batch translator with custom parallel configuration
    pub fn with_parallel_config(service: TranslationService, parallel_config: ParallelTranslationConfig) -> Self {
        Self {
            max_concurrent_requests: parallel_config.max_concurrent_requests,
            retry_individual_entries: service.options.retry_individual_entries,
            parallel_config,
            service,
        }
    }
    
    /// Translate batches of subtitle entries with optional batch completion callback
    pub async fn translate_batches_with_callback<F, C>(
        &self,
        batches: &[Vec<SubtitleEntry>],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
        progress_callback: F,
        batch_complete_callback: Option<C>,
    ) -> Result<(Vec<SubtitleEntry>, TokenUsageStats)>
    where
        F: Fn(usize, usize) + Clone + Send + 'static,
        C: Fn(Vec<SubtitleEntry>) + Clone + Send + Sync + 'static,
    {
        self.translate_batches_internal(
            batches,
            source_language,
            target_language,
            log_capture,
            progress_callback,
            batch_complete_callback,
        )
        .await
    }

    /// Translate batches of subtitle entries
    pub async fn translate_batches(
        &self,
        batches: &[Vec<SubtitleEntry>],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
        progress_callback: impl Fn(usize, usize) + Clone + Send + 'static,
    ) -> Result<(Vec<SubtitleEntry>, TokenUsageStats)> {
        self.translate_batches_internal::<_, fn(Vec<SubtitleEntry>)>(
            batches,
            source_language,
            target_language,
            log_capture,
            progress_callback,
            None,
        )
        .await
    }

    /// Internal implementation for batch translation
    async fn translate_batches_internal<F, C>(
        &self,
        batches: &[Vec<SubtitleEntry>],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
        progress_callback: F,
        batch_complete_callback: Option<C>,
    ) -> Result<(Vec<SubtitleEntry>, TokenUsageStats)>
    where
        F: Fn(usize, usize) + Clone + Send + 'static,
        C: Fn(Vec<SubtitleEntry>) + Clone + Send + Sync + 'static,
    {
        // Use parallel mode for better performance unless explicitly disabled
        if !self.parallel_config.use_legacy_batch_mode {
            return self.translate_parallel(
                batches,
                source_language,
                target_language,
                log_capture,
                progress_callback,
                batch_complete_callback,
            ).await;
        }
        // Initialize token usage stats
        let mut token_stats = TokenUsageStats::with_provider_info(
            self.service.config.provider.to_lowercase_string(),
            self.service.config.get_model()
        );
        
        // Create a semaphore to limit concurrent requests
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent_requests));
        
        // Track progress
        let total_batches = batches.len();
        let processed_batches = Arc::new(AtomicUsize::new(0));
        
        // Wrap callback in Arc for sharing across async tasks
        let batch_callback = batch_complete_callback.map(Arc::new);

        // Process batches concurrently
        let results = stream::iter(batches.iter().enumerate())
            .map(|(batch_index, batch)| {
                let service = self.service.clone();
                let semaphore = semaphore.clone();
                let log_capture = log_capture.clone();
                let processed_batches = processed_batches.clone();
                let progress_callback = progress_callback.clone();
                let batch_callback = batch_callback.clone();
                let source_language = source_language.to_string();
                let target_language = target_language.to_string();
                let retry_individual_entries = self.retry_individual_entries;

                async move {
                    // Acquire a permit from the semaphore
                    let _permit = match semaphore.acquire().await {
                        Ok(permit) => permit,
                        Err(e) => {
                            // Log error and return early - semaphore acquisition should not fail under normal conditions
                            let mut logs = log_capture.lock().await;
                            logs.push(LogEntry {
                                level: "ERROR".to_string(),
                                message: format!("Failed to acquire semaphore permit: {}", e),
                            });
                            return (batch_index, Err(anyhow!("Failed to acquire semaphore permit: {}", e)));
                        }
                    };
                    
                    // Log batch processing start
                    {
                        let mut logs = log_capture.lock().await;
                        logs.push(LogEntry {
                            level: "INFO".to_string(),
                            message: format!("Processing batch {} of {}", batch_index + 1, total_batches),
                        });
                    }
                    
                    // Process the batch
                    let start_time = Instant::now();
                    let result = service.translate_batch_with_recovery(
                        batch,
                        &source_language,
                        &target_language,
                        log_capture.clone(),
                        retry_individual_entries
                    ).await;
                    
                    // Update progress
                    let current = processed_batches.fetch_add(1, Ordering::SeqCst) + 1;
                    progress_callback(current, total_batches);
                    
                    // Log batch processing completion and invoke callback
                    {
                        let mut logs = log_capture.lock().await;
                        let duration = start_time.elapsed();
                        match &result {
                            Ok((entries, _)) => {
                                logs.push(LogEntry {
                                    level: "INFO".to_string(),
                                    message: format!(
                                        "Batch {} completed in {:?}",
                                        batch_index + 1,
                                        duration
                                    ),
                                });

                                // Invoke batch complete callback if provided
                                if let Some(ref callback) = batch_callback {
                                    callback(entries.clone());
                                }
                            }
                            Err(e) => {
                                logs.push(LogEntry {
                                    level: "ERROR".to_string(),
                                    message: format!("Batch {} failed: {}", batch_index + 1, e),
                                });
                            }
                        }
                    }

                    (batch_index, result)
                }
            })
            .buffer_unordered(self.max_concurrent_requests)
            .collect::<Vec<_>>()
            .await;
        
        // Process results
        let mut all_entries = Vec::new();
        let mut errors = Vec::new();
        
        // Sort results by batch index to maintain original order
        let mut sorted_results = results;
        sorted_results.sort_by_key(|(idx, _)| *idx);
        
        for (batch_idx, result) in sorted_results {
            match result {
                Ok((entries, token_usage)) => {
                    all_entries.extend(entries);
                    
                    // Aggregate token usage if available
                    if let Some((prompt_tokens, completion_tokens, duration)) = token_usage {
                        token_stats.add_token_usage(prompt_tokens, completion_tokens);
                        if let Some(dur) = duration {
                            token_stats.api_duration += dur;
                        }
                    }
                },
                Err(e) => {
                    errors.push(format!("Batch {} failed: {}", batch_idx + 1, e));
                }
            }
        }
        
        // Check if any batches failed
        if !errors.is_empty() {
            let error_message = format!("Failed to translate all batches: {}", errors.join("; "));
            error!("{}", error_message);
            return Err(anyhow!(error_message));
        }
        
        // Return all translated entries and token stats
        Ok((all_entries, token_stats))
    }
    
    /// High-performance parallel translation
    /// 
    /// This method translates entries using multiple concurrent API requests,
    /// with each request handling a small number of entries. This maximizes
    /// throughput by keeping multiple LLM inference requests in flight.
    async fn translate_parallel<F, C>(
        &self,
        batches: &[Vec<SubtitleEntry>],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
        progress_callback: F,
        batch_complete_callback: Option<C>,
    ) -> Result<(Vec<SubtitleEntry>, TokenUsageStats)>
    where
        F: Fn(usize, usize) + Clone + Send + 'static,
        C: Fn(Vec<SubtitleEntry>) + Clone + Send + Sync + 'static,
    {
        // Flatten all batches into a single list of entries
        let all_entries: Vec<SubtitleEntry> = batches.iter().flat_map(|batch| batch.iter().cloned()).collect();
        
        let total_entries = all_entries.len();
        if total_entries == 0 {
            return Ok((Vec::new(), TokenUsageStats::new()));
        }
        
        // Initialize token usage stats
        let token_stats = Arc::new(Mutex::new(TokenUsageStats::with_provider_info(
            self.service.config.provider.to_lowercase_string(),
            self.service.config.get_model()
        )));
        
        // Create work items: small chunks of entries for parallel processing
        // Each work item includes context entries from previous chunks for consistency
        let entries_per_request = self.parallel_config.entries_per_request.max(1);
        let context_count = self.parallel_config.context_entries_count;
        
        // First, create the basic chunks
        let chunks: Vec<Vec<SubtitleEntry>> = all_entries
            .chunks(entries_per_request)
            .map(|chunk| chunk.to_vec())
            .collect();
        
        // Now create work items with context from previous entries
        // Work item format: (index, entries_to_translate, context_entries)
        let work_items: Vec<(usize, Vec<SubtitleEntry>, Vec<SubtitleEntry>)> = chunks
            .iter()
            .enumerate()
            .map(|(idx, chunk)| {
                // Calculate context: get entries from the end of previous chunks
                let context = if context_count > 0 && idx > 0 {
                    // Calculate start position in all_entries for context
                    let chunk_start = idx * entries_per_request;
                    let context_start = chunk_start.saturating_sub(context_count);
                    all_entries[context_start..chunk_start].to_vec()
                } else {
                    Vec::new()
                };
                (idx, chunk.clone(), context)
            })
            .collect();
        
        let total_work_items = work_items.len();
        
        // Log parallel translation start
        {
            let mut logs = log_capture.lock().await;
            let context_msg = if context_count > 0 {
                format!(" with {} context entries", context_count)
            } else {
                String::new()
            };
            logs.push(LogEntry {
                level: "INFO".to_string(),
                message: format!(
                    "Starting parallel translation: {} entries in {} work items ({} concurrent){}",
                    total_entries,
                    total_work_items,
                    self.parallel_config.max_concurrent_requests,
                    context_msg
                ),
            });
        }
        
        // Note: Parallel mode info is logged to log_capture only to avoid breaking progress bar
        // The app_controller logs this info before the progress bar starts
        
        // Create a semaphore to limit concurrent requests
        let semaphore = Arc::new(Semaphore::new(self.parallel_config.max_concurrent_requests));
        
        // Track progress
        let processed_items = Arc::new(AtomicUsize::new(0));
        
        // Wrap callback in Arc for sharing across async tasks
        let batch_callback = batch_complete_callback.map(Arc::new);
        
        // Process work items in parallel
        let results = stream::iter(work_items.into_iter())
            .map(|(work_idx, entries, context_entries)| {
                let service = self.service.clone();
                let semaphore = semaphore.clone();
                let log_capture = log_capture.clone();
                let token_stats = token_stats.clone();
                let processed_items = processed_items.clone();
                let progress_callback = progress_callback.clone();
                let batch_callback = batch_callback.clone();
                let source_language = source_language.to_string();
                let target_language = target_language.to_string();

                async move {
                    // Acquire a permit from the semaphore
                    let _permit = match semaphore.acquire().await {
                        Ok(permit) => permit,
                        Err(e) => {
                            return (work_idx, Err(anyhow!("Failed to acquire semaphore permit: {}", e)));
                        }
                    };
                    
                    let start_time = Instant::now();
                    
                    // Translate entries individually or as a small batch
                    let result = if entries.len() == 1 {
                        // Single entry - translate directly with context
                        service.translate_single_entry_with_context(
                            &entries[0],
                            &context_entries,
                            &source_language,
                            &target_language,
                            log_capture.clone(),
                        ).await.map(|entry| vec![entry])
                    } else {
                        // Small batch - use optimized batch translation with context
                        service.translate_small_batch_with_context(
                            &entries,
                            &context_entries,
                            &source_language,
                            &target_language,
                            log_capture.clone(),
                        ).await
                    };
                    
                    let duration = start_time.elapsed();
                    
                    // Update token stats if we have usage info
                    if let Ok(ref translated) = result {
                        let mut stats = token_stats.lock().await;
                        stats.api_duration += duration;
                        
                        // Invoke batch complete callback if provided
                        if let Some(ref callback) = batch_callback {
                            callback(translated.clone());
                        }
                    }
                    
                    // Update progress - report actual work items for accurate ETA
                    let current = processed_items.fetch_add(1, Ordering::SeqCst) + 1;
                    // Report work item progress directly for accurate ETA calculation
                    progress_callback(current, total_work_items);
                    
                    (work_idx, result.map(|entries| (entries, duration)))
                }
            })
            .buffer_unordered(self.parallel_config.max_concurrent_requests)
            .collect::<Vec<_>>()
            .await;
        
        // Collect and sort results
        let mut sorted_results = results;
        sorted_results.sort_by_key(|(idx, _)| *idx);
        
        let mut all_translated = Vec::with_capacity(total_entries);
        let mut errors = Vec::new();
        let mut total_duration = Duration::from_secs(0);
        
        for (work_idx, result) in sorted_results {
            match result {
                Ok((entries, duration)) => {
                    all_translated.extend(entries);
                    total_duration += duration;
                },
                Err(e) => {
                    errors.push(format!("Work item {} failed: {}", work_idx + 1, e));
                }
            }
        }
        
        // Log completion
        {
            let mut logs = log_capture.lock().await;
            if errors.is_empty() {
                logs.push(LogEntry {
                    level: "INFO".to_string(),
                    message: format!(
                        "Parallel translation completed: {} entries in {:?}",
                        all_translated.len(),
                        total_duration
                    ),
                });
            } else {
                logs.push(LogEntry {
                    level: "WARN".to_string(),
                    message: format!(
                        "Parallel translation completed with {} errors: {}",
                        errors.len(),
                        errors.join("; ")
                    ),
                });
            }
        }
        
        // Get final token stats
        let final_stats = token_stats.lock().await.clone();
        
        if !errors.is_empty() && all_translated.is_empty() {
            return Err(anyhow!("All translation requests failed: {}", errors.join("; ")));
        }
        
        Ok((all_translated, final_stats))
    }
}

impl TranslationService {
    /// Translate a single entry in parallel mode (no markers needed)
    async fn translate_single_entry_parallel(
        &self,
        entry: &SubtitleEntry,
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
    ) -> Result<SubtitleEntry> {
        // Skip empty entries
        if entry.text.trim().is_empty() {
            return Ok(entry.clone());
        }
        
        // Translate the entry text directly
        let (translated_text, _) = self.translate_text_with_usage(
            &entry.text,
            source_language,
            target_language,
            Some(log_capture),
        ).await?;
        
        // Apply format preservation
        let final_text = if self.options.preserve_formatting {
            FormatPreserver::preserve_formatting(&entry.text, &translated_text)
        } else {
            translated_text
        };
        
        // Create translated entry
        let mut translated_entry = entry.clone();
        translated_entry.text = final_text;
        
        Ok(translated_entry)
    }
    
    /// Translate a small batch of entries in parallel mode
    async fn translate_small_batch_parallel(
        &self,
        entries: &[SubtitleEntry],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
    ) -> Result<Vec<SubtitleEntry>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        
        // For small batches (2-5 entries), use simplified markers
        let mut combined_text = String::new();
        for (idx, entry) in entries.iter().enumerate() {
            combined_text.push_str(&format!("[{}]\n{}\n", idx + 1, entry.text));
        }
        
        // Translate combined text
        let (translated_text, _) = self.translate_text_with_usage(
            &combined_text,
            source_language,
            target_language,
            Some(log_capture.clone()),
        ).await?;
        
        // Parse results back into entries
        let mut translated_entries = Vec::with_capacity(entries.len());
        
        // Track entries that need individual translation due to marker parsing failures
        let mut entries_needing_retry: Vec<(usize, &SubtitleEntry)> = Vec::new();
        
        for (idx, entry) in entries.iter().enumerate() {
            // Try to find the translated text for this entry
            let marker = format!("[{}]", idx + 1);
            let next_marker = format!("[{}]", idx + 2);
            
            let entry_text = if let Some(start_pos) = translated_text.find(&marker) {
                let text_start = start_pos + marker.len();
                let text_end = translated_text[text_start..]
                    .find(&next_marker)
                    .map(|pos| text_start + pos)
                    .unwrap_or(translated_text.len());
                
                let extracted = translated_text[text_start..text_end].trim().to_string();
                
                // Validate extracted text - if it's empty or looks like a marker, it's invalid
                if extracted.is_empty() || extracted.starts_with('[') {
                    debug!("Marker [{}] found but extracted text is invalid, will retry individually", idx + 1);
                    entries_needing_retry.push((idx, entry));
                    None
                } else {
                    Some(extracted)
                }
            } else {
                // Marker not found - need to retry this entry individually
                debug!("Marker [{}] not found in response, will retry individually", idx + 1);
                entries_needing_retry.push((idx, entry));
                None
            };
            
            if let Some(text) = entry_text {
                // Apply format preservation
                let final_text = if self.options.preserve_formatting {
                    FormatPreserver::preserve_formatting(&entry.text, &text)
                } else {
                    text
                };
                
                let mut translated_entry = entry.clone();
                translated_entry.text = final_text;
                translated_entries.push((idx, translated_entry));
            }
        }
        
        // Retry failed entries concurrently (not sequentially)
        if !entries_needing_retry.is_empty() {
            {
                let mut logs = log_capture.lock().await;
                logs.push(LogEntry {
                    level: "WARN".to_string(),
                    message: format!(
                        "Retrying {} entries concurrently due to marker parsing failures",
                        entries_needing_retry.len()
                    ),
                });
            }

            // Launch all retry translations concurrently
            let retry_futures: Vec<_> = entries_needing_retry
                .into_iter()
                .map(|(idx, entry)| {
                    let source_lang = source_language.to_string();
                    let target_lang = target_language.to_string();
                    let log_cap = log_capture.clone();
                    let entry_clone = entry.clone();
                    async move {
                        match self.translate_single_entry_parallel(
                            &entry_clone,
                            &source_lang,
                            &target_lang,
                            log_cap,
                        ).await {
                            Ok(translated_entry) => (idx, Ok(translated_entry)),
                            Err(e) => (idx, Err((e, entry_clone))),
                        }
                    }
                })
                .collect();

            // Await all retries concurrently
            let retry_results = join_all(retry_futures).await;

            // Process results
            for (idx, result) in retry_results {
                match result {
                    Ok(translated_entry) => {
                        translated_entries.push((idx, translated_entry));
                    }
                    Err((e, original_entry)) => {
                        // Log the error and use original as last resort
                        {
                            let mut logs = log_capture.lock().await;
                            logs.push(LogEntry {
                                level: "ERROR".to_string(),
                                message: format!(
                                    "Failed to translate entry {} individually: {}. Using original text.",
                                    idx + 1, e
                                ),
                            });
                        }
                        translated_entries.push((idx, original_entry));
                    }
                }
            }
        }
        
        // Sort by original index to maintain order
        translated_entries.sort_by_key(|(idx, _)| *idx);
        
        // Extract just the entries
        Ok(translated_entries.into_iter().map(|(_, entry)| entry).collect())
    }
    
    /// Translate a single entry with context from previous entries
    /// 
    /// Context entries are included in the prompt to help maintain consistency
    /// (formal/informal address, character genders, tone) but are NOT translated.
    async fn translate_single_entry_with_context(
        &self,
        entry: &SubtitleEntry,
        context_entries: &[SubtitleEntry],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
    ) -> Result<SubtitleEntry> {
        // Skip empty entries
        if entry.text.trim().is_empty() {
            return Ok(entry.clone());
        }
        
        // If no context, use the regular method
        if context_entries.is_empty() {
            return self.translate_single_entry_parallel(entry, source_language, target_language, log_capture).await;
        }
        
        // Build prompt with context (compact format to reduce tokens)
        let mut prompt = String::new();
        prompt.push_str("[CONTEXT]\n");
        for ctx_entry in context_entries {
            prompt.push_str(&format!("{}\n", ctx_entry.text));
        }
        prompt.push_str("[TRANSLATE]\n");
        prompt.push_str(&entry.text);
        
        // Translate with context
        let (translated_text, _) = self.translate_text_with_usage(
            &prompt,
            source_language,
            target_language,
            Some(log_capture),
        ).await?;
        
        // Extract only the translated part (remove any context that might have been included)
        let final_translated = Self::extract_translated_portion(&translated_text, &entry.text);
        
        // Apply format preservation
        let final_text = if self.options.preserve_formatting {
            FormatPreserver::preserve_formatting(&entry.text, &final_translated)
        } else {
            final_translated
        };
        
        // Create translated entry
        let mut translated_entry = entry.clone();
        translated_entry.text = final_text;
        
        Ok(translated_entry)
    }
    
    /// Translate a small batch of entries with context from previous entries
    /// 
    /// Context entries are included in the prompt to help maintain consistency
    /// (formal/informal address, character genders, tone) but are NOT translated.
    async fn translate_small_batch_with_context(
        &self,
        entries: &[SubtitleEntry],
        context_entries: &[SubtitleEntry],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
    ) -> Result<Vec<SubtitleEntry>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        
        // If no context, use the regular method
        if context_entries.is_empty() {
            return self.translate_small_batch_parallel(entries, source_language, target_language, log_capture).await;
        }
        
        // Build prompt with context section and entries to translate
        // Use minimal headers to reduce token overhead
        let mut combined_text = String::new();
        
        // Add context section (compact format)
        combined_text.push_str("[CONTEXT]\n");
        for ctx_entry in context_entries {
            combined_text.push_str(&format!("{}\n", ctx_entry.text));
        }
        combined_text.push_str("[TRANSLATE]\n");
        
        // Add entries to translate with markers
        for (idx, entry) in entries.iter().enumerate() {
            combined_text.push_str(&format!("[{}] {}\n", idx + 1, entry.text));
        }
        
        // Translate combined text
        let (translated_text, _) = self.translate_text_with_usage(
            &combined_text,
            source_language,
            target_language,
            Some(log_capture.clone()),
        ).await?;
        
        // Parse results back into entries (same logic as translate_small_batch_parallel)
        let mut translated_entries = Vec::with_capacity(entries.len());
        let mut entries_needing_retry: Vec<(usize, &SubtitleEntry)> = Vec::new();
        
        for (idx, entry) in entries.iter().enumerate() {
            let marker = format!("[{}]", idx + 1);
            let next_marker = format!("[{}]", idx + 2);
            
            let entry_text = if let Some(start_pos) = translated_text.find(&marker) {
                let text_start = start_pos + marker.len();
                let text_end = translated_text[text_start..]
                    .find(&next_marker)
                    .map(|pos| text_start + pos)
                    .unwrap_or(translated_text.len());
                
                let extracted = translated_text[text_start..text_end].trim().to_string();
                
                if extracted.is_empty() || extracted.starts_with('[') || extracted.starts_with("===") {
                    debug!("Marker [{}] found but extracted text is invalid, will retry individually", idx + 1);
                    entries_needing_retry.push((idx, entry));
                    None
                } else {
                    Some(extracted)
                }
            } else {
                debug!("Marker [{}] not found in response, will retry individually", idx + 1);
                entries_needing_retry.push((idx, entry));
                None
            };
            
            if let Some(text) = entry_text {
                let final_text = if self.options.preserve_formatting {
                    FormatPreserver::preserve_formatting(&entry.text, &text)
                } else {
                    text
                };
                
                let mut translated_entry = entry.clone();
                translated_entry.text = final_text;
                translated_entries.push((idx, translated_entry));
            }
        }
        
        // Retry failed entries concurrently with context
        if !entries_needing_retry.is_empty() {
            {
                let mut logs = log_capture.lock().await;
                logs.push(LogEntry {
                    level: "WARN".to_string(),
                    message: format!(
                        "Retrying {} entries concurrently due to marker parsing failures",
                        entries_needing_retry.len()
                    ),
                });
            }

            // Launch all retry translations concurrently
            let retry_futures: Vec<_> = entries_needing_retry
                .into_iter()
                .map(|(idx, entry)| {
                    let source_lang = source_language.to_string();
                    let target_lang = target_language.to_string();
                    let log_cap = log_capture.clone();
                    let ctx_entries = context_entries.to_vec();
                    let entry_clone = entry.clone();
                    async move {
                        match self.translate_single_entry_with_context(
                            &entry_clone,
                            &ctx_entries,
                            &source_lang,
                            &target_lang,
                            log_cap,
                        ).await {
                            Ok(translated_entry) => (idx, Ok(translated_entry)),
                            Err(e) => (idx, Err((e, entry_clone))),
                        }
                    }
                })
                .collect();

            // Await all retries concurrently
            let retry_results = join_all(retry_futures).await;

            // Process results
            for (idx, result) in retry_results {
                match result {
                    Ok(translated_entry) => {
                        translated_entries.push((idx, translated_entry));
                    }
                    Err((e, original_entry)) => {
                        {
                            let mut logs = log_capture.lock().await;
                            logs.push(LogEntry {
                                level: "ERROR".to_string(),
                                message: format!(
                                    "Failed to translate entry {} individually: {}. Using original text.",
                                    idx + 1, e
                                ),
                            });
                        }
                        translated_entries.push((idx, original_entry));
                    }
                }
            }
        }

        translated_entries.sort_by_key(|(idx, _)| *idx);
        Ok(translated_entries.into_iter().map(|(_, entry)| entry).collect())
    }

    /// Extract the translated portion from a response that may include context
    fn extract_translated_portion(response: &str, _original: &str) -> String {
        // Try to find the [TRANSLATE] marker (compact format)
        if let Some(pos) = response.find("[TRANSLATE]") {
            let after_marker = pos + "[TRANSLATE]".len();
            return response[after_marker..].trim().to_string();
        }
        
        // Fallback: try legacy "=== TRANSLATE" format
        if let Some(pos) = response.find("=== TRANSLATE") {
            if let Some(newline_pos) = response[pos..].find('\n') {
                return response[pos + newline_pos..].trim().to_string();
            }
        }
        
        // If no marker found, try to detect and skip any context echoing
        let lines: Vec<&str> = response.lines().collect();
        
        // Look for any TRANSLATE markers in lines
        for (i, line) in lines.iter().enumerate() {
            if line.contains("TRANSLATE") || line.starts_with("[TRANSLATE") {
                return lines[i + 1..].join("\n").trim().to_string();
            }
        }
        
        // Otherwise return the full response
        response.trim().to_string()
    }
    
    /// Translate a batch of subtitle entries with recovery options
    pub async fn translate_batch_with_recovery(
        &self,
        batch: &[SubtitleEntry],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
        retry_individual_entries: bool
    ) -> Result<(Vec<SubtitleEntry>, Option<(Option<u64>, Option<u64>, Option<Duration>)>)> {
        // Skip empty batches
        if batch.is_empty() {
            return Ok((Vec::new(), None));
        }
        
        // Try to translate the entire batch first
        let batch_result = self.translate_batch(batch, source_language, target_language, log_capture.clone()).await;
        
        // If batch translation succeeded or we don't want to retry individual entries, return the result
        if batch_result.is_ok() || !retry_individual_entries {
            return batch_result;
        }
        
        // If batch translation failed, try to translate each entry individually
        {
            let mut logs = log_capture.lock().await;
            logs.push(LogEntry {
                level: "WARN".to_string(),
                message: "Batch translation failed, retrying individual entries".to_string(),
            });
        }
        
        let mut translated_entries = Vec::with_capacity(batch.len());
        let mut errors = Vec::new();
        
        for (idx, entry) in batch.iter().enumerate() {
            let result = self.translate_single_entry(entry, source_language, target_language, log_capture.clone()).await;
            
            match result {
                Ok(translated_entry) => {
                    translated_entries.push(translated_entry);
                },
                Err(e) => {
                    let error_message = format!("Failed to translate entry {}: {}", idx + 1, e);
                    errors.push(error_message.clone());
                    
                    {
                        let mut logs = log_capture.lock().await;
                        logs.push(LogEntry {
                            level: "ERROR".to_string(),
                            message: error_message,
                        });
                    }
                    
                    // Add the original entry as a fallback
                    translated_entries.push(entry.clone());
                }
            }
        }
        
        // Log any errors
        if !errors.is_empty() {
            // Remove direct warning that breaks the progress bar
            // warn!("Some entries failed to translate: {}", errors.join("; "));
            
            // Instead, add this warning to the log capture
            let error_message = format!("Some entries failed to translate: {}", errors.join("; "));
            let mut logs = log_capture.lock().await;
            logs.push(LogEntry {
                level: "WARN".to_string(),
                message: error_message,
            });
        }
        
        // For individual entries, we don't have aggregated token usage easily available
        // This could be improved in the future to track individual entry token usage
        Ok((translated_entries, None))
    }
    
    /// Translate a batch of subtitle entries
    async fn translate_batch(
        &self,
        batch: &[SubtitleEntry],
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>,
    ) -> Result<(Vec<SubtitleEntry>, Option<(Option<u64>, Option<u64>, Option<Duration>)>)> {
        // Skip empty batches
        if batch.is_empty() {
            return Ok((Vec::new(), None));
        }

        // Combine all entries into a single text for translation
        let mut combined_text = String::new();
        let mut entry_indices = Vec::new();

        for (idx, entry) in batch.iter().enumerate() {
            // Add a marker before each entry
            combined_text.push_str(&format!("<<ENTRY_{}>>", idx));
            combined_text.push('\n');

            // Add the entry text
            combined_text.push_str(&entry.text);
            combined_text.push('\n');

            // Store the entry index
            entry_indices.push(idx);
        }

        // Add a final marker
        combined_text.push_str("<<END>>");

        // Translate the combined text
        let (translated_text, token_usage) = self
            .translate_text_with_usage(
                &combined_text,
                source_language,
                target_language,
                Some(log_capture.clone()),
            )
            .await?;

        // Validate markers in the response
        let marker_validation = MarkerValidator::validate(&translated_text, batch.len());

        if !marker_validation.passed() {
            // Log validation failure
            {
                let mut logs = log_capture.lock().await;
                logs.push(LogEntry {
                    level: "WARN".to_string(),
                    message: format!(
                        "Marker validation failed: {}",
                        marker_validation.error_message.as_deref().unwrap_or("Unknown error")
                    ),
                });

                if !marker_validation.missing_indices.is_empty() {
                    logs.push(LogEntry {
                        level: "DEBUG".to_string(),
                        message: format!(
                            "Missing markers for entries: {:?}",
                            marker_validation.missing_indices
                        ),
                    });
                }
            }

            // Try to recover partial results
            if marker_validation.found_indices.is_empty() {
                return Err(anyhow!(
                    "No valid markers found in response. Translation may be truncated."
                ));
            }

            debug!(
                "Attempting partial recovery: {}/{} markers found",
                marker_validation.found_indices.len(),
                batch.len()
            );
        }

        // Split the translated text back into entries
        let mut translated_entries: Vec<(usize, SubtitleEntry)> = Vec::with_capacity(batch.len());
        let mut entries_needing_retry: Vec<usize> = Vec::new();
        let mut current_idx = 0;

        for idx in entry_indices {
            let start_marker = format!("<<ENTRY_{}>>", idx);
            let end_marker = if idx == batch.len() - 1 {
                "<<END>>".to_string()
            } else {
                format!("<<ENTRY_{}>>", idx + 1)
            };

            // Find the start and end positions
            let start_pos = translated_text[current_idx..]
                .find(&start_marker)
                .map(|pos| pos + current_idx + start_marker.len());

            let start_pos = match start_pos {
                Some(pos) => pos,
                None => {
                    // Marker not found - need to retry this entry individually
                    {
                        let mut logs = log_capture.lock().await;
                        logs.push(LogEntry {
                            level: "WARN".to_string(),
                            message: format!(
                                "Entry {} marker not found, will retry individually",
                                idx
                            ),
                        });
                    }
                    entries_needing_retry.push(idx);
                    continue;
                }
            };

            let end_pos = translated_text[start_pos..].find(&end_marker).map(|pos| pos + start_pos);

            let end_pos = match end_pos {
                Some(pos) => pos,
                None => {
                    // End marker not found - try to use remaining text
                    {
                        let mut logs = log_capture.lock().await;
                        logs.push(LogEntry {
                            level: "WARN".to_string(),
                            message: format!(
                                "End marker for entry {} not found, using remaining text",
                                idx
                            ),
                        });
                    }
                    translated_text.len()
                }
            };

            // Extract the translated text for this entry
            let mut entry_text = translated_text[start_pos..end_pos].trim().to_string();

            // Validate extracted text - if empty or looks invalid, retry individually
            if entry_text.is_empty() || entry_text.starts_with("<<") {
                {
                    let mut logs = log_capture.lock().await;
                    logs.push(LogEntry {
                        level: "WARN".to_string(),
                        message: format!(
                            "Entry {} extracted text is invalid, will retry individually",
                            idx
                        ),
                    });
                }
                entries_needing_retry.push(idx);
                continue;
            }

            // Apply format preservation if enabled
            if self.options.preserve_formatting {
                entry_text = FormatPreserver::preserve_formatting(&batch[idx].text, &entry_text);
            }

            // Create a new entry with the translated text
            let mut translated_entry = batch[idx].clone();
            translated_entry.text = entry_text;

            // Add the translated entry with its index
            translated_entries.push((idx, translated_entry));

            // Update the current position
            current_idx = end_pos;
        }

        // Retry failed entries concurrently
        if !entries_needing_retry.is_empty() {
            {
                let mut logs = log_capture.lock().await;
                logs.push(LogEntry {
                    level: "WARN".to_string(),
                    message: format!(
                        "Retrying {} entries concurrently due to marker parsing failures",
                        entries_needing_retry.len()
                    ),
                });
            }

            // Launch all retry translations concurrently
            let retry_futures: Vec<_> = entries_needing_retry
                .into_iter()
                .map(|idx| {
                    let entry = batch[idx].clone();
                    let source_lang = source_language.to_string();
                    let target_lang = target_language.to_string();
                    let log_cap = log_capture.clone();
                    async move {
                        match self.translate_single_entry(
                            &entry,
                            &source_lang,
                            &target_lang,
                            log_cap,
                        ).await {
                            Ok(translated_entry) => (idx, Ok(translated_entry)),
                            Err(e) => (idx, Err((e, entry))),
                        }
                    }
                })
                .collect();

            // Await all retries concurrently
            let retry_results = join_all(retry_futures).await;

            // Process results
            for (idx, result) in retry_results {
                match result {
                    Ok(translated_entry) => {
                        translated_entries.push((idx, translated_entry));
                    }
                    Err((e, original_entry)) => {
                        // Log the error and use original as last resort
                        {
                            let mut logs = log_capture.lock().await;
                            logs.push(LogEntry {
                                level: "ERROR".to_string(),
                                message: format!(
                                    "Failed to translate entry {} individually: {}. Using original text.",
                                    idx, e
                                ),
                            });
                        }
                        translated_entries.push((idx, original_entry));
                    }
                }
            }
        }

        // Sort by original index to maintain order
        translated_entries.sort_by_key(|(idx, _)| *idx);

        // Extract just the entries
        let final_entries: Vec<SubtitleEntry> = translated_entries.into_iter()
            .map(|(_, entry)| entry)
            .collect();

        Ok((final_entries, token_usage))
    }
    
    /// Translate a single subtitle entry
    async fn translate_single_entry(
        &self,
        entry: &SubtitleEntry,
        source_language: &str,
        target_language: &str,
        log_capture: Arc<Mutex<Vec<LogEntry>>>
    ) -> Result<SubtitleEntry> {
        // Skip empty entries
        if entry.text.trim().is_empty() {
            return Ok(entry.clone());
        }
        
        // Translate the entry text
        let (translated_text, _) = self.translate_text_with_usage(
            &entry.text,
            source_language,
            target_language,
            Some(log_capture)
        ).await?;
        
        // Apply format preservation if enabled
        let final_text = if self.options.preserve_formatting {
            FormatPreserver::preserve_formatting(&entry.text, &translated_text)
        } else {
            translated_text
        };
        
        // Create a new entry with the translated text
        let mut translated_entry = entry.clone();
        translated_entry.text = final_text;

        Ok(translated_entry)
    }
}

/// Adaptive batch sizer that calculates optimal batch size based on token limits
///
/// This struct estimates token counts from text length and adjusts batch sizes
/// to stay within provider token limits while maximizing throughput.
#[derive(Debug, Clone)]
pub struct AdaptiveBatchSizer {
    /// Average characters per token (varies by language, ~4 for English)
    chars_per_token: f32,
    /// Minimum batch size (never go below this)
    min_batch_size: usize,
    /// Maximum batch size (never exceed this)
    max_batch_size: usize,
    /// Safety margin for token estimation (0.8 = use 80% of limit)
    safety_margin: f32,
}

impl Default for AdaptiveBatchSizer {
    fn default() -> Self {
        Self {
            chars_per_token: 4.0,
            min_batch_size: 1,
            max_batch_size: 10,
            safety_margin: 0.8,
        }
    }
}

impl AdaptiveBatchSizer {
    /// Create a new adaptive batch sizer with custom settings
    pub fn new(chars_per_token: f32, min_batch_size: usize, max_batch_size: usize) -> Self {
        Self {
            chars_per_token,
            min_batch_size,
            max_batch_size,
            safety_margin: 0.8,
        }
    }

    /// Estimate token count for given text
    fn estimate_tokens(&self, text: &str) -> usize {
        (text.len() as f32 / self.chars_per_token).ceil() as usize
    }

    /// Calculate optimal batch size for given entries and token limit
    ///
    /// Returns the number of entries that can fit within the token limit.
    pub fn calculate_batch_size(&self, entries: &[SubtitleEntry], token_limit: usize) -> usize {
        if entries.is_empty() {
            return 0;
        }

        let effective_limit = (token_limit as f32 * self.safety_margin) as usize;
        let mut total_tokens = 0;
        let mut count = 0;

        for entry in entries {
            let entry_tokens = self.estimate_tokens(&entry.text);
            // Account for prompt overhead (~50 tokens per entry for translation instructions)
            let overhead = 50;

            if total_tokens + entry_tokens + overhead > effective_limit && count > 0 {
                break;
            }

            total_tokens += entry_tokens + overhead;
            count += 1;

            if count >= self.max_batch_size {
                break;
            }
        }

        count.max(self.min_batch_size).min(entries.len())
    }
}

#[cfg(test)]
mod batch_sizer_tests {
    use super::*;

    fn make_entry(text: &str, seq: usize) -> SubtitleEntry {
        SubtitleEntry {
            seq_num: seq,
            start_time_ms: 0,
            end_time_ms: 1000,
            text: text.to_string(),
        }
    }

    #[test]
    fn test_adaptive_batch_sizer_with_empty_entries_should_return_zero() {
        let sizer = AdaptiveBatchSizer::default();
        assert_eq!(sizer.calculate_batch_size(&[], 4000), 0);
    }

    #[test]
    fn test_adaptive_batch_sizer_with_small_entries_should_fit_multiple() {
        let sizer = AdaptiveBatchSizer::default();
        let entries: Vec<_> = (0..5)
            .map(|i| make_entry("Hello world", i))
            .collect();

        // 4000 token limit should easily fit 5 small entries
        let batch_size = sizer.calculate_batch_size(&entries, 4000);
        assert_eq!(batch_size, 5);
    }

    #[test]
    fn test_adaptive_batch_sizer_with_large_entries_should_limit_batch_size() {
        let sizer = AdaptiveBatchSizer::default();
        // Create entries with ~1000 chars each (~250 tokens)
        let long_text = "x".repeat(1000);
        let entries: Vec<_> = (0..10)
            .map(|i| make_entry(&long_text, i))
            .collect();

        // 500 token limit should only fit ~1-2 entries
        let batch_size = sizer.calculate_batch_size(&entries, 500);
        assert!(batch_size <= 2);
        assert!(batch_size >= 1);
    }

    #[test]
    fn test_adaptive_batch_sizer_should_respect_max_batch_size() {
        let sizer = AdaptiveBatchSizer::new(4.0, 1, 3); // max 3
        let entries: Vec<_> = (0..10)
            .map(|i| make_entry("Short", i))
            .collect();

        let batch_size = sizer.calculate_batch_size(&entries, 100000);
        assert_eq!(batch_size, 3);
    }

    #[test]
    fn test_adaptive_batch_sizer_should_respect_min_batch_size() {
        let sizer = AdaptiveBatchSizer::new(4.0, 2, 10); // min 2
        let long_text = "x".repeat(10000);
        let entries = vec![make_entry(&long_text, 0), make_entry(&long_text, 1)];

        // Even with tiny limit, should return at least min_batch_size
        let batch_size = sizer.calculate_batch_size(&entries, 100);
        assert_eq!(batch_size, 2);
    }
}

// Note: BatchTranslator requires a real TranslationService with provider connection.
// Meaningful unit tests for batch translation logic are in:
// - validation/markers.rs (marker extraction and validation)
// - translation/formatting.rs (format preservation)
// Integration tests in tests/ directory cover the full batch translation flow. 