/*!
 * Benchmarks for translation pipeline operations.
 *
 * Measures performance of:
 * - Document creation and manipulation
 * - Context window operations
 * - Scene detection
 * - Glossary extraction
 * - Analysis pass
 * - Validation pass
 */

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use yastwai::subtitle_processor::SubtitleEntry;
use yastwai::translation::{
    AnalysisConfig, AnalysisPass, ContextWindow, ContextWindowConfig, Glossary, SceneDetector,
    SubtitleDocument,
};
use yastwai::translation::document::DocumentEntry;
use yastwai::translation::pipeline::validation_pass::{ValidationConfig, ValidationPass};

/// Generate test subtitle entries.
fn generate_entries(count: usize) -> Vec<SubtitleEntry> {
    let texts = [
        "Hello, how are you today?",
        "I'm doing well, thank you for asking.",
        "The weather is quite nice.",
        "Did you see the news this morning?",
        "No, I haven't had time to check.",
        "Something important happened at the meeting.",
        "Tell me more about it.",
        "Well, it's a long story...",
        "I have time to listen.",
        "Let me explain everything.",
    ];

    (0..count)
        .map(|i| {
            let text = texts[i % texts.len()];
            SubtitleEntry::new(
                i + 1,
                (i as u64) * 3000,
                (i as u64) * 3000 + 2500,
                text.to_string(),
            )
        })
        .collect()
}

/// Generate a document with translations.
fn generate_document(entry_count: usize, with_translations: bool) -> SubtitleDocument {
    let entries = generate_entries(entry_count);
    let mut doc = SubtitleDocument::from_entries(entries, "en");

    if with_translations {
        let translations = [
            "Bonjour, comment allez-vous aujourd'hui?",
            "Je vais bien, merci de demander.",
            "Le temps est assez agréable.",
            "Avez-vous vu les nouvelles ce matin?",
            "Non, je n'ai pas eu le temps de vérifier.",
            "Quelque chose d'important s'est passé à la réunion.",
            "Dites-m'en plus.",
            "Eh bien, c'est une longue histoire...",
            "J'ai le temps d'écouter.",
            "Laissez-moi tout vous expliquer.",
        ];

        for (i, entry) in doc.entries.iter_mut().enumerate() {
            let trans = translations[i % translations.len()];
            entry.set_translation(trans.to_string(), Some(0.9));
        }
    }

    doc
}

// ============================================================================
// Document Operations Benchmarks
// ============================================================================

fn bench_document_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("document_creation");

    for size in [10, 50, 100, 500, 1000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let entries = generate_entries(size);
            b.iter(|| {
                black_box(SubtitleDocument::from_entries(entries.clone(), "en"))
            });
        });
    }

    group.finish();
}

