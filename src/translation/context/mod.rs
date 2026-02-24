/*!
 * Context management for subtitle translation.
 *
 * This module provides context-aware translation support:
 * - Sliding window context for maintaining narrative flow
 * - Glossary extraction for terminology consistency
 */

pub mod dynamic;
pub mod fuzzy;
pub mod glossary;
pub mod scenes;
pub mod speakers;
pub mod summary;
pub mod window;

// Re-export types used by pipeline and other modules
pub use glossary::{ConsistencyIssue, ExtractionConfig, GlossaryEnforcer, GlossaryExtractor, GlossaryPreflightChecker, PreflightReport};
pub use scenes::{SceneDetectionConfig, SceneDetector};
pub use summary::{HistorySummarizer, SummarizationConfig};
pub use window::{ContextWindow, ContextWindowConfig, ContextWindowExt};

