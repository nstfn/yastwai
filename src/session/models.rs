/*!
 * Session-specific models and DTOs.
 *
 * These structures provide a higher-level abstraction over the raw
 * database records for session management operations.
 */

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::database::models::{SessionRecord, SessionStatus, SourceEntryRecord};
use crate::subtitle_processor::SubtitleEntry;

/// Parameters for creating a new session
#[derive(Debug, Clone)]
pub struct SessionCreateParams {
    /// Path to the source file
    pub source_file_path: PathBuf,
    /// Source language code
    pub source_language: String,
    /// Target language code
    pub target_language: String,
    /// Translation provider name
    pub provider: String,
    /// Model name
    pub model: String,
    /// Subtitle entries to translate
    pub entries: Vec<SubtitleEntry>,
}

impl SessionCreateParams {
    /// Create new session parameters
    pub fn new(
        source_file_path: PathBuf,
        source_language: String,
        target_language: String,
        provider: String,
        model: String,
        entries: Vec<SubtitleEntry>,
    ) -> Self {
        Self {
            source_file_path,
            source_language,
            target_language,
            provider,
            model,
            entries,
        }
    }
}

/// High-level session information for display and tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session ID
    pub id: String,
    /// Source file path
    pub source_file_path: String,
    /// Source language
    pub source_language: String,
    /// Target language
    pub target_language: String,
    /// Provider used
    pub provider: String,
    /// Model used
    pub model: String,
    /// Total entries to translate
    pub total_entries: i64,
    /// Completed entries
    pub completed_entries: i64,
    /// Pending entries count
    pub pending_entries: i64,
    /// Session status
    pub status: SessionStatus,
    /// Creation time
    pub created_at: String,
    /// Last update time
    pub updated_at: String,
    /// Completion time (if completed)
    pub completed_at: Option<String>,
}

impl SessionInfo {
    /// Create from a session record
    pub fn from_record(record: &SessionRecord) -> Self {
        Self {
            id: record.id.clone(),
            source_file_path: record.source_file_path.clone(),
            source_language: record.source_language.clone(),
            target_language: record.target_language.clone(),
            provider: record.provider.clone(),
            model: record.model.clone(),
            total_entries: record.total_entries,
            completed_entries: record.completed_entries,
            pending_entries: record.total_entries - record.completed_entries,
            status: record.status,
            created_at: record.created_at.clone(),
            updated_at: record.updated_at.clone(),
            completed_at: record.completed_at.clone(),
        }
    }

    /// Calculate completion percentage
    pub fn completion_percentage(&self) -> f64 {
        if self.total_entries == 0 {
            return 0.0;
        }
        (self.completed_entries as f64 / self.total_entries as f64) * 100.0
    }

    /// Check if session is resumable
    pub fn is_resumable(&self) -> bool {
        matches!(self.status, SessionStatus::InProgress | SessionStatus::Paused)
    }

    /// Get a human-readable status string
    pub fn status_display(&self) -> &'static str {
        match self.status {
            SessionStatus::InProgress => "In Progress",
            SessionStatus::Paused => "Paused",
            SessionStatus::Completed => "Completed",
            SessionStatus::Failed => "Failed",
        }
    }
}

impl std::fmt::Display for SessionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} -> {} ({:.1}% complete, {})",
            &self.id[..8],
            self.source_language,
            self.target_language,
            self.completion_percentage(),
            self.status_display()
        )
    }
}

/// Result of attempting to resume a session
#[derive(Debug)]
pub enum ResumeResult {
    /// Session was resumed successfully
    Resumed {
        /// The session info
        session: SessionInfo,
        /// Entries that still need translation
        pending_entries: Vec<PendingEntry>,
    },
    /// No matching session found, need to create new
    NotFound,
    /// Source file has changed since session was created
    SourceFileChanged {
        /// The existing session info
        session: SessionInfo,
        /// Old file hash
        old_hash: String,
        /// New file hash
        new_hash: String,
    },
    /// Session exists but is already completed
    AlreadyCompleted {
        /// The completed session info
        session: SessionInfo,
    },
    /// Session exists but failed
    Failed {
        /// The failed session info
        session: SessionInfo,
    },
}

impl ResumeResult {
    /// Check if this result allows proceeding with translation
    pub fn can_proceed(&self) -> bool {
        matches!(self, ResumeResult::Resumed { .. } | ResumeResult::NotFound)
    }

    /// Get the session info if available
    pub fn session_info(&self) -> Option<&SessionInfo> {
        match self {
            ResumeResult::Resumed { session, .. } => Some(session),
            ResumeResult::SourceFileChanged { session, .. } => Some(session),
            ResumeResult::AlreadyCompleted { session } => Some(session),
            ResumeResult::Failed { session } => Some(session),
            ResumeResult::NotFound => None,
        }
    }
}

