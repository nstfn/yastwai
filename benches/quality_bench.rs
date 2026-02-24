/*!
 * Benchmarks for quality assurance operations.
 *
 * Measures performance of:
 * - Quality metrics calculation
 * - Consistency checking
 * - Repair engine operations
 * - Error recovery handling
 */

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use yastwai::subtitle_processor::SubtitleEntry;
use yastwai::translation::{
    ConsistencyChecker, ConsistencyConfig, EntryMetrics, ErrorRecovery, Glossary, MetricsData,
    QualityMetrics, QualityThresholds, RecoveryStrategy, RepairConfig, RepairEngine,
    SubtitleDocument, TranslationPipelineError as TranslationError, TranslationErrorKind,
};
use yastwai::translation::document::DocumentEntry;

/// Generate a document for benchmarking.
fn generate_document(count: usize, with_issues: bool) -> SubtitleDocument {
    let entries: Vec<SubtitleEntry> = (0..count)
        .map(|i| {
            let text = if with_issues && i % 4 == 0 {
                format!("<i>Entry {} with formatting</i>", i)
            } else {
                format!("Entry {} content here", i)
            };
            SubtitleEntry::new(i + 1, (i as u64) * 3000, (i as u64) * 3000 + 2500, text)
        })
        .collect();

    let mut doc = SubtitleDocument::from_entries(entries, "en");

    // Add translations
    for (i, entry) in doc.entries.iter_mut().enumerate() {
        let trans = if with_issues {
            match i % 5 {
                0 => format!("Entrée {} avec formatage", i), // Missing formatting
                1 => String::new(),                          // Empty translation
                2 => format!("Entrée {} contenu très très long qui dépasse la longueur normale attendue pour une traduction de sous-titres", i), // Too long
                _ => format!("Entrée {} contenu ici", i),    // Normal
            }
        } else {
            format!("Entrée {} contenu ici", i)
        };

        if !trans.is_empty() || !with_issues {
            entry.set_translation(trans, Some(0.9));
        }
    }

    // Add some glossary terms
    doc.glossary.add_character("John");
    doc.glossary.add_character("Sarah");
    doc.glossary.add_term("mission", "mission", None);
    doc.glossary.add_term("organization", "organisation", None);

    doc
}

/// Generate metrics data for benchmarking.
fn generate_metrics_data(count: usize, with_issues: bool) -> MetricsData {
    let mut data = MetricsData::new();
    data.total_entries = count;
    data.translated_entries = if with_issues { count - count / 10 } else { count };
    data.empty_entries = if with_issues { count / 20 } else { 0 };
    data.entries_with_issues = if with_issues { count / 5 } else { 0 };

    for i in 0..count {
        let is_issue = with_issues && i % 5 == 0;
        data.add_entry(
            i + 1,
            EntryMetrics {
                is_translated: !with_issues || i % 10 != 0,
                is_empty: with_issues && i % 20 == 0,
                has_issues: is_issue,
                length_ratio: Some(if is_issue { 2.5 } else { 1.1 }),
                chars_per_second: Some(15.0),
                line_lengths: vec![40],
                expected_tags: if i % 4 == 0 { 2 } else { 0 },
                missing_tags: if is_issue && i % 4 == 0 { 2 } else { 0 },
                confidence: Some(0.9),
            },
        );
    }

    data
}

// ============================================================================
// Quality Metrics Benchmarks
// ============================================================================

fn bench_metrics_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("metrics_calculation");

    for size in [50, 100, 500, 1000].iter() {
        let data = generate_metrics_data(*size, false);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            let metrics = QualityMetrics::new();
            b.iter(|| {
                black_box(metrics.calculate_score(data))
            });
        });
    }

    group.finish();
}

fn bench_metrics_with_issues(c: &mut Criterion) {
    let mut group = c.benchmark_group("metrics_with_issues");

    for size in [50, 100, 500].iter() {
        let data = generate_metrics_data(*size, true);

        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            let metrics = QualityMetrics::new();
            b.iter(|| {
                black_box(metrics.calculate_score(data))
            });
        });
    }

    group.finish();
}

