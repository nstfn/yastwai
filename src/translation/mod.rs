/*!
 * Translation service for subtitle translation using AI providers.
 *
 * This module contains the core functionality for translating subtitles
 * using various AI providers.
 */

// Re-export main types
pub use self::core::TranslationService;
pub use self::pipeline::PipelineAdapter;

// Re-export document types
pub use self::document::{DocumentEntry, Glossary, SubtitleDocument};

// Re-export context types
pub use self::context::{ContextWindow, ContextWindowConfig, ContextWindowExt, SceneDetector};

// Re-export pipeline pass types
pub use self::pipeline::analysis_pass::{AnalysisPass, AnalysisConfig};
pub use self::pipeline::validation_pass::{ValidationPass, ValidationConfig as PipelineValidationConfig};

// Re-export quality types
pub use self::quality::consistency::{ConsistencyChecker, ConsistencyConfig, ConsistencyReport};
pub use self::quality::errors::{ErrorRecovery, RecoveryStrategy, TranslationError as TranslationPipelineError, TranslationErrorKind};
pub use self::quality::metrics::{EntryMetrics, MetricsData, QualityMetrics, QualityScore, QualityThresholds};
pub use self::quality::repair::{RepairConfig, RepairEngine};
// Public modules
pub mod batch;
pub mod cache;
pub mod core;
pub mod formatting;
pub mod pipeline;

// Modules accessible for benchmarks and tests
pub mod context;
pub mod document;
pub mod quality;

// Internal modules
pub(crate) mod concurrency;
pub(crate) mod prompts;
pub(crate) mod speculative;
pub(crate) mod subtitle_standards;
