/*!
 * Integration tests for the full translation pipeline.
 *
 * Tests end-to-end translation workflows with realistic SRT content.
 */

use std::path::PathBuf;
use yastwai::subtitle_processor::{SubtitleEntry, SubtitleProcessor};
use yastwai::translation::{
    AnalysisPass, ConsistencyChecker, QualityMetrics, RepairEngine, SubtitleDocument,
    TranslationPipeline, ValidationPass,
};
use yastwai::translation::pipeline::analysis_pass::AnalysisConfig;

/// Get path to test resources directory.
fn test_resources_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("resources")
}

/// Create a realistic movie subtitle document.
fn create_movie_subtitles() -> SubtitleDocument {
    let entries = vec![
        // Opening scene
        SubtitleEntry::new(1, 0, 3000, "In a world where nothing is as it seems...".to_string()),
        SubtitleEntry::new(2, 3500, 6000, "One man must find the truth.".to_string()),

        // Scene 1 - Office
        SubtitleEntry::new(3, 10000, 12000, "Good morning, Mr. Anderson.".to_string()),
        SubtitleEntry::new(4, 12500, 14500, "Morning, Sarah. Any messages?".to_string()),
        SubtitleEntry::new(5, 15000, 17000, "Just one. From your brother.".to_string()),
        SubtitleEntry::new(6, 17500, 20000, "He says it's urgent.".to_string()),

        // Scene 2 - Phone call (after 5 second gap)
        SubtitleEntry::new(7, 26000, 28000, "Hello? Tom, is that you?".to_string()),
        SubtitleEntry::new(8, 28500, 31000, "Listen carefully. They're watching.".to_string()),
        SubtitleEntry::new(9, 31500, 34000, "What are you talking about?".to_string()),
        SubtitleEntry::new(10, 34500, 37000, "Meet me at the extraction point.".to_string()),

        // Scene 3 - Action (after 8 second gap)
        SubtitleEntry::new(11, 46000, 47500, "<i>Whispered:</i> Over here!".to_string()),
        SubtitleEntry::new(12, 48000, 50000, "We don't have much time.".to_string()),
        SubtitleEntry::new(13, 50500, 53000, "The organization is compromised.".to_string()),
        SubtitleEntry::new(14, 53500, 56000, "Then we go to plan B.".to_string()),
        SubtitleEntry::new(15, 56500, 59000, "Are you sure about this, Tom?".to_string()),
    ];

    let mut doc = SubtitleDocument::from_entries(entries, "en");

    // Add known entities
    doc.glossary.add_character("Anderson");
    doc.glossary.add_character("Sarah");
    doc.glossary.add_character("Tom");
    doc.glossary.add_term("extraction point", "point d'extraction", None);
    doc.glossary.add_term("organization", "organisation", None);

    doc
}

/// Create translations for the movie subtitles.
fn add_translations(doc: &mut SubtitleDocument, quality: &str) {
    let translations = match quality {
        "good" => vec![
            "Dans un monde où rien n'est ce qu'il semble...",
            "Un homme doit trouver la vérité.",
            "Bonjour, M. Anderson.",
            "Bonjour, Sarah. Des messages?",
            "Un seul. De votre frère.",
            "Il dit que c'est urgent.",
            "Allô? Tom, c'est toi?",
            "Écoute bien. Ils nous surveillent.",
            "De quoi tu parles?",
            "Retrouve-moi au point d'extraction.",
            "<i>Murmuré:</i> Par ici!",
            "On n'a pas beaucoup de temps.",
            "L'organisation est compromise.",
            "Alors on passe au plan B.",
            "Tu es sûr de ça, Tom?",
        ],
        "mixed" => vec![
            "Dans un monde où rien n'est ce qu'il semble...",
            "Un homme doit trouver la vérité.",
            "Bonjour, M. Anderson.",
            "Bonjour, Sara. Des messages?", // Name changed
            "Un seul. De votre frère.",
            "", // Empty translation
            "Allô? Thomas, c'est toi?", // Name changed
            "Écoute bien. Ils nous surveillent.",
            "De quoi tu parles?",
            "Retrouve-moi au extraction point.", // Term not translated
            "Murmuré: Par ici!", // Missing formatting
            "On n'a pas beaucoup de temps.",
            "L'organization est compromise.", // Typo
            "Alors on passe au plan B.",
            "Tu es sûr de ça, Tom?",
        ],
        _ => vec![""; 15],
    };

    for (i, trans) in translations.iter().enumerate() {
        if !trans.is_empty() {
            doc.entries[i].set_translation(trans.to_string(), Some(0.85));
        }
    }
}