fn bench_thresholds_comparison(c: &mut Criterion) {
    let data = generate_metrics_data(200, true);
    let metrics = QualityMetrics::new();
    let score = metrics.calculate_score(&data);

    c.bench_function("thresholds_strict_check", |b| {
        let thresholds = QualityThresholds::strict();
        b.iter(|| {
            black_box(score.meets_threshold(thresholds.min_overall))
        });
    });

    c.bench_function("thresholds_lenient_check", |b| {
        let thresholds = QualityThresholds::lenient();
        b.iter(|| {
            black_box(score.meets_threshold(thresholds.min_overall))
        });
    });
}

// ============================================================================
// Consistency Checker Benchmarks
// ============================================================================

fn bench_consistency_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("consistency_check");

    for size in [50, 100, 200].iter() {
        let doc = generate_document(*size, false);

        group.bench_with_input(BenchmarkId::from_parameter(size), &doc, |b, doc| {
            let checker = ConsistencyChecker::new();
            b.iter(|| {
                black_box(checker.check(doc))
            });
        });
    }

    group.finish();
}

fn bench_consistency_strict(c: &mut Criterion) {
    let doc = generate_document(100, true);
    let checker = ConsistencyChecker::with_config(ConsistencyConfig::strict());

    c.bench_function("consistency_strict_100", |b| {
        b.iter(|| {
            black_box(checker.check(&doc))
        });
    });
}

fn bench_consistency_lenient(c: &mut Criterion) {
    let doc = generate_document(100, true);
    let checker = ConsistencyChecker::with_config(ConsistencyConfig::lenient());

    c.bench_function("consistency_lenient_100", |b| {
        b.iter(|| {
            black_box(checker.check(&doc))
        });
    });
}

// ============================================================================
// Repair Engine Benchmarks
// ============================================================================

fn bench_repair_single_entry(c: &mut Criterion) {
    let entry = {
        let mut e = DocumentEntry::from_subtitle_entry(
            SubtitleEntry::new(1, 0, 2500, "<i>Important text here</i>".to_string()),
        );
        e.set_translation("Texte important ici".to_string(), Some(0.9)); // Missing formatting
        e
    };
    let glossary = Glossary::default();

    c.bench_function("repair_single_entry", |b| {
        let engine = RepairEngine::new();
        b.iter(|| {
            black_box(engine.repair_entry(&entry, &glossary))
        });
    });
}

fn bench_repair_with_glossary(c: &mut Criterion) {
    let entry = {
        let mut e = DocumentEntry::from_subtitle_entry(
            SubtitleEntry::new(1, 0, 2500, "Go to the extraction point.".to_string()),
        );
        e.set_translation("Allez au extraction point.".to_string(), Some(0.9)); // Term not translated
        e
    };

    let mut glossary = Glossary::default();
    glossary.add_term("extraction point", "point d'extraction", None);

    c.bench_function("repair_with_glossary", |b| {
        let engine = RepairEngine::new();
        b.iter(|| {
            black_box(engine.repair_entry(&entry, &glossary))
        });
    });
}

fn bench_repair_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("repair_batch");

    for size in [10, 50, 100].iter() {
        let doc = generate_document(*size, true);

        group.bench_with_input(BenchmarkId::from_parameter(size), &doc, |b, doc| {
            let engine = RepairEngine::new();
            b.iter(|| {
                for entry in &doc.entries {
                    black_box(engine.repair_entry(entry, &doc.glossary));
                }
            });
        });
    }

    group.finish();
}

fn bench_repair_aggressive(c: &mut Criterion) {
    let doc = generate_document(50, true);
    let engine = RepairEngine::with_config(RepairConfig::aggressive());

    c.bench_function("repair_aggressive_50", |b| {
        b.iter(|| {
            for entry in &doc.entries {
                black_box(engine.repair_entry(entry, &doc.glossary));
            }
        });
    });
}

fn bench_repair_conservative(c: &mut Criterion) {
    let doc = generate_document(50, true);
    let engine = RepairEngine::with_config(RepairConfig::conservative());

    c.bench_function("repair_conservative_50", |b| {
        b.iter(|| {
            for entry in &doc.entries {
                black_box(engine.repair_entry(entry, &doc.glossary));
            }
        });
    });
}

// ============================================================================
// Error Recovery Benchmarks
// ============================================================================

