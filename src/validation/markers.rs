/*!
 * Marker validation for batch translation responses.
 *
 * This module validates that the <<ENTRY_X>> markers used in batch
 * translations are correctly preserved in the AI response, enabling
 * proper splitting of translated text back into individual entries.
 */

use anyhow::{Result, anyhow};
use log::debug;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Regex for matching entry markers
static ENTRY_MARKER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<<ENTRY_(\d+)>>").expect("Invalid entry marker regex")
});

/// End marker constant
const END_MARKER: &str = "<<END>>";

/// Marker validation result
#[derive(Debug, Clone)]
pub struct MarkerValidationResult {
    /// Whether all markers are present
    pub all_present: bool,
    /// Expected marker indices
    pub expected_indices: Vec<usize>,
    /// Found marker indices
    pub found_indices: Vec<usize>,
    /// Missing marker indices
    pub missing_indices: Vec<usize>,
    /// Whether end marker is present
    pub end_marker_present: bool,
    /// Markers found out of order
    pub out_of_order: bool,
    /// Error message if validation failed
    pub error_message: Option<String>,
}

impl MarkerValidationResult {
    /// Check if validation passed (all markers present and in order)
    pub fn passed(&self) -> bool {
        self.all_present && self.end_marker_present && !self.out_of_order
    }

    /// Create a passing result
    pub fn success(expected_count: usize) -> Self {
        let indices: Vec<usize> = (0..expected_count).collect();
        Self {
            all_present: true,
            expected_indices: indices.clone(),
            found_indices: indices,
            missing_indices: vec![],
            end_marker_present: true,
            out_of_order: false,
            error_message: None,
        }
    }

    /// Create a failing result
    pub fn failure(
        expected_indices: Vec<usize>,
        found_indices: Vec<usize>,
        end_marker_present: bool,
        error_message: String,
    ) -> Self {
        let found_set: HashSet<usize> = found_indices.iter().copied().collect();
        let missing: Vec<usize> = expected_indices
            .iter()
            .filter(|i| !found_set.contains(i))
            .copied()
            .collect();

        Self {
            all_present: missing.is_empty(),
            expected_indices,
            found_indices,
            missing_indices: missing,
            end_marker_present,
            out_of_order: false,
            error_message: Some(error_message),
        }
    }
}

/// Marker validator for batch translation responses
pub struct MarkerValidator;

impl MarkerValidator {
    /// Validate that all expected markers are present in the response
    ///
    /// # Arguments
    /// * `response` - The translated response text
    /// * `expected_count` - The number of entries expected (0-indexed markers)
    ///
    /// # Returns
    /// * `MarkerValidationResult` with validation details
    pub fn validate(response: &str, expected_count: usize) -> MarkerValidationResult {
        if expected_count == 0 {
            return MarkerValidationResult::success(0);
        }

        let expected_indices: Vec<usize> = (0..expected_count).collect();

        // Find all markers in the response
        let found_indices: Vec<usize> = ENTRY_MARKER_REGEX
            .captures_iter(response)
            .filter_map(|cap| cap.get(1).and_then(|m| m.as_str().parse().ok()))
            .collect();

        // Check for end marker
        let end_marker_present = response.contains(END_MARKER);

        // Check for missing markers (O(1) lookups via HashSet)
        let found_set: HashSet<usize> = found_indices.iter().copied().collect();
        let missing_indices: Vec<usize> = expected_indices
            .iter()
            .filter(|i| !found_set.contains(i))
            .copied()
            .collect();

        // Check for correct order
        let mut sorted_found = found_indices.clone();
        sorted_found.sort_unstable();
        let out_of_order = found_indices != sorted_found;

        let all_present = missing_indices.is_empty();

        let error_message = if !all_present {
            Some(format!(
                "Missing markers: {:?}",
                missing_indices
            ))
        } else if !end_marker_present {
            Some("Missing <<END>> marker".to_string())
        } else if out_of_order {
            Some("Markers found out of order".to_string())
        } else {
            None
        };

        debug!(
            "Marker validation: expected={}, found={}, missing={}, end_marker={}",
            expected_count,
            found_indices.len(),
            missing_indices.len(),
            end_marker_present
        );

        MarkerValidationResult {
            all_present,
            expected_indices,
            found_indices,
            missing_indices,
            end_marker_present,
            out_of_order,
            error_message,
        }
    }

    /// Extract text between markers for a specific entry index
    ///
    /// # Arguments
    /// * `response` - The full response text
    /// * `entry_index` - The entry index to extract
    /// * `total_entries` - Total number of entries in the batch
    ///
    /// # Returns
    /// * The text between markers, or an error if markers not found
    pub fn extract_entry_text(
        response: &str,
        entry_index: usize,
        total_entries: usize,
    ) -> Result<String> {
        let start_marker = format!("<<ENTRY_{}>>", entry_index);
        let end_marker = if entry_index == total_entries - 1 {
            END_MARKER.to_string()
        } else {
            format!("<<ENTRY_{}>>", entry_index + 1)
        };

        // Find start position
        let start_pos = response
            .find(&start_marker)
            .ok_or_else(|| anyhow!("Start marker not found: {}", start_marker))?;

        let content_start = start_pos + start_marker.len();

        // Find end position
        let end_pos = response[content_start..]
            .find(&end_marker)
            .map(|pos| pos + content_start)
            .ok_or_else(|| anyhow!("End marker not found: {}", end_marker))?;

        // Extract and trim the content
        let content = response[content_start..end_pos].trim().to_string();

        Ok(content)
    }

