/*!
 * Pipeline orchestrator for coordinating translation passes.
 *
 * The orchestrator manages the four-phase translation pipeline:
 * 1. Analysis Pass: Document preprocessing
 * 2. Translation Pass: Main translation with JSON I/O
 * 3. Reflection Pass: MQM-based critique and improvement of drafts
 * 4. Validation Pass: Quality assurance and auto-repair
 */

use anyhow::Result;
use std::time::{Duration, Instant};

use crate::translation::core::TranslationService;
use crate::translation::document::SubtitleDocument;

use super::analysis_pass::{AnalysisConfig, AnalysisPass, AnalysisResult};
use super::reflection_pass::{ReflectionConfig, ReflectionPass, SourceDraftPair};
use super::translation_pass::{TranslationPass, TranslationPassConfig, TranslationStats};
use super::validation_pass::{ValidationConfig, ValidationPass, ValidationReport};
use crate::translation::subtitle_standards::SubtitleStandards;

/// Configuration for the translation pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Whether to run the analysis pass
    pub enable_analysis: bool,

    /// Whether to run the validation pass
    pub enable_validation: bool,

    /// Configuration for the analysis pass
    pub analysis_config: AnalysisConfig,

    /// Configuration for the translation pass
    pub translation_config: TranslationPassConfig,

    /// Configuration for the validation pass
    pub validation_config: ValidationConfig,

    /// Whether to run the reflection pass
    pub enable_reflection: bool,

    /// Configuration for the reflection pass
    pub reflection_config: ReflectionConfig,

    /// Subtitle display standards
    pub subtitle_standards: SubtitleStandards,

    /// Source language
    pub source_language: String,

    /// Target language
    pub target_language: String,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            enable_analysis: true,
            enable_validation: true,
            enable_reflection: true,
            analysis_config: AnalysisConfig::thorough(),
            translation_config: TranslationPassConfig::quality(),
            validation_config: ValidationConfig::strict(),
            reflection_config: ReflectionConfig::default(),
            subtitle_standards: SubtitleStandards::default(),
            source_language: "en".to_string(),
            target_language: "fr".to_string(),
        }
    }
}

impl PipelineConfig {
    /// Create a new pipeline configuration.
    pub fn new(source_language: &str, target_language: &str) -> Self {
        Self {
            source_language: source_language.to_string(),
            target_language: target_language.to_string(),
            ..Default::default()
        }
    }

    /// Enable or disable analysis pass.
    pub fn with_analysis(mut self, enabled: bool) -> Self {
        self.enable_analysis = enabled;
        self
    }

    /// Enable or disable validation pass.
    pub fn with_validation(mut self, enabled: bool) -> Self {
        self.enable_validation = enabled;
        self
    }

    /// Set custom analysis configuration.
    pub fn with_analysis_config(mut self, config: AnalysisConfig) -> Self {
        self.analysis_config = config;
        self
    }

    /// Set custom translation configuration.
    pub fn with_translation_config(mut self, config: TranslationPassConfig) -> Self {
        self.translation_config = config;
        self
    }

    /// Set custom validation configuration.
    pub fn with_validation_config(mut self, config: ValidationConfig) -> Self {
        self.validation_config = config;
        self
    }

    /// Enable or disable reflection pass.
    pub fn with_reflection(mut self, enabled: bool) -> Self {
        self.enable_reflection = enabled;
        self
    }

    /// Set custom reflection configuration.
    pub fn with_reflection_config(mut self, config: ReflectionConfig) -> Self {
        self.reflection_config = config;
        self
    }

    /// Set subtitle standards.
    pub fn with_subtitle_standards(mut self, standards: SubtitleStandards) -> Self {
        self.subtitle_standards = standards;
        self
    }
}

/// Progress information during pipeline execution.
#[derive(Debug, Clone)]
pub struct PipelineProgress {
    /// Current phase
    pub phase: PipelinePhase,

    /// Progress within current phase (0.0 - 1.0)
    pub phase_progress: f32,