/// A pending entry that needs translation
#[derive(Debug, Clone)]
pub struct PendingEntry {
    /// Database ID of the source entry
    pub source_entry_id: i64,
    /// Sequence number in the subtitle file
    pub seq_num: i64,
    /// Start time in milliseconds
    pub start_time_ms: i64,
    /// End time in milliseconds
    pub end_time_ms: i64,
    /// Source text to translate
    pub source_text: String,
    /// Number of previous translation attempts (0 for new)
    pub attempt_count: i64,
}

impl PendingEntry {
    /// Create from a source entry record
    pub fn from_source_record(record: &SourceEntryRecord) -> Self {
        Self {
            source_entry_id: record.id,
            seq_num: record.seq_num,
            start_time_ms: record.start_time_ms,
            end_time_ms: record.end_time_ms,
            source_text: record.source_text.clone(),
            attempt_count: 0,
        }
    }

    /// Convert to a SubtitleEntry for translation
    pub fn to_subtitle_entry(&self) -> SubtitleEntry {
        SubtitleEntry::new(
            self.seq_num.max(0) as usize,
            self.start_time_ms.max(0) as u64,
            self.end_time_ms.max(0) as u64,
            self.source_text.clone(),
        )
    }
}

/// Statistics about a translation session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    /// Total entries
    pub total_entries: i64,
    /// Entries successfully translated
    pub translated_entries: i64,
    /// Entries pending translation
    pub pending_entries: i64,
    /// Entries that failed and need retry
    pub failed_entries: i64,
    /// Entries that passed validation
    pub validated_entries: i64,
    /// Cache hit count during this session
    pub cache_hits: i64,
    /// Cache miss count during this session
    pub cache_misses: i64,
}

impl SessionStats {
    /// Create empty stats
    pub fn new() -> Self {
        Self {
            total_entries: 0,
            translated_entries: 0,
            pending_entries: 0,
            failed_entries: 0,
            validated_entries: 0,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    /// Calculate translation progress as percentage
    pub fn progress_percentage(&self) -> f64 {
        if self.total_entries == 0 {
            return 0.0;
        }
        (self.translated_entries as f64 / self.total_entries as f64) * 100.0
    }

    /// Calculate cache hit rate
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            return 0.0;
        }
        (self.cache_hits as f64 / total as f64) * 100.0
    }
}

impl Default for SessionStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::SessionRecord;

    fn create_test_session_record() -> SessionRecord {
        SessionRecord::new(
            "test-session-id".to_string(),
            "/path/to/video.mkv".to_string(),
            "hash123".to_string(),
            "en".to_string(),
            "fr".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            100,
        )
    }

    #[test]
    fn test_sessionInfo_fromRecord_shouldPopulateAllFields() {
        let record = create_test_session_record();
        let info = SessionInfo::from_record(&record);

        assert_eq!(info.id, "test-session-id");
        assert_eq!(info.source_language, "en");
        assert_eq!(info.target_language, "fr");
        assert_eq!(info.total_entries, 100);
        assert_eq!(info.pending_entries, 100);
    }

    #[test]
    fn test_sessionInfo_completionPercentage_shouldCalculateCorrectly() {
        let mut record = create_test_session_record();
        record.completed_entries = 50;

        let info = SessionInfo::from_record(&record);
        assert_eq!(info.completion_percentage(), 50.0);
    }

    #[test]
    fn test_sessionInfo_isResumable_shouldReturnCorrectly() {
        let mut record = create_test_session_record();

        let info = SessionInfo::from_record(&record);
        assert!(info.is_resumable());

        record.status = SessionStatus::Completed;
        let info = SessionInfo::from_record(&record);
        assert!(!info.is_resumable());
    }

    #[test]
    fn test_pendingEntry_toSubtitleEntry_shouldConvert() {
        let pending = PendingEntry {
            source_entry_id: 1,
            seq_num: 5,
            start_time_ms: 1000,
            end_time_ms: 2000,
            source_text: "Hello World".to_string(),
            attempt_count: 0,
        };

        let subtitle = pending.to_subtitle_entry();

        assert_eq!(subtitle.seq_num, 5);
        assert_eq!(subtitle.start_time_ms, 1000);
        assert_eq!(subtitle.end_time_ms, 2000);
        assert_eq!(subtitle.text, "Hello World");
    }

    #[test]
    fn test_sessionStats_progressPercentage_shouldCalculate() {
        let mut stats = SessionStats::new();
        stats.total_entries = 100;
        stats.translated_entries = 75;

        assert_eq!(stats.progress_percentage(), 75.0);
    }

    #[test]
    fn test_sessionStats_cacheHitRate_shouldCalculate() {
        let mut stats = SessionStats::new();
        stats.cache_hits = 30;
        stats.cache_misses = 70;

        assert_eq!(stats.cache_hit_rate(), 30.0);
    }

    #[test]
    fn test_resumeResult_canProceed_shouldReturnCorrectly() {
        assert!(ResumeResult::NotFound.can_proceed());
        assert!(!ResumeResult::AlreadyCompleted {
            session: SessionInfo::from_record(&create_test_session_record())
        }
        .can_proceed());
    }
}
