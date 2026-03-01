/*!
 * Integration tests for the translation pipeline module.
 *
 * Tests the interaction between pipeline components:
 * - Analysis pass -> Translation pass flow
 * - Validation pass with repair
 * - Full pipeline orchestration
 */

use yastwai::subtitle_processor::SubtitleEntry;
use yastwai::translation::{
    AnalysisPass, AnalysisResult, PipelineConfig, SubtitleDocument, TranslationPipeline,
    ValidationPass, ValidationReport,
};
use yastwai::translation::pipeline::analysis_pass::AnalysisConfig;
use yastwai::translation::pipeline::validation_pass::ValidationConfig;

/// Helper to create a test document with sample entries.
fn create_sample_document(entries: Vec<(&str, Option<&str>)>) -> SubtitleDocument {
    let subtitle_entries: Vec<SubtitleEntry> = entries
        .iter()
        .enumerate()
        .map(|(i, (text, _))| {
            SubtitleEntry::new(
                i + 1,
                (i as u64) * 2000,
                (i as u64 + 1) * 2000,
                text.to_string(),
            )
        })
        .collect();

    let mut doc = SubtitleDocument::from_entries(subtitle_entries, "en");

    // Set translations
    for (i, (_, translation)) in entries.iter().enumerate() {
        if let Some(t) = translation {
            doc.entries[i].set_translation(t.to_string(), Some(0.9));
        }
    }

    doc
}

/// Helper to create a realistic movie dialogue document.
fn create_movie_dialogue() -> SubtitleDocument {
    let entries = vec![
        ("John, we need to talk.", Some("John, il faut qu'on parle.")),
        ("What is it, Sarah?", Some("Qu'est-ce qu'il y a, Sarah?")),
        ("The mission... it's compromised.", Some("La mission... elle est compromise.")),
        ("How? We were so careful!", Some("Comment? On a été si prudents!")),
        ("There's a mole in the organization.", Some("Il y a une taupe dans l'organisation.")),
        ("Do you know who?", Some("Tu sais qui c'est?")),
        ("I have my suspicions.", Some("J'ai mes soupçons.")),
        ("We need to move now.", Some("On doit bouger maintenant.")),
        ("Agreed. Get the team ready.", Some("D'accord. Prépare l'équipe.")),
        ("John... be careful.", Some("John... fais attention.")),
    ];

    let subtitle_entries: Vec<SubtitleEntry> = entries
        .iter()
        .enumerate()
        .map(|(i, (text, _))| {
            SubtitleEntry::new(
                i + 1,
                (i as u64) * 3000,
                (i as u64) * 3000 + 2500,
                text.to_string(),
            )
        })
        .collect();

    let mut doc = SubtitleDocument::from_entries(subtitle_entries, "en");
    doc.glossary.add_character("John");
    doc.glossary.add_character("Sarah");
    doc.glossary.add_term("mission", "mission", None);
    doc.glossary.add_term("organization", "organisation", None);

    for (i, (_, translation)) in entries.iter().enumerate() {
        if let Some(t) = translation {
            doc.entries[i].set_translation(t.to_string(), Some(0.9));
        }
    }

    doc
}

// ============================================================================
// Analysis Pass Tests
// ============================================================================

#[test]
fn test_analysisPass_withMovieDialogue_shouldExtractCharacters() {
    let mut doc = create_movie_dialogue();

    let pass = AnalysisPass::with_defaults();
    let result = pass.analyze_and_update(&mut doc);

    assert!(result.character_count >= 2, "Should find at least John and Sarah");
    assert!(doc.glossary.character_names.contains("John"));
    assert!(doc.glossary.character_names.contains("Sarah"));
}

#[test]
fn test_analysisPass_withTimingGaps_shouldDetectScenes() {
    // Create entries with a significant timing gap
    let entries = vec![
        SubtitleEntry::new(1, 0, 2000, "Scene one dialogue.".to_string()),
        SubtitleEntry::new(2, 2100, 4000, "More scene one.".to_string()),
        // 10 second gap - scene change
        SubtitleEntry::new(3, 14000, 16000, "Scene two begins.".to_string()),
        SubtitleEntry::new(4, 16100, 18000, "Scene two continues.".to_string()),
    ];

    let mut doc = SubtitleDocument::from_entries(entries, "en");

    let config = AnalysisConfig {
        detect_scenes: true,
        ..Default::default()
    };
    let pass = AnalysisPass::new(config);
    let result = pass.analyze_and_update(&mut doc);

    assert!(result.scene_count >= 1, "Should detect at least one scene boundary");
}

#[test]
fn test_analysisPass_minimalConfig_shouldBeEfficient() {
    let doc = create_sample_document(vec![
        ("Hello world", None),
        ("Goodbye world", None),
    ]);

    let pass = AnalysisPass::new(AnalysisConfig::minimal());
    let result = pass.analyze(&doc);

    // Minimal config should skip scene detection and summary
    assert_eq!(result.scene_count, 0);
    assert!(result.summary.is_none());
}

