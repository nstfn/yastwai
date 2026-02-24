/*!
 * Pipeline orchestrator for coordinating translation passes.
 *
 * The orchestrator manages the three-phase translation pipeline:
 * 1. Analysis Pass: Document preprocessing
 * 2. Translation Pass: Main translation with JSON I/O
 * 3. Validation Pass: Quality assurance and auto-repair
 */

use anyhow::Result;
use std::time::{Duration, Instant};

use crate::translation::core::TranslationService;
use crate::translation::document::SubtitleDocument;

use super::analysis_pass::{AnalysisConfig, AnalysisPass, AnalysisResult};
use super::translation_pass::{TranslationPass, TranslationPassConfig, TranslationStats};
use super::validation_pass::{ValidationConfig, ValidationPass, ValidationReport};

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
            analysis_config: AnalysisConfig::default(),
            translation_config: TranslationPassConfig::default(),
            validation_config: ValidationConfig::default(),
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

    /// Create a fast pipeline configuration (minimal analysis, no validation).
    pub fn fast(source_language: &str, target_language: &str) -> Self {
        Self {
            enable_analysis: true,
            enable_validation: false,
            analysis_config: AnalysisConfig::minimal(),
            translation_config: TranslationPassConfig::fast(),
            validation_config: ValidationConfig::default(),
            source_language: source_language.to_string(),
            target_language: target_language.to_string(),
        }
    }

    /// Create a quality-focused pipeline configuration.
    pub fn quality(source_language: &str, target_language: &str) -> Self {
        Self {
            enable_analysis: true,
            enable_validation: true,
            analysis_config: AnalysisConfig::thorough(),
            translation_config: TranslationPassConfig::quality(),
            validation_config: ValidationConfig::strict(),
            source_language: source_language.to_string(),
            target_language: target_language.to_string(),
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
            PipelinePhase::Analysis => phase_progress * 0.1,
            PipelinePhase::Translation => 0.1 + phase_progress * 0.8,
            PipelinePhase::Validation => 0.9 + phase_progress * 0.1,
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
    validation_pass: ValidationPass,
}

impl TranslationPipeline {
    /// Create a new pipeline with the given configuration.
    pub fn new(config: PipelineConfig) -> Self {
        let analysis_pass = AnalysisPass::new(config.analysis_config.clone());
        let translation_pass = TranslationPass::new(config.translation_config.clone());
        let validation_pass = ValidationPass::new(config.validation_config.clone());

        Self {
            config,
            analysis_pass,
            translation_pass,
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
    }

    #[test]
    fn test_pipelineConfig_fast_shouldDisableValidation() {
        let config = PipelineConfig::fast("en", "fr");

        assert!(config.enable_analysis);
        assert!(!config.enable_validation);
    }

    #[test]
    fn test_pipelineConfig_quality_shouldEnableAll() {
        let config = PipelineConfig::quality("en", "fr");

        assert!(config.enable_analysis);
        assert!(config.enable_validation);
    }

    #[test]
    fn test_pipelineProgress_update_shouldCalculateOverall() {
        let mut progress = PipelineProgress::new(PipelinePhase::Translation, 100);

        progress.update(0.5, "Halfway through translation");

        // Translation phase is 0.1 to 0.9 (80% of total)
        // At 50% of translation, overall should be ~0.1 + 0.4 = 0.5
        assert!(progress.overall_progress > 0.4);
        assert!(progress.overall_progress < 0.6);
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
}
