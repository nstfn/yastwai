/*!
 * Translation pipeline for multi-pass subtitle translation.
 *
 * The pipeline processes documents through four phases:
 * 1. **Analysis Pass**: Extract characters, terminology, detect scenes, summarize
 * 2. **Translation Pass**: Translate entries with rich context using JSON I/O
 * 3. **Reflection Pass**: Critique and improve drafts using MQM-based review
 * 4. **Validation Pass**: Check quality, consistency, and auto-repair issues
 */

pub mod adapter;
pub mod analysis_pass;
pub mod orchestrator;
pub mod translation_pass;
pub mod validation_pass;
pub mod reflection_pass;

// Re-export types used externally
pub use adapter::PipelineAdapter;
pub use orchestrator::{PipelineConfig, PipelineProgress, TranslationPipeline};

use anyhow::{anyhow, Result};

/// Extract JSON from a potentially wrapped LLM response.
///
/// Handles: raw JSON, markdown code fences, and embedded JSON in prose.
pub(crate) fn extract_json(response: &str) -> Result<String> {
    let trimmed = response.trim();

    if trimmed.starts_with('{') {
        return Ok(trimmed.to_string());
    }

    if let Some(start) = trimmed.find("```json") {
        if let Some(end) = trimmed[start + 7..].find("```") {
            return Ok(trimmed[start + 7..start + 7 + end].trim().to_string());
        }
    }

    if let Some(start) = trimmed.find("```") {
        if let Some(end) = trimmed[start + 3..].find("```") {
            let json = trimmed[start + 3..start + 3 + end].trim();
            if json.starts_with('{') {
                return Ok(json.to_string());
            }
        }
    }

    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if end > start {
            return Ok(trimmed[start..=end].to_string());
        }
    }

    Err(anyhow!("Could not extract JSON from LLM response"))
}