fn bench_document_with_glossary(c: &mut Criterion) {
    let mut group = c.benchmark_group("document_with_glossary");

    for term_count in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(term_count),
            term_count,
            |b, &term_count| {
                b.iter(|| {
                    let entries = generate_entries(100);
                    let mut doc = SubtitleDocument::from_entries(entries, "en");

                    for i in 0..term_count {
                        doc.glossary.add_term(
                            &format!("term_{}", i),
                            &format!("terme_{}", i),
                            Some("noun"),
                        );
                    }

                    black_box(doc)
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Context Window Benchmarks
// ============================================================================

fn bench_context_window_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_window");

    let doc = generate_document(500, true);

    for position in [10, 100, 250, 400].iter() {
        group.bench_with_input(
            BenchmarkId::new("build_at_position", position),
            position,
            |b, &position| {
                let config = ContextWindowConfig::default();
                b.iter(|| {
                    black_box(ContextWindow::new(&doc, position, &config, "en", "fr"))
                });
            },
        );
    }

    group.finish();
}

fn bench_context_window_json(c: &mut Criterion) {
    let doc = generate_document(200, true);
    let config = ContextWindowConfig::default();
    let window = ContextWindow::new(&doc, 100, &config, "en", "fr");

    c.bench_function("context_window_to_json", |b| {
        b.iter(|| {
            black_box(serde_json::to_string(&window).unwrap())
        });
    });
}

// ============================================================================
// Scene Detection Benchmarks
// ============================================================================

fn bench_scene_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("scene_detection");

    for size in [50, 100, 500, 1000].iter() {
        // Create entries with varying gaps to create scenes
        let entries: Vec<DocumentEntry> = (0..*size)
            .map(|i| {
                let gap = if i % 20 == 0 && i > 0 { 10000 } else { 500 };
                let start = if i == 0 { 0 } else { (i as u64) * 3000 + gap };
                DocumentEntry::from_subtitle_entry(
                    SubtitleEntry::new(i + 1, start, start + 2500, format!("Entry {}", i)),
                )
            })
            .collect();

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &entries, |b, entries| {
            let detector = SceneDetector::with_defaults();
            b.iter(|| {
                black_box(detector.detect_scenes(entries))
            });
        });
    }

    group.finish();
}

// ============================================================================
// Analysis Pass Benchmarks
// ============================================================================

fn bench_analysis_pass(c: &mut Criterion) {
    let mut group = c.benchmark_group("analysis_pass");

    for size in [50, 100, 200].iter() {
        let doc = generate_document(*size, false);

        group.bench_with_input(BenchmarkId::from_parameter(size), &doc, |b, doc| {
            let pass = AnalysisPass::with_defaults();
            b.iter(|| {
                black_box(pass.analyze(doc))
            });
        });
    }

    group.finish();
}

fn bench_analysis_pass_minimal(c: &mut Criterion) {
    let doc = generate_document(200, false);
    let pass = AnalysisPass::new(AnalysisConfig::minimal());

    c.bench_function("analysis_pass_minimal_200", |b| {
        b.iter(|| {
            black_box(pass.analyze(&doc))
        });
    });
}

// ============================================================================
// Validation Pass Benchmarks
// ============================================================================

fn bench_validation_pass(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation_pass");

    for size in [50, 100, 200].iter() {
        let doc = generate_document(*size, true);

        group.bench_with_input(BenchmarkId::from_parameter(size), &doc, |b, doc| {
            let pass = ValidationPass::with_defaults();
            b.iter(|| {
                black_box(pass.validate(doc))
            });
        });
    }

    group.finish();
}

fn bench_validation_with_repair(c: &mut Criterion) {
    // Create document with some issues to repair
    let entries: Vec<SubtitleEntry> = (0..100)
        .map(|i| {
            let text = if i % 5 == 0 {
                format!("<i>Entry {} with formatting</i>", i)
            } else {
                format!("Entry {}", i)
            };
            SubtitleEntry::new(i + 1, (i as u64) * 3000, (i as u64) * 3000 + 2500, text)
        })
        .collect();

    let mut doc = SubtitleDocument::from_entries(entries, "en");

    // Add translations with missing formatting
    for (i, entry) in doc.entries.iter_mut().enumerate() {
        let trans = if i % 5 == 0 {
            format!("Entrée {} avec formatage", i) // Missing <i> tags
        } else {
            format!("Entrée {}", i)
        };
        entry.set_translation(trans, Some(0.9));
    }

    let pass = ValidationPass::new(ValidationConfig {
        enable_auto_repair: true,
        check_formatting: true,
        ..Default::default()
    });

    c.bench_function("validation_with_repair_100", |b| {
        b.iter(|| {
            let mut doc_clone = doc.clone();
            black_box(pass.validate_and_repair(&mut doc_clone))
        });
    });
}

// ============================================================================
// Glossary Operations Benchmarks
// ============================================================================

fn bench_glossary_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("glossary_lookup");

    for term_count in [10, 50, 100, 500].iter() {
        let mut glossary = Glossary::default();
        for i in 0..*term_count {
            glossary.add_term(&format!("term_{}", i), &format!("terme_{}", i), None);
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(term_count),
            &glossary,
            |b, glossary| {
                b.iter(|| {
                    // Look up various terms
                    let _ = black_box(glossary.get_translation("term_0"));
                    let _ = black_box(glossary.get_translation("term_25"));
                    let _ = black_box(glossary.get_translation("nonexistent"));
                });
            },
        );
    }

    group.finish();
}

fn bench_glossary_character_check(c: &mut Criterion) {
    let mut glossary = Glossary::default();
    for name in ["John", "Sarah", "Michael", "Emma", "David", "Lisa", "Tom", "Jane"] {
        glossary.add_character(name);
    }

    c.bench_function("glossary_character_lookup", |b| {
        b.iter(|| {
            let _ = black_box(glossary.is_character_name("John"));
            let _ = black_box(glossary.is_character_name("Sarah"));
            let _ = black_box(glossary.is_character_name("Unknown"));
        });
    });
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(
    document_benches,
    bench_document_creation,
    bench_document_with_glossary,
);

criterion_group!(
    context_benches,
    bench_context_window_build,
    bench_context_window_json,
);

criterion_group!(
    detection_benches,
    bench_scene_detection,
);

criterion_group!(
    pipeline_benches,
    bench_analysis_pass,
    bench_analysis_pass_minimal,
    bench_validation_pass,
    bench_validation_with_repair,
);

criterion_group!(
    glossary_benches,
    bench_glossary_lookup,
    bench_glossary_character_check,
);

criterion_main!(
    document_benches,
    context_benches,
    detection_benches,
    pipeline_benches,
    glossary_benches,
);