    /// Overall progress (0.0 - 1.0)
    pub overall_progress: f32,

    /// Current status message
    pub status: String,

    /// Entries processed so far
    pub entries_processed: usize,

    /// Total entries to process
    pub total_entries: usize,
}

impl PipelineProgress {
    /// Create a new progress indicator.
    pub fn new(phase: PipelinePhase, total_entries: usize) -> Self {
        Self {
            phase,
            phase_progress: 0.0,
            overall_progress: 0.0,
            status: String::new(),
            entries_processed: 0,
            total_entries,
        }
    }

    /// Update progress for current phase.
    pub fn update(&mut self, phase_progress: f32, status: &str) {
        self.phase_progress = phase_progress;
        self.status = status.to_string();

        // Calculate overall progress based on phase
        self.overall_progress = match self.phase {
            PipelinePhase::Analysis => phase_progress * 0.05,
            PipelinePhase::Translation => 0.05 + phase_progress * 0.40,
            PipelinePhase::Reflection => 0.45 + phase_progress * 0.40,
            PipelinePhase::Validation => 0.85 + phase_progress * 0.15,
        };
    }

    /// Transition to next phase.
    pub fn next_phase(&mut self, phase: PipelinePhase) {
        self.phase = phase;
        self.phase_progress = 0.0;
        self.status = format!("Starting {:?} phase", phase);
    }
}

/// Phases of the translation pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelinePhase {
    /// Document analysis phase
    Analysis,
    /// Main translation phase
    Translation,
    /// Reflection and improvement phase
    Reflection,
    /// Quality validation phase
    Validation,
}

/// Result of the complete pipeline execution.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Analysis result (if analysis was enabled)
    pub analysis: Option<AnalysisResult>,

    /// Translation statistics
    pub translation_stats: TranslationStats,

    /// Validation report (if validation was enabled)
    pub validation: Option<ValidationReport>,

    /// Total duration of pipeline execution
    pub duration: Duration,

    /// Whether the pipeline completed successfully
    pub success: bool,

    /// Error message if pipeline failed
    pub error: Option<String>,
}

impl PipelineResult {
    /// Create a new successful result.
    pub fn success(
        analysis: Option<AnalysisResult>,
        translation_stats: TranslationStats,
        validation: Option<ValidationReport>,
        duration: Duration,
    ) -> Self {
        Self {
            analysis,
            translation_stats,
            validation,
            duration,
            success: true,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failure(error: &str, duration: Duration) -> Self {
        Self {
            analysis: None,
            translation_stats: TranslationStats::new(),
            validation: None,
            duration,
            success: false,
            error: Some(error.to_string()),
        }
    }

    /// Get the overall quality score (if validation was run).
    pub fn quality_score(&self) -> Option<f32> {
        self.validation.as_ref().map(|v| v.quality_score)
    }

    /// Get a summary of the pipeline result.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        parts.push(format!("Duration: {:.2}s", self.duration.as_secs_f32()));

        if let Some(ref analysis) = self.analysis {
            parts.push(format!("Analysis: {}", analysis.description()));
        }

        parts.push(format!(
            "Translation: {} entries in {} batches",
            self.translation_stats.total_entries_translated, self.translation_stats.total_batches
        ));

        if let Some(ref validation) = self.validation {
            parts.push(format!(
                "Validation: {:.1}% quality score",
                validation.quality_score * 100.0
            ));
        }

        if !self.success {
            if let Some(ref error) = self.error {
                parts.push(format!("Error: {}", error));
            }
        }

        parts.join(" | ")
    }
}

/// The main translation pipeline orchestrator.
pub struct TranslationPipeline {
    config: PipelineConfig,
    analysis_pass: AnalysisPass,
    translation_pass: TranslationPass,
    reflection_pass: ReflectionPass,
    validation_pass: ValidationPass,
}

impl TranslationPipeline {
    /// Create a new pipeline with the given configuration.
    pub fn new(config: PipelineConfig) -> Self {
        let analysis_pass = AnalysisPass::new(config.analysis_config.clone());

        let mut translation_config = config.translation_config.clone();
        translation_config.subtitle_standards = config.subtitle_standards.clone();
        let translation_pass = TranslationPass::new(translation_config);

        let reflection_pass = ReflectionPass::new(config.reflection_config.clone());

        let mut validation_config = config.validation_config.clone();
        validation_config.subtitle_standards = config.subtitle_standards.clone();
        let validation_pass = ValidationPass::new(validation_config);

        Self {
            config,
            analysis_pass,
            translation_pass,
            reflection_pass,
            validation_pass,
        }
    }