fn bench_error_handling(c: &mut Criterion) {
    let errors = [
        TranslationError::new(TranslationErrorKind::Network, "Connection failed"),
        TranslationError::new(TranslationErrorKind::Timeout, "Request timed out"),
        TranslationError::new(TranslationErrorKind::RateLimit, "Rate limit exceeded"),
        TranslationError::new(TranslationErrorKind::ParseError, "Invalid JSON response"),
        TranslationError::new(TranslationErrorKind::InvalidResponse, "Bad response format"),
    ];

    c.bench_function("error_recovery_single", |b| {
        b.iter(|| {
            let mut recovery = ErrorRecovery::new();
            black_box(recovery.handle_error(&errors[0]))
        });
    });

    c.bench_function("error_recovery_sequence", |b| {
        b.iter(|| {
            let mut recovery = ErrorRecovery::new();
            for error in &errors {
                black_box(recovery.handle_error(error));
            }
        });
    });
}

fn bench_error_recovery_with_entries(c: &mut Criterion) {
    let entry_ids: Vec<usize> = (1..=100).collect();
    let error =
        TranslationError::new(TranslationErrorKind::ParseError, "Invalid JSON").with_entries(entry_ids);

    c.bench_function("error_recovery_with_entries", |b| {
        b.iter(|| {
            let mut recovery = ErrorRecovery::new();
            black_box(recovery.handle_error(&error))
        });
    });
}

fn bench_error_summary(c: &mut Criterion) {
    let mut recovery = ErrorRecovery::new();

    // Add various errors
    for i in 0..20 {
        let kind = match i % 5 {
            0 => TranslationErrorKind::Network,
            1 => TranslationErrorKind::Timeout,
            2 => TranslationErrorKind::RateLimit,
            3 => TranslationErrorKind::ParseError,
            _ => TranslationErrorKind::InvalidResponse,
        };
        recovery.handle_error(&TranslationError::new(kind, &format!("Error {}", i)));
    }

    c.bench_function("error_summary_generation", |b| {
        b.iter(|| {
            black_box(recovery.error_summary())
        });
    });
}

fn bench_recovery_strategy_custom(c: &mut Criterion) {
    let strategy = RecoveryStrategy::aggressive();

    let error = TranslationError::new(TranslationErrorKind::Network, "Connection failed");

    c.bench_function("recovery_custom_strategy", |b| {
        b.iter(|| {
            let mut recovery = ErrorRecovery::with_strategy(strategy.clone());
            // Simulate retries
            for _ in 0..3 {
                black_box(recovery.handle_error(&error));
            }
        });
    });
}

// ============================================================================
// Combined Operations Benchmarks
// ============================================================================

fn bench_full_quality_check(c: &mut Criterion) {
    let doc = generate_document(100, true);
    let metrics = QualityMetrics::new();
    let checker = ConsistencyChecker::new();
    let repair = RepairEngine::new();

    c.bench_function("full_quality_pipeline_100", |b| {
        b.iter(|| {
            // Calculate metrics
            let mut data = MetricsData::new();
            data.total_entries = doc.entries.len();
            data.translated_entries = doc.entries.iter().filter(|e| e.translated_text.is_some()).count();

            for entry in &doc.entries {
                data.add_entry(
                    entry.id,
                    EntryMetrics {
                        is_translated: entry.translated_text.is_some(),
                        is_empty: entry
                            .translated_text
                            .as_ref()
                            .map(|t| t.is_empty())
                            .unwrap_or(false),
                        has_issues: false,
                        length_ratio: Some(1.0),
                        ..Default::default()
                    },
                );
            }

            let _score = black_box(metrics.calculate_score(&data));

            // Check consistency
            let _consistency = black_box(checker.check(&doc));

            // Attempt repairs
            for entry in &doc.entries {
                black_box(repair.repair_entry(entry, &doc.glossary));
            }
        });
    });
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(
    metrics_benches,
    bench_metrics_calculation,
    bench_metrics_with_issues,
    bench_thresholds_comparison,
);

criterion_group!(
    consistency_benches,
    bench_consistency_check,
    bench_consistency_strict,
    bench_consistency_lenient,
);

criterion_group!(
    repair_benches,
    bench_repair_single_entry,
    bench_repair_with_glossary,
    bench_repair_batch,
    bench_repair_aggressive,
    bench_repair_conservative,
);

criterion_group!(
    error_benches,
    bench_error_handling,
    bench_error_recovery_with_entries,
    bench_error_summary,
    bench_recovery_strategy_custom,
);

criterion_group!(combined_benches, bench_full_quality_check,);

criterion_main!(
    metrics_benches,
    consistency_benches,
    repair_benches,
    error_benches,
    combined_benches,
);
