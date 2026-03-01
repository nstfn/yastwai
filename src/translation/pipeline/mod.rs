/*!
 * Translation pipeline for multi-pass subtitle translation.
 *
 * The pipeline processes documents through three phases:
 * 1. **Analysis Pass**: Extract characters, terminology, detect scenes, summarize
 * 2. **Translation Pass**: Translate entries with rich context using JSON I/O
 * 3. **Validation Pass**: Check quality, consistency, and auto-repair issues
 */

pub mod adapter;
pub mod analysis_pass;
pub mod orchestrator;
pub mod translation_pass;
pub mod validation_pass;
pub mod reflection_pass;

// Re-export types used externally
pub use adapter::{PipelineAdapter, PipelineMode};
pub use orchestrator::{PipelineConfig, PipelineProgress, TranslationPipeline};