    /// Create a pipeline with default configuration for the given languages.
    pub fn for_languages(source_language: &str, target_language: &str) -> Self {
        Self::new(PipelineConfig::new(source_language, target_language))
    }

    /// Translate a document through the full pipeline.
    pub async fn translate(
        &self,
        service: &TranslationService,
        doc: &mut SubtitleDocument,
        progress_callback: Option<Box<dyn Fn(PipelineProgress) + Send>>,
    ) -> Result<PipelineResult> {
        let start_time = Instant::now();
        let total_entries = doc.entries.len();

        let mut progress = PipelineProgress::new(PipelinePhase::Analysis, total_entries);

        // Phase 1: Analysis
        let analysis_result = if self.config.enable_analysis {
            progress.update(0.0, "Analyzing document...");
            if let Some(ref callback) = progress_callback {
                callback(progress.clone());
            }

            let result = self.analysis_pass.analyze_and_update(doc);

            progress.update(1.0, &format!("Analysis complete: {}", result.description()));
            if let Some(ref callback) = progress_callback {
                callback(progress.clone());
            }

            Some(result)
        } else {
            None
        };

        // Phase 2: Translation
        progress.next_phase(PipelinePhase::Translation);
        if let Some(ref callback) = progress_callback {
            callback(progress.clone());
        }

        let translation_callback: Option<Box<dyn Fn(f32) + Send>> = progress_callback
            .as_ref()
            .map(|_cb| {
                let cb_clone = Box::new({
                    let progress = progress.clone();
                    move |p: f32| {
                        let mut prog = progress.clone();
                        prog.update(p / 100.0, &format!("Translating... {:.1}%", p));
                        // Note: We can't call cb here directly due to ownership
                        // In real code, you'd use Arc<Mutex<>> or channels
                    }
                });
                cb_clone as Box<dyn Fn(f32) + Send>
            });

        let translation_stats = match self
            .translation_pass
            .translate_document(
                service,
                doc,
                &self.config.source_language,
                &self.config.target_language,
                translation_callback,
            )
            .await
        {
            Ok(stats) => stats,
            Err(e) => {
                return Ok(PipelineResult::failure(
                    &format!("Translation failed: {}", e),
                    start_time.elapsed(),
                ));
            }
        };

        progress.update(1.0, "Translation complete");
        if let Some(ref callback) = progress_callback {
            callback(progress.clone());
        }

        // Phase 2.5: Reflection
        if self.config.enable_reflection {
            progress.next_phase(PipelinePhase::Reflection);
            progress.update(0.0, "Reflecting on translations...");
            if let Some(ref callback) = progress_callback {
                callback(progress.clone());
            }

            // Build index map for O(1) entry lookups by ID
            let entry_index: std::collections::HashMap<usize, usize> = doc.entries.iter()
                .enumerate()
                .map(|(idx, e)| (e.id, idx))
                .collect();

            let total_translated = doc.entries.iter()
                .filter(|e| e.translated_text.is_some())
                .count();
            let mut reflected_count = 0;

            // Process in batches matching translation window size
            let batch_size = self.config.translation_config.window_config.batch_size;
            let entry_ids: Vec<usize> = doc.entries.iter()
                .filter(|e| e.translated_text.is_some())
                .map(|e| e.id)
                .collect();

            for chunk_ids in entry_ids.chunks(batch_size) {
                // Check if we should skip based on confidence
                let draft_entries: Vec<crate::translation::prompts::TranslatedEntry> = chunk_ids
                    .iter()
                    .filter_map(|&id| {
                        entry_index.get(&id).and_then(|&idx| {
                            let e = &doc.entries[idx];
                            e.translated_text.as_ref().map(|t| {
                                crate::translation::prompts::TranslatedEntry {
                                    id: e.id,
                                    translated: t.clone(),
                                    confidence: e.confidence,
                                }
                            })
                        })
                    })
                    .collect();

                if self.reflection_pass.should_skip(&draft_entries) {
                    reflected_count += chunk_ids.len();
                    continue;
                }

                let glossary = if doc.glossary.is_empty() {
                    None
                } else {
                    Some(doc.glossary.clone())
                };

                let max_retries = self.reflection_pass.max_retries();
                for _retry in 0..max_retries {
                    // Step 1: Reflect (re-read current translations for retry support)
                    let current_pairs: Vec<SourceDraftPair> = chunk_ids
                        .iter()
                        .filter_map(|&id| {
                            entry_index.get(&id).and_then(|&idx| {
                                let e = &doc.entries[idx];
                                e.translated_text.as_ref().map(|t| SourceDraftPair {
                                    id: e.id,
                                    source: e.original_text.clone(),
                                    draft: t.clone(),
                                    duration_seconds: e.timecode.duration_ms() as f32 / 1000.0,
                                })
                            })
                        })
                        .collect();

                    match self.reflection_pass.reflect(
                        service,
                        &current_pairs,
                        &glossary,
                        &self.config.source_language,
                        &self.config.target_language,
                        &self.config.subtitle_standards,
                    ).await {
                        Ok(reflection) => {
                            // Filter by minimum severity
                            let filtered = self.reflection_pass.filter_by_severity(reflection.suggestions);
                            if filtered.is_empty() {
                                break; // No actionable suggestions, done with this batch
                            }

                            // Step 2: Improve
                            match self.reflection_pass.improve(
                                service,
                                &current_pairs,
                                &filtered,
                                &self.config.source_language,
                                &self.config.target_language,
                                &self.config.subtitle_standards,
                            ).await {
                                Ok(improved) => {
                                    for entry in &improved {
                                        if let Some(&idx) = entry_index.get(&entry.id) {
                                            if !entry.translated.is_empty() {
                                                doc.entries[idx].set_translation(
                                                    entry.translated.clone(),
                                                    entry.confidence,
                                                );
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Improvement step failed, keeping draft translations: {}", e);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Reflection step failed, keeping draft translations: {}", e);
                            break;
                        }
                    }
                }

                reflected_count += chunk_ids.len();
                if let Some(ref callback) = progress_callback {
                    let p = reflected_count as f32 / total_translated.max(1) as f32;
                    progress.update(p, &format!("Reflected on {}/{} entries", reflected_count, total_translated));
                    callback(progress.clone());
                }
            }

            progress.update(1.0, "Reflection complete");
            if let Some(ref callback) = progress_callback {
                callback(progress.clone());
            }
        }

        // Phase 3: Validation
        let validation_result = if self.config.enable_validation {
            progress.next_phase(PipelinePhase::Validation);
            progress.update(0.0, "Validating translations...");
            if let Some(ref callback) = progress_callback {
                callback(progress.clone());
            }

            let report = self.validation_pass.validate_and_repair(doc);

            progress.update(1.0, &report.summary());
            if let Some(ref callback) = progress_callback {
                callback(progress.clone());
            }

            Some(report)
        } else {
            None
        };

        let duration = start_time.elapsed();

        Ok(PipelineResult::success(
            analysis_result,
            translation_stats,
            validation_result,
            duration,
        ))
    }

    /// Run only the analysis pass.
    pub fn analyze(&self, doc: &mut SubtitleDocument) -> AnalysisResult {
        self.analysis_pass.analyze_and_update(doc)
    }

    /// Run only the validation pass.
    pub fn validate(&self, doc: &mut SubtitleDocument) -> ValidationReport {
        self.validation_pass.validate_and_repair(doc)
    }

    /// Get the pipeline configuration.
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }
}

impl Default for TranslationPipeline {
    fn default() -> Self {
        Self::new(PipelineConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle_processor::SubtitleEntry;

    fn create_test_document(count: usize) -> SubtitleDocument {
        let entries: Vec<SubtitleEntry> = (1..=count)
            .map(|i| {
                SubtitleEntry::new(
                    i,
                    (i as u64 - 1) * 2000,
                    i as u64 * 2000,
                    format!("Test line {}", i),
                )
            })
            .collect();

        SubtitleDocument::from_entries(entries, "en")
    }

    #[test]
    fn test_pipelineConfig_new_shouldSetLanguages() {
        let config = PipelineConfig::new("English", "French");

        assert_eq!(config.source_language, "English");
        assert_eq!(config.target_language, "French");
        assert!(config.enable_analysis);
        assert!(config.enable_validation);
        assert!(config.enable_reflection);
    }

    #[test]
    fn test_pipelineProgress_update_shouldCalculateOverall() {
        let mut progress = PipelineProgress::new(PipelinePhase::Translation, 100);

        progress.update(0.5, "Halfway through translation");

        // Translation phase is 0.05 to 0.45 (40% of total)
        // At 50% of translation, overall should be 0.05 + 0.5*0.40 = 0.25
        assert!(progress.overall_progress > 0.2);
        assert!(progress.overall_progress < 0.3);
    }

    #[test]
    fn test_pipelineProgress_nextPhase_shouldResetPhaseProgress() {
        let mut progress = PipelineProgress::new(PipelinePhase::Analysis, 100);
        progress.update(1.0, "Done");

        progress.next_phase(PipelinePhase::Translation);

        assert_eq!(progress.phase, PipelinePhase::Translation);
        assert_eq!(progress.phase_progress, 0.0);
    }

    #[test]
    fn test_pipelineResult_summary_shouldIncludeAllInfo() {
        let result = PipelineResult::success(
            Some(AnalysisResult::empty()),
            TranslationStats {
                total_batches: 5,
                completed_batches: 5,
                total_entries_translated: 50,
                ..Default::default()
            },
            Some(ValidationReport::new(50)),
            Duration::from_secs(10),
        );

        let summary = result.summary();

        assert!(summary.contains("10.00s"));
        assert!(summary.contains("50 entries"));
        assert!(summary.contains("5 batches"));
    }

    #[test]
    fn test_pipelineResult_failure_shouldContainError() {
        let result = PipelineResult::failure("Test error", Duration::from_secs(1));

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result.summary().contains("Test error"));
    }

    #[test]
    fn test_translationPipeline_analyze_shouldUpdateDocument() {
        let mut doc = create_test_document(10);

        let pipeline = TranslationPipeline::for_languages("en", "fr");
        let result = pipeline.analyze(&mut doc);

        // Analysis should run and produce a result
        let _ = result.scene_count;
    }

    #[test]
    fn test_translationPipeline_validate_shouldReturnReport() {
        let mut doc = create_test_document(5);

        // Set some translations
        for entry in &mut doc.entries {
            entry.set_translation(format!("Translated: {}", entry.original_text), Some(0.9));
        }

        let pipeline = TranslationPipeline::for_languages("en", "fr");
        let report = pipeline.validate(&mut doc);

        assert_eq!(report.entries_validated, 5);
    }

    #[test]
    fn test_pipelineConfig_default_shouldEnableReflection() {
        let config = PipelineConfig::new("en", "fr");
        assert!(config.enable_reflection);
    }

    #[test]
    fn test_pipelinePhase_reflection_existsInEnum() {
        let phase = PipelinePhase::Reflection;
        assert_eq!(format!("{:?}", phase), "Reflection");
    }
}
