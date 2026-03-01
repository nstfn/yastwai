/*!
 * Subtitle display standards for translation quality validation.
 *
 * Provides configurable constraints for reading speed (CPS),
 * line length (CPL), and subtitle structure.
 */

/// Subtitle display standards configuration.
#[derive(Debug, Clone)]
pub struct SubtitleStandards {
    /// Maximum characters per line (spaces included).
    pub max_chars_per_line: usize,
    /// Maximum number of lines per subtitle block.
    pub max_lines: usize,
    /// Target characters per second for comfortable reading.
    pub target_cps: f32,
    /// Minimum subtitle duration in seconds to apply CPS checks.
    pub min_duration_for_cps_check: f32,
}

impl Default for SubtitleStandards {
    fn default() -> Self {
        Self {
            max_chars_per_line: 42,
            max_lines: 2,
            target_cps: 17.0,
            min_duration_for_cps_check: 0.5,
        }
    }
}

impl SubtitleStandards {
    /// Calculate maximum characters allowed for a given duration.
    pub fn max_characters_for_duration(&self, duration_seconds: f32) -> usize {
        (self.target_cps * duration_seconds) as usize
    }

    /// Calculate CPS for a given text and duration.
    pub fn calculate_cps(&self, text: &str, duration_seconds: f32) -> f32 {
        if duration_seconds <= 0.0 {
            return 0.0;
        }
        text.chars().count() as f32 / duration_seconds
    }

    /// Whether CPS validation should apply for this duration.
    pub fn should_check_cps(&self, duration_seconds: f32) -> bool {
        duration_seconds >= self.min_duration_for_cps_check
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subtitle_standards_default_values() {
        let standards = SubtitleStandards::default();
        assert_eq!(standards.max_chars_per_line, 42);
        assert_eq!(standards.max_lines, 2);
        assert!((standards.target_cps - 17.0).abs() < f32::EPSILON);
        assert!((standards.min_duration_for_cps_check - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_max_characters_for_duration() {
        let standards = SubtitleStandards::default();
        assert_eq!(standards.max_characters_for_duration(2.0), 34);
    }

    #[test]
    fn test_calculate_cps() {
        let standards = SubtitleStandards::default();
        // 10 chars in 2 seconds = 5 CPS
        assert!((standards.calculate_cps("0123456789", 2.0) - 5.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_calculate_cps_zero_duration() {
        let standards = SubtitleStandards::default();
        assert!((standards.calculate_cps("test", 0.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn test_should_check_cps_skips_short_durations() {
        let standards = SubtitleStandards::default();
        assert!(!standards.should_check_cps(0.3));
        assert!(standards.should_check_cps(0.5));
        assert!(standards.should_check_cps(1.0));
    }
}