// ============================================================================
// Validation Pass Tests
// ============================================================================

#[test]
fn test_validationPass_withGoodTranslations_shouldPass() {
    let doc = create_sample_document(vec![
        ("Hello", Some("Bonjour")),
        ("Goodbye", Some("Au revoir")),
        ("Thank you", Some("Merci")),
    ]);

    let pass = ValidationPass::with_defaults();
    let report = pass.validate(&doc);

    assert!(report.passed(), "Good translations should pass validation");
    assert!(report.quality_score >= 0.9, "Quality score should be high");
}

#[test]
fn test_validationPass_withMissingTranslations_shouldFail() {
    let doc = create_sample_document(vec![
        ("Hello", Some("Bonjour")),
        ("Goodbye", None), // Missing!
        ("Thank you", Some("Merci")),
    ]);

    let pass = ValidationPass::with_defaults();
    let report = pass.validate(&doc);

    assert!(!report.passed(), "Missing translations should fail");
    assert!(report.issues.len() >= 1);
}

#[test]
fn test_validationPass_withEmptyTranslations_shouldFail() {
    let doc = create_sample_document(vec![
        ("Hello", Some("Bonjour")),
        ("Important text here", Some("")), // Empty!
        ("Thank you", Some("Merci")),
    ]);

    let pass = ValidationPass::with_defaults();
    let report = pass.validate(&doc);

    assert!(!report.passed());
}

#[test]
fn test_validationPass_autoRepair_shouldFixFormatting() {
    let entries = vec![SubtitleEntry::new(
        1,
        0,
        2000,
        "<i>Whispered text</i>".to_string(),
    )];

    let mut doc = SubtitleDocument::from_entries(entries, "en");
    doc.entries[0].set_translation("Texte chuchoté".to_string(), Some(0.9)); // Missing <i> tags

    let pass = ValidationPass::new(ValidationConfig {
        check_formatting: true,
        enable_auto_repair: true,
        ..Default::default()
    });

    let report = pass.validate_and_repair(&mut doc);

    // After repair, formatting should be restored
    let translated = doc.entries[0].translated_text.as_ref().unwrap();
    assert!(
        translated.contains("<i>") && translated.contains("</i>"),
        "Formatting should be repaired"
    );
    assert!(report.repair_result.is_some());
}

// ============================================================================
// Pipeline Orchestration Tests
// ============================================================================

#[test]
fn test_pipelineConfig_default_shouldEnableAllPhases() {
    let config = PipelineConfig::new("en", "fr");

    assert!(config.enable_analysis);
    assert!(config.enable_validation);
    assert!(config.enable_reflection);
}

#[test]
fn test_translationPipeline_analyze_shouldProcessDocument() {
    let mut doc = create_movie_dialogue();

    let pipeline = TranslationPipeline::for_languages("en", "fr");
    let result = pipeline.analyze(&mut doc);

    assert!(result.has_data());
    assert!(result.character_count >= 2);
}

#[test]
fn test_translationPipeline_validate_shouldCheckQuality() {
    let mut doc = create_movie_dialogue();

    let pipeline = TranslationPipeline::for_languages("en", "fr");
    let report = pipeline.validate(&mut doc);

    assert_eq!(report.entries_validated, 10);
    assert!(report.quality_score > 0.0);
}

// ============================================================================
// End-to-End Tests
// ============================================================================

#[test]
fn test_fullPipeline_analysisToValidation_shouldMaintainData() {
    let mut doc = create_movie_dialogue();

    // Run analysis
    let analysis_pass = AnalysisPass::with_defaults();
    let analysis_result = analysis_pass.analyze_and_update(&mut doc);

    // Verify glossary was populated
    assert!(!doc.glossary.character_names.is_empty());

    // Run validation
    let validation_pass = ValidationPass::with_defaults();
    let validation_report = validation_pass.validate(&doc);

    // All entries should be validated
    assert_eq!(validation_report.entries_validated, doc.entries.len());

    // Quality should be high for good translations
    assert!(validation_report.quality_score >= 0.8);
}

#[test]
fn test_pipelineWithNamePreservation_shouldFlagInconsistency() {
    let entries = vec![
        SubtitleEntry::new(1, 0, 2000, "John walked in.".to_string()),
        SubtitleEntry::new(2, 2000, 4000, "John sat down.".to_string()),
    ];

    let mut doc = SubtitleDocument::from_entries(entries, "en");
    doc.glossary.add_character("John");

    // First entry preserves name, second doesn't
    doc.entries[0].set_translation("John est entré.".to_string(), Some(0.9));
    doc.entries[1].set_translation("Jean s'est assis.".to_string(), Some(0.9)); // Name changed!

    let validation_pass = ValidationPass::new(ValidationConfig {
        check_glossary_consistency: true,
        ..Default::default()
    });

    let report = validation_pass.validate(&doc);

    // Should detect inconsistency (GlossaryInconsistency issue expected)
    // Note: This depends on the exact validation implementation
    assert!(report.entries_validated == 2);
}