    /// Attempt to recover entries from a partially corrupted response
    ///
    /// This tries to extract as many entries as possible even if some markers
    /// are missing or corrupted.
    ///
    /// # Arguments
    /// * `response` - The response text
    /// * `expected_count` - Number of expected entries
    ///
    /// # Returns
    /// * Vector of (index, text) tuples for successfully extracted entries
    pub fn recover_entries(response: &str, expected_count: usize) -> Vec<(usize, String)> {
        let mut recovered = Vec::new();

        for i in 0..expected_count {
            if let Ok(text) = Self::extract_entry_text(response, i, expected_count) {
                if !text.is_empty() {
                    recovered.push((i, text));
                }
            }
        }

        debug!(
            "Recovered {}/{} entries from response",
            recovered.len(),
            expected_count
        );

        recovered
    }

    /// Check if a response appears to be truncated
    ///
    /// Signs of truncation:
    /// - Missing end marker
    /// - Last entry marker has no content after it
    /// - Response ends mid-sentence
    pub fn is_truncated(response: &str, expected_count: usize) -> bool {
        // Missing end marker is a strong indicator
        if !response.contains(END_MARKER) {
            return true;
        }

        // Check if we have all expected markers
        let validation = Self::validate(response, expected_count);
        if !validation.all_present {
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_withAllMarkersPresent_shouldPass() {
        let response = r#"
<<ENTRY_0>>
Hello translated
<<ENTRY_1>>
World translated
<<ENTRY_2>>
Test translated
<<END>>"#;

        let result = MarkerValidator::validate(response, 3);

        assert!(result.passed());
        assert!(result.all_present);
        assert!(result.end_marker_present);
        assert!(result.missing_indices.is_empty());
    }

    #[test]
    fn test_validate_withMissingMarkers_shouldFail() {
        let response = r#"
<<ENTRY_0>>
Hello translated
<<ENTRY_2>>
Test translated
<<END>>"#;

        let result = MarkerValidator::validate(response, 3);

        assert!(!result.passed());
        assert!(!result.all_present);
        assert_eq!(result.missing_indices, vec![1]);
    }

    #[test]
    fn test_validate_withMissingEndMarker_shouldFail() {
        let response = r#"
<<ENTRY_0>>
Hello translated
<<ENTRY_1>>
World translated"#;

        let result = MarkerValidator::validate(response, 2);

        assert!(!result.passed());
        assert!(!result.end_marker_present);
    }

    #[test]
    fn test_extractEntryText_shouldExtractCorrectly() {
        let response = r#"<<ENTRY_0>>
First entry text
<<ENTRY_1>>
Second entry text
<<END>>"#;

        let text0 = MarkerValidator::extract_entry_text(response, 0, 2).unwrap();
        let text1 = MarkerValidator::extract_entry_text(response, 1, 2).unwrap();

        assert_eq!(text0, "First entry text");
        assert_eq!(text1, "Second entry text");
    }

    #[test]
    fn test_extractEntryText_withMissingMarker_shouldError() {
        let response = r#"<<ENTRY_0>>
First entry text
<<END>>"#;

        let result = MarkerValidator::extract_entry_text(response, 1, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_recoverEntries_shouldExtractAvailable() {
        // Note: extract_entry_text requires the *next* marker to be present
        // So ENTRY_0 can't be extracted without ENTRY_1
        // ENTRY_2 can be extracted because <<END>> is present
        let response = r#"<<ENTRY_0>>
First entry
<<ENTRY_1>>
Second entry
<<ENTRY_2>>
Third entry
<<END>>"#;

        let recovered = MarkerValidator::recover_entries(response, 3);

        assert_eq!(recovered.len(), 3);
        assert_eq!(recovered[0], (0, "First entry".to_string()));
        assert_eq!(recovered[1], (1, "Second entry".to_string()));
        assert_eq!(recovered[2], (2, "Third entry".to_string()));
    }

    #[test]
    fn test_recoverEntries_withMissingMiddle_shouldRecoverLast() {
        // When ENTRY_1 is missing, we can only extract ENTRY_2
        // because ENTRY_0 expects ENTRY_1 as end marker
        let response = r#"<<ENTRY_0>>
First entry
<<ENTRY_2>>
Third entry
<<END>>"#;

        let recovered = MarkerValidator::recover_entries(response, 3);

        // Only ENTRY_2 can be recovered (uses <<END>> as terminator)
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0], (2, "Third entry".to_string()));
    }

    #[test]
    fn test_isTruncated_withMissingEndMarker_shouldReturnTrue() {
        let response = r#"<<ENTRY_0>>
Hello"#;

        assert!(MarkerValidator::is_truncated(response, 1));
    }

    #[test]
    fn test_isTruncated_withCompleteResponse_shouldReturnFalse() {
        let response = r#"<<ENTRY_0>>
Hello
<<END>>"#;

        assert!(!MarkerValidator::is_truncated(response, 1));
    }

    #[test]
    fn test_validate_withOutOfOrderMarkers_shouldDetect() {
        let response = r#"<<ENTRY_1>>
Second
<<ENTRY_0>>
First
<<END>>"#;

        let result = MarkerValidator::validate(response, 2);

        assert!(!result.passed());
        assert!(result.out_of_order);
    }
}
