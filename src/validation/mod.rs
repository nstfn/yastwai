/*!
 * Validation module for translation quality assurance.
 *
 * This module provides comprehensive validation for translated subtitles:
 * - Marker validation (<<ENTRY_X>> markers in batch translations)
 * - Timecode validation (timing integrity)
 * - Format preservation validation (tags, styles)
 * - Length validation (reasonable translation length ratios)
 */

#![allow(unused_imports)]

pub mod markers;
pub mod timecodes;
pub mod formatting;
pub mod length;
pub mod service;

// Re-export main types
pub use markers::MarkerValidator;
pub use service::{ValidationConfig, ValidationService};
