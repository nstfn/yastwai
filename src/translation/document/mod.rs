/*!
 * Document modeling for subtitle translation.
 *
 * This module provides a rich document model for subtitles that enables:
 * - Structured JSON communication with LLMs
 * - Immutable timecode preservation
 * - Context tracking (scenes, speakers, glossary)
 * - Translation state management
 */

pub mod model;

// Re-export types used by other modules
pub use model::{
    DocumentEntry, FormattingTag, Glossary, Scene, SubtitleDocument, Timecode,
};