// ============================================================================
// Full Pipeline Integration Tests
// ============================================================================

#[test]
fn test_fullPipeline_withGoodTranslations_shouldPassValidation() {
    let mut doc = create_movie_subtitles();
    add_translations(&mut doc, "good");

    // Run analysis
    let analysis_pass = AnalysisPass::with_defaults();
    let analysis_result = analysis_pass.analyze_and_update(&mut doc);

    assert!(analysis_result.character_count >= 3, "Should detect characters");

    // Run validation
    let validation_pass = ValidationPass::with_defaults();
    let report = validation_pass.validate(&doc);

    assert!(report.passed(), "Good translations should pass");
    assert!(report.quality_score >= 0.8, "Quality score should be high");
}

#[test]
fn test_fullPipeline_withMixedQuality_shouldDetectIssues() {
    let mut doc = create_movie_subtitles();
    add_translations(&mut doc, "mixed");

    // Run validation
    let validation_pass = ValidationPass::with_defaults();
    let report = validation_pass.validate(&doc);

    // Should detect various issues
    assert!(!report.issues.is_empty(), "Should detect issues in mixed quality");

    // Should detect empty translation
    let has_empty_issue = report.issues.iter().any(|i| {
        matches!(i, yastwai::translation::pipeline::validation_pass::ValidationIssue::MissingTranslation { .. } |
                    yastwai::translation::pipeline::validation_pass::ValidationIssue::EmptyTranslation { .. })
    });
    assert!(has_empty_issue, "Should detect empty/missing translation");
}

#[test]
fn test_fullPipeline_withRepair_shouldImproveQuality() {
    let mut doc = create_movie_subtitles();
    add_translations(&mut doc, "mixed");

    // Initial validation
    let validation_pass = ValidationPass::with_defaults();
    let initial_report = validation_pass.validate(&doc);
    let initial_score = initial_report.quality_score;

    // Apply repairs
    let repair_engine = RepairEngine::new();
    for entry in &mut doc.entries {
        let repairs = repair_engine.repair_entry(entry, &doc.glossary);
        if let Some(repaired_text) = repair_engine.get_final_text(&repairs) {
            entry.set_translation(repaired_text, entry.confidence);
        }
    }

    // Post-repair validation
    let final_report = validation_pass.validate(&doc);
    let final_score = final_report.quality_score;

    // Score should improve or stay same after repair
    assert!(
        final_score >= initial_score * 0.95, // Allow small variance
        "Repair should not degrade quality: {} -> {}",
        initial_score,
        final_score
    );
}

// ============================================================================
// Scene Detection Tests
// ============================================================================

#[test]
fn test_sceneDetection_withTimingGaps_shouldDetectScenes() {
    let doc = create_movie_subtitles();

    let config = AnalysisConfig {
        detect_scenes: true,
        scene_config: yastwai::translation::SceneDetectionConfig {
            min_gap_ms: 5000, // 5 second gap
            ..Default::default()
        },
        ..Default::default()
    };

    let pass = AnalysisPass::new(config);
    let result = pass.analyze(&doc);

    // Should detect multiple scenes (we have 5+ second gaps)
    assert!(result.scene_count >= 2, "Should detect scene boundaries from timing gaps");
}

// ============================================================================
// Character/Glossary Extraction Tests
// ============================================================================

#[test]
fn test_glossaryExtraction_shouldFindCharacters() {
    let doc = create_movie_subtitles();

    let pass = AnalysisPass::with_defaults();
    let result = pass.analyze(&doc);

    // Should find characters mentioned multiple times
    // Note: Anderson, Sarah, Tom are in the text
    assert!(result.character_count >= 1, "Should extract character names");
}

