/*!
 * Pipeline adapter for legacy compatibility.
 *
 * This module bridges the new SubtitleDocument-based pipeline with
 * the existing SubtitleEntry-based translation flow, enabling gradual
 * migration while maintaining full backwards compatibility.
 */

use std::convert::Infallible;
use std::str::FromStr;

use anyhow::Result;

use crate::subtitle_processor::SubtitleEntry;
use crate::translation::core::{TokenUsageStats, TranslationService};
use crate::translation::document::SubtitleDocument;
use crate::translation::pipeline::{PipelineConfig, PipelineProgress, TranslationPipeline};
use crate::translation::subtitle_standards::SubtitleStandards;

/// Adapter for using the new pipeline with legacy SubtitleEntry-based code.
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

    /// Create a fast adapter (minimal analysis, no validation).
    pub fn fast(service: TranslationService, source_lang: &str, target_lang: &str) -> Self {
        let config = PipelineConfig::fast(source_lang, target_lang);
        Self::new(service, config)
    }

    /// Create a quality-focused adapter.
    pub fn quality(service: TranslationService, source_lang: &str, target_lang: &str) -> Self {
        let config = PipelineConfig::quality(source_lang, target_lang);
        Self::new(service, config)
    }

    /// Translate subtitle entries using the new pipeline.
    ///
    /// This method converts from the legacy `SubtitleEntry` format to the
    /// new `SubtitleDocument` format, runs the pipeline, and converts back.
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
        // Note: The new pipeline doesn't track tokens in the same way as the legacy system
        // Token tracking would need to be added to the pipeline for accurate reporting
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

    /// Parse a subtitle preset name into SubtitleStandards.
    pub fn parse_subtitle_preset(preset: &str) -> SubtitleStandards {
        match preset.to_lowercase().as_str() {
            "children" => SubtitleStandards::children(),
            "relaxed" => SubtitleStandards::relaxed(),
            _ => SubtitleStandards::netflix(),
        }
    }
}

/// Configuration for the pipeline adapter mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PipelineMode {
    /// Use the legacy batch translator (default for backwards compatibility)
    #[default]
    Legacy,
    /// Use the new pipeline with fast settings
    Fast,
    /// Use the new pipeline with default settings
    Standard,
    /// Use the new pipeline with quality settings
    Quality,
}

impl PipelineMode {
    /// Check if this mode uses the new pipeline.
    pub fn is_pipeline_enabled(&self) -> bool {
        !matches!(self, PipelineMode::Legacy)
    }
}

impl FromStr for PipelineMode {
    type Err = Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "fast" => PipelineMode::Fast,
            "standard" | "default" => PipelineMode::Standard,
            "quality" => PipelineMode::Quality,
            "legacy" | "off" | "disabled" => PipelineMode::Legacy,
            _ => PipelineMode::Legacy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipelineMode_fromStr_shouldParseModes() {
        assert_eq!("fast".parse::<PipelineMode>().unwrap(), PipelineMode::Fast);
        assert_eq!("standard".parse::<PipelineMode>().unwrap(), PipelineMode::Standard);
        assert_eq!("quality".parse::<PipelineMode>().unwrap(), PipelineMode::Quality);
        assert_eq!("legacy".parse::<PipelineMode>().unwrap(), PipelineMode::Legacy);
        assert_eq!("off".parse::<PipelineMode>().unwrap(), PipelineMode::Legacy);
        assert_eq!("unknown".parse::<PipelineMode>().unwrap(), PipelineMode::Legacy);
    }

    #[test]
    fn test_pipelineMode_isPipelineEnabled_shouldReturnCorrectly() {
        assert!(!PipelineMode::Legacy.is_pipeline_enabled());
        assert!(PipelineMode::Fast.is_pipeline_enabled());
        assert!(PipelineMode::Standard.is_pipeline_enabled());
        assert!(PipelineMode::Quality.is_pipeline_enabled());
    }

    #[test]
    fn test_parseSubtitlePreset_shouldReturnCorrectStandards() {
        let netflix = PipelineAdapter::parse_subtitle_preset("netflix");
        assert!((netflix.target_cps - 17.0).abs() < f32::EPSILON);

        let children = PipelineAdapter::parse_subtitle_preset("children");
        assert!((children.target_cps - 15.0).abs() < f32::EPSILON);

        let relaxed = PipelineAdapter::parse_subtitle_preset("relaxed");
        assert!((relaxed.target_cps - 20.0).abs() < f32::EPSILON);

        // Unknown preset defaults to netflix
        let unknown = PipelineAdapter::parse_subtitle_preset("unknown");
        assert!((unknown.target_cps - 17.0).abs() < f32::EPSILON);
    }
}
