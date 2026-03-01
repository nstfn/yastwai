/*!
 * Pipeline adapter for legacy compatibility.
 *
 * This module bridges the SubtitleDocument-based pipeline with
 * the SubtitleEntry-based translation flow.
 */

use anyhow::Result;

use crate::subtitle_processor::SubtitleEntry;
use crate::translation::core::{TokenUsageStats, TranslationService};
use crate::translation::document::SubtitleDocument;
use crate::translation::pipeline::{PipelineConfig, PipelineProgress, TranslationPipeline};

/// Adapter for using the pipeline with legacy SubtitleEntry-based code.
pub struct PipelineAdapter {
    pipeline: TranslationPipeline,
    service: TranslationService,
}

impl PipelineAdapter {
    /// Create a new pipeline adapter.
    pub fn new(service: TranslationService, config: PipelineConfig) -> Self {
        Self {
            pipeline: TranslationPipeline::new(config),
            service,
        }
    }

    /// Create an adapter with default configuration.
    pub fn with_defaults(service: TranslationService, source_lang: &str, target_lang: &str) -> Self {
        let config = PipelineConfig::new(source_lang, target_lang);
        Self::new(service, config)
    }

    /// Translate subtitle entries using the pipeline.
    ///
    /// # Arguments
    /// * `entries` - Flat list of subtitle entries to translate
    /// * `source_language` - Source language code
    /// * `target_language` - Target language code
    /// * `progress_callback` - Optional callback for progress updates
    /// * `batch_callback` - Optional callback called after each batch completes
    ///
    /// # Returns
    /// Tuple of (translated entries, token usage stats)
    pub async fn translate_entries<P, B>(
        &self,
        entries: Vec<SubtitleEntry>,
        source_language: &str,
        target_language: &str,
        progress_callback: Option<P>,
        batch_callback: Option<B>,
    ) -> Result<(Vec<SubtitleEntry>, TokenUsageStats)>
    where
        P: Fn(usize, usize) + Clone + Send + 'static,
        B: Fn(Vec<SubtitleEntry>) + Clone + Send + Sync + 'static,
    {
        let total_entries = entries.len();

        // Convert to SubtitleDocument
        let mut doc = SubtitleDocument::from_entries(entries, source_language);
        doc = doc.with_target_language(target_language);

        // Create progress adapter
        let progress_adapter: Option<Box<dyn Fn(PipelineProgress) + Send>> =
            progress_callback.map(|cb| {
                let callback = cb.clone();
                let total = total_entries;
                Box::new(move |progress: PipelineProgress| {
                    let completed = (progress.overall_progress * total as f32) as usize;
                    callback(completed, total);
                }) as Box<dyn Fn(PipelineProgress) + Send>
            });

        // Clone batch callback for use after pipeline completes
        let batch_callback_clone = batch_callback.clone();

        // Run the pipeline
        let _result = self
            .pipeline
            .translate(&self.service, &mut doc, progress_adapter)
            .await?;

        // Convert back to SubtitleEntry list
        let translated_entries = doc.to_subtitle_entries();

        // Fire batch callback with all completed entries if provided
        if let Some(cb) = batch_callback_clone {
            cb(translated_entries.clone());
        }

        // Create token usage stats
        let token_usage = TokenUsageStats::default();

        Ok((translated_entries, token_usage))
    }

    /// Translate chunked subtitle entries (maintains compatibility with BatchTranslator interface).
    ///
    /// This is the main integration point for replacing BatchTranslator in app_controller.
    pub async fn translate_chunks<P, B>(
        &self,
        chunks: &[Vec<SubtitleEntry>],
        source_language: &str,
        target_language: &str,
        progress_callback: Option<P>,
        batch_callback: Option<B>,
    ) -> Result<(Vec<SubtitleEntry>, TokenUsageStats)>
    where
        P: Fn(usize, usize) + Clone + Send + 'static,
        B: Fn(Vec<SubtitleEntry>) + Clone + Send + Sync + 'static,
    {
        // Flatten chunks into a single list
        let entries: Vec<SubtitleEntry> = chunks.iter().flat_map(|c| c.iter().cloned()).collect();

        // Use the single-pass translation
        self.translate_entries(
            entries,
            source_language,
            target_language,
            progress_callback,
            batch_callback,
        )
        .await
    }

    /// Get the underlying pipeline for advanced usage.
    pub fn pipeline(&self) -> &TranslationPipeline {
        &self.pipeline
    }

    /// Get the pipeline result from the last translation.
    pub fn get_pipeline_config(&self) -> &PipelineConfig {
        self.pipeline.config()
    }
}