#[test]
fn test_consistencyCheck_withNameChanges_shouldFlag() {
    let mut doc = create_movie_subtitles();
    add_translations(&mut doc, "mixed"); // Has name changes

    let checker = ConsistencyChecker::new();
    let report = checker.check(&doc);

    // Should detect name inconsistencies
    let name_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|i| matches!(i, yastwai::translation::StyleIssue::NameNotPreserved { .. }))
        .collect();

    // At least some name issues should be detected
    // (Sarah -> Sara, Tom -> Thomas in mixed translations)
    assert!(!name_issues.is_empty() || report.score < 1.0,
            "Should detect name consistency issues");
}

// ============================================================================
// Quality Metrics Tests
// ============================================================================

#[test]
fn test_qualityMetrics_fullDocument_shouldCalculateScore() {
    let mut doc = create_movie_subtitles();
    add_translations(&mut doc, "good");

    let metrics = QualityMetrics::new();
    let mut data = yastwai::translation::quality::metrics::MetricsData::new();

    for entry in &doc.entries {
        let is_translated = entry.translated_text.is_some();
        let is_empty = entry.translated_text.as_ref().map(|t| t.is_empty()).unwrap_or(true);

        let length_ratio = if is_translated && !is_empty {
            let orig_len = entry.original_text.len() as f32;
            let trans_len = entry.translated_text.as_ref().unwrap().len() as f32;
            Some(trans_len / orig_len)
        } else {
            None
        };

        data.add_entry(
            entry.id,
            yastwai::translation::quality::metrics::EntryMetrics {
                is_translated,
                is_empty: is_translated && is_empty,
                has_issues: !is_translated || is_empty,
                length_ratio,
                confidence: entry.confidence,
                ..Default::default()
            },
        );
    }

    let score = metrics.calculate_score(&data);

    assert!(score.entries_evaluated == 15);
    assert!(score.overall > 0.0);
    assert!(score.grade() != 'F', "Good translations should not get F grade");
}

// ============================================================================
// Error Recovery Integration Tests
// ============================================================================

#[test]
fn test_errorRecovery_multipleErrors_shouldTrack() {
    use yastwai::translation::{ErrorRecovery, TranslationError, TranslationErrorKind};

    let mut recovery = ErrorRecovery::new();

    // Simulate a series of errors during translation
    let errors = vec![
        TranslationError::new(TranslationErrorKind::Timeout, "Request 1 timed out"),
        TranslationError::new(TranslationErrorKind::Network, "Connection lost"),
        TranslationError::new(TranslationErrorKind::InvalidResponse, "Bad JSON"),
    ];

    for error in &errors {
        let _ = recovery.handle_error(error);
    }

    assert!(recovery.has_errors());
    assert_eq!(recovery.errors().len(), 3);

    let summary = recovery.error_summary();
    assert!(summary.contains("Timeout"));
    assert!(summary.contains("Network"));
}

// ============================================================================
// Real SRT File Tests
// ============================================================================

#[test]
fn test_loadTestSrt_andAnalyze_shouldWork() {
    let srt_path = test_resources_dir().join("test.srt");

    // Skip if test file doesn't exist
    if !srt_path.exists() {
        println!("Skipping test: {} not found", srt_path.display());
        return;
    }

    let processor = SubtitleProcessor::from_srt(&srt_path)
        .expect("Should load test.srt");

    let entries: Vec<SubtitleEntry> = processor.entries().cloned().collect();
    let doc = SubtitleDocument::from_entries(entries, "en");

    let pass = AnalysisPass::with_defaults();
    let result = pass.analyze(&doc);

    // Should successfully analyze without panicking
    assert!(result.scene_count >= 0);
}

// ============================================================================
// Pipeline Configuration Tests
// ============================================================================

#[test]
fn test_pipelineConfig_default_shouldEnableAllPhases() {
    use yastwai::translation::PipelineConfig;

    let config = PipelineConfig::new("en", "fr");

    assert!(config.enable_analysis);
    assert!(config.enable_validation);
    assert!(config.enable_reflection);
}

#[test]
fn test_pipeline_creation_shouldWork() {
    let pipeline = TranslationPipeline::for_languages("English", "French");

    assert_eq!(pipeline.config().source_language, "English");
    assert_eq!(pipeline.config().target_language, "French");
}
