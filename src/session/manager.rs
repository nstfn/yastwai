/*!
 * Session manager for translation session lifecycle.
 *
 * This module handles:
 * - Creating new translation sessions
 * - Resuming interrupted sessions
 * - Tracking progress and state changes
 * - Session cleanup and maintenance
 */

use anyhow::{Context, Result};
use log::{debug, info, warn};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use uuid::Uuid;

use crate::database::models::{
    SessionRecord, SessionStatus, SourceEntryRecord, TranslatedEntryRecord, TranslationStatus,
};
use crate::database::repository::Repository;
use crate::subtitle_processor::SubtitleEntry;

use super::models::{PendingEntry, ResumeResult, SessionCreateParams, SessionInfo, SessionStats};

/// Session manager for handling translation session lifecycle
pub struct SessionManager {
    /// Repository for database operations
    repo: Repository,
}

impl SessionManager {
    /// Create a new session manager with the given repository
    pub fn new(repo: Repository) -> Self {
        Self { repo }
    }

    /// Create a new session manager with the default database
    pub fn new_default() -> Result<Self> {
        let repo = Repository::new_default()?;
        Ok(Self::new(repo))
    }

    /// Create a new session manager with an in-memory database (for testing)
    pub fn new_in_memory() -> Result<Self> {
        let repo = Repository::new_in_memory()?;
        Ok(Self::new(repo))
    }

    /// Get the underlying repository
    pub fn repository(&self) -> &Repository {
        &self.repo
    }

    // =========================================================================
    // Session Creation
    // =========================================================================

    /// Create a new translation session
    pub async fn create_session(&self, params: SessionCreateParams) -> Result<SessionInfo> {
        // Generate session ID
        let session_id = Uuid::new_v4().to_string();

        // Compute file hash
        let file_hash = self.hash_file(&params.source_file_path).await?;

        info!(
            "Creating new session {} for {} ({} entries)",
            &session_id[..8],
            params.source_file_path.display(),
            params.entries.len()
        );

        // Create session record
        let session_record = SessionRecord::new(
            session_id.clone(),
            params.source_file_path.to_string_lossy().to_string(),
            file_hash,
            params.source_language,
            params.target_language,
            params.provider,
            params.model,
            params.entries.len() as i64,
        );

        // Save session to database
        self.repo.create_session(&session_record).await?;

        // Create source entry records
        let source_entries: Vec<SourceEntryRecord> = params
            .entries
            .iter()
            .map(|e| {
                SourceEntryRecord::new(
                    session_id.clone(),
                    e.seq_num as i64,
                    e.start_time_ms as i64,
                    e.end_time_ms as i64,
                    e.text.clone(),
                )
            })
            .collect();

        // Insert source entries
        self.repo.insert_source_entries(source_entries).await?;

        debug!("Session {} created successfully", &session_id[..8]);

        Ok(SessionInfo::from_record(&session_record))
    }

    /// Try to resume an existing session or create a new one
    pub async fn resume_or_create(
        &self,
        params: SessionCreateParams,
    ) -> Result<(SessionInfo, Vec<PendingEntry>)> {
        // Try to find a resumable session
        let resume_result = self.try_resume(&params).await?;

        match resume_result {
            ResumeResult::Resumed {
                session,
                pending_entries,
            } => {
                info!(
                    "Resuming session {} ({} pending entries)",
                    &session.id[..8],
                    pending_entries.len()
                );
                Ok((session, pending_entries))
            }
            ResumeResult::NotFound => {
                // Create new session
                let session = self.create_session(params.clone()).await?;

                // Get pending entries (all entries for a new session)
                let pending = self.get_pending_entries(&session.id).await?;

                Ok((session, pending))
            }
            ResumeResult::SourceFileChanged {
                session,
                old_hash,
                new_hash,
            } => {
                warn!(
                    "Source file changed since session {} was created (old: {}, new: {})",
                    &session.id[..8],
                    &old_hash[..8],
                    &new_hash[..8]
                );

                // Mark old session as failed
                self.repo
                    .update_session_status(&session.id, SessionStatus::Failed)
                    .await?;

                // Create new session
                let new_session = self.create_session(params.clone()).await?;
                let pending = self.get_pending_entries(&new_session.id).await?;

                Ok((new_session, pending))
            }
            ResumeResult::AlreadyCompleted { session } => {
                info!("Session {} already completed", &session.id[..8]);
                // Return completed session with empty pending list
                Ok((session, vec![]))
            }
            ResumeResult::Failed { session } => {
                warn!("Previous session {} failed, creating new session", &session.id[..8]);

                // Create new session
                let new_session = self.create_session(params.clone()).await?;
                let pending = self.get_pending_entries(&new_session.id).await?;

                Ok((new_session, pending))
            }
        }
    }

    /// Try to resume an existing session
    pub async fn try_resume(&self, params: &SessionCreateParams) -> Result<ResumeResult> {
        // Compute current file hash
        let current_hash = self.hash_file(&params.source_file_path).await?;

        // Look for a matching session
        let existing_session = self
            .repo
            .find_resumable_session(
                &current_hash,
                &params.source_language,
                &params.target_language,
                &params.provider,
                &params.model,
            )
            .await?;

        match existing_session {
            Some(session) => {
                // Verify file hash matches
                if session.source_file_hash != current_hash {
                    return Ok(ResumeResult::SourceFileChanged {
                        session: SessionInfo::from_record(&session),
                        old_hash: session.source_file_hash.clone(),
                        new_hash: current_hash,
                    });
                }

                // Check session status
                match session.status {
                    SessionStatus::Completed => {
                        Ok(ResumeResult::AlreadyCompleted {
                            session: SessionInfo::from_record(&session),
                        })
                    }
                    SessionStatus::Failed => {
                        Ok(ResumeResult::Failed {
                            session: SessionInfo::from_record(&session),
                        })
                    }
                    SessionStatus::InProgress | SessionStatus::Paused => {
                        // Get pending entries
                        let pending_records = self.repo.get_pending_entries(&session.id).await?;
                        let pending_entries: Vec<PendingEntry> = pending_records
                            .iter()
                            .map(PendingEntry::from_source_record)
                            .collect();

                        // Update session status to in_progress
                        self.repo
                            .update_session_status(&session.id, SessionStatus::InProgress)
                            .await?;

                        Ok(ResumeResult::Resumed {
                            session: SessionInfo::from_record(&session),
                            pending_entries,
                        })
                    }
                }
            }
            None => Ok(ResumeResult::NotFound),
        }
    }

    // =========================================================================
    // Session State Management
    // =========================================================================

    /// Get session by ID
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionInfo>> {
        let record = self.repo.get_session(session_id).await?;
        Ok(record.map(|r| SessionInfo::from_record(&r)))
    }

    /// Get pending entries for a session
    pub async fn get_pending_entries(&self, session_id: &str) -> Result<Vec<PendingEntry>> {
        let records = self.repo.get_pending_entries(session_id).await?;
        Ok(records.iter().map(PendingEntry::from_source_record).collect())
    }

    /// Record translated entries for a session
    pub async fn record_translations(
        &self,
        session_id: &str,
        translations: Vec<(i64, String)>, // (source_entry_id, translated_text)
    ) -> Result<()> {
        let entries: Vec<TranslatedEntryRecord> = translations
            .into_iter()
            .map(|(source_entry_id, translated_text)| {
                TranslatedEntryRecord::new(source_entry_id, translated_text)
            })
            .collect();

        self.repo.insert_translated_entries(entries).await?;

        // Update session progress
        let session = self.repo.get_session(session_id).await?;
        if let Some(_session) = session {
            let translated = self.repo.get_translated_entries(session_id).await?;
            let completed_count = translated
                .iter()
                .filter(|(_, t)| t.translation_status == TranslationStatus::Validated || 
                                 t.translation_status == TranslationStatus::Translated)
                .count() as i64;

            self.repo
                .update_session_progress(session_id, completed_count)
                .await?;
        }

        Ok(())
    }

    /// Mark session as paused (for graceful interruption)
    pub async fn pause_session(&self, session_id: &str) -> Result<()> {
        info!("Pausing session {}", &session_id[..8.min(session_id.len())]);
        self.repo
            .update_session_status(session_id, SessionStatus::Paused)
            .await
    }

    /// Mark session as completed
    pub async fn complete_session(&self, session_id: &str) -> Result<()> {
        info!("Completing session {}", &session_id[..8.min(session_id.len())]);
        self.repo
            .update_session_status(session_id, SessionStatus::Completed)
            .await
    }

    /// Mark session as failed
    pub async fn fail_session(&self, session_id: &str) -> Result<()> {
        warn!("Marking session {} as failed", &session_id[..8.min(session_id.len())]);
        self.repo
            .update_session_status(session_id, SessionStatus::Failed)
            .await
    }

    // =========================================================================
    // Session Listing and Cleanup
    // =========================================================================

    /// List all sessions
    pub async fn list_sessions(&self, status: Option<SessionStatus>) -> Result<Vec<SessionInfo>> {
        let records = self.repo.list_sessions(status).await?;
        Ok(records.iter().map(SessionInfo::from_record).collect())
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        info!("Deleting session {}", session_id);
        self.repo.delete_session(session_id).await
    }

    /// Clean up old sessions
    pub async fn cleanup_old_sessions(&self, days: i64) -> Result<i64> {
        info!("Cleaning up sessions older than {} days", days);
        self.repo.delete_old_sessions(days).await
    }

    /// Get session statistics
    pub async fn get_session_stats(&self, session_id: &str) -> Result<SessionStats> {
        let entries = self.repo.get_source_entries(session_id).await?;
        let translated = self.repo.get_translated_entries(session_id).await?;

        let mut stats = SessionStats::new();
        stats.total_entries = entries.len() as i64;
        stats.translated_entries = translated
            .iter()
            .filter(|(_, t)| t.translation_status != TranslationStatus::Pending)
            .count() as i64;
        stats.pending_entries = stats.total_entries - stats.translated_entries;
        stats.failed_entries = translated
            .iter()
            .filter(|(_, t)| t.translation_status == TranslationStatus::Failed)
            .count() as i64;
        stats.validated_entries = translated
            .iter()
            .filter(|(_, t)| t.translation_status == TranslationStatus::Validated)
            .count() as i64;

        Ok(stats)
    }

    /// Get all translated entries for a session (for building output)
    pub async fn get_translated_entries(
        &self,
        session_id: &str,
    ) -> Result<Vec<SubtitleEntry>> {
        let entries = self.repo.get_translated_entries(session_id).await?;

        let subtitles: Vec<SubtitleEntry> = entries
            .into_iter()
            .map(|(source, translated)| {
                SubtitleEntry::new(
                    source.seq_num.max(0) as usize,
                    source.start_time_ms.max(0) as u64,
                    source.end_time_ms.max(0) as u64,
                    translated.translated_text,
                )
            })
            .collect();

        Ok(subtitles)
    }

    // =========================================================================
    // Utility Methods
    // =========================================================================

    /// Compute SHA256 hash of a file
    async fn hash_file(&self, path: &Path) -> Result<String> {
        let path = path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            let mut file = std::fs::File::open(&path)
                .with_context(|| format!("Failed to open file for hashing: {:?}", path))?;

            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 8192];

            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }

            Ok(format!("{:x}", hasher.finalize()))
        })
        .await
        .context("File hashing task panicked")?
    }
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        Self {
            repo: self.repo.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    async fn create_test_manager() -> SessionManager {
        SessionManager::new_in_memory().expect("Failed to create test session manager")
    }

    fn create_test_entries() -> Vec<SubtitleEntry> {
        vec![
            SubtitleEntry::new(1, 0, 1000, "Hello".to_string()),
            SubtitleEntry::new(2, 1000, 2000, "World".to_string()),
            SubtitleEntry::new(3, 2000, 3000, "Test".to_string()),
        ]
    }

    fn create_temp_file_with_content(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write to temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    #[tokio::test]
    async fn test_createSession_shouldCreateWithCorrectData() {
        let manager = create_test_manager().await;
        let temp_file = create_temp_file_with_content("test content for hashing");

        let params = SessionCreateParams::new(
            temp_file.path().to_path_buf(),
            "en".to_string(),
            "fr".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            create_test_entries(),
        );

        let session = manager.create_session(params).await.expect("Failed to create session");

        assert_eq!(session.source_language, "en");
        assert_eq!(session.target_language, "fr");
        assert_eq!(session.total_entries, 3);
        assert_eq!(session.pending_entries, 3);
        assert!(session.is_resumable());
    }

    #[tokio::test]
    async fn test_getPendingEntries_shouldReturnAllForNewSession() {
        let manager = create_test_manager().await;
        let temp_file = create_temp_file_with_content("test content");

        let params = SessionCreateParams::new(
            temp_file.path().to_path_buf(),
            "en".to_string(),
            "fr".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            create_test_entries(),
        );

        let session = manager.create_session(params).await.unwrap();
        let pending = manager.get_pending_entries(&session.id).await.unwrap();

        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].source_text, "Hello");
        assert_eq!(pending[1].source_text, "World");
        assert_eq!(pending[2].source_text, "Test");
    }

    #[tokio::test]
    async fn test_resumeOrCreate_withNoExisting_shouldCreateNew() {
        let manager = create_test_manager().await;
        let temp_file = create_temp_file_with_content("unique content 1");

        let params = SessionCreateParams::new(
            temp_file.path().to_path_buf(),
            "en".to_string(),
            "de".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            create_test_entries(),
        );

        let (session, pending) = manager.resume_or_create(params).await.unwrap();

        assert_eq!(session.total_entries, 3);
        assert_eq!(pending.len(), 3);
    }

    #[tokio::test]
    async fn test_resumeOrCreate_withExisting_shouldResume() {
        let manager = create_test_manager().await;
        let temp_file = create_temp_file_with_content("resume test content");

        let params = SessionCreateParams::new(
            temp_file.path().to_path_buf(),
            "en".to_string(),
            "es".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            create_test_entries(),
        );

        // Create first session
        let (session1, _) = manager.resume_or_create(params.clone()).await.unwrap();

        // Pause and try to resume
        manager.pause_session(&session1.id).await.unwrap();

        let (session2, pending) = manager.resume_or_create(params).await.unwrap();

        // Should be the same session
        assert_eq!(session1.id, session2.id);
        assert_eq!(pending.len(), 3);
    }

    #[tokio::test]
    async fn test_completeSession_shouldMarkAsCompleted() {
        let manager = create_test_manager().await;
        let temp_file = create_temp_file_with_content("complete test");

        let params = SessionCreateParams::new(
            temp_file.path().to_path_buf(),
            "en".to_string(),
            "it".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            create_test_entries(),
        );

        let session = manager.create_session(params).await.unwrap();
        manager.complete_session(&session.id).await.unwrap();

        let updated = manager.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(updated.status, SessionStatus::Completed);
        assert!(!updated.is_resumable());
    }

    #[tokio::test]
    async fn test_listSessions_shouldReturnAllSessions() {
        let manager = create_test_manager().await;

        let temp_file1 = create_temp_file_with_content("list test 1");
        let temp_file2 = create_temp_file_with_content("list test 2");

        let params1 = SessionCreateParams::new(
            temp_file1.path().to_path_buf(),
            "en".to_string(),
            "fr".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            create_test_entries(),
        );

        let params2 = SessionCreateParams::new(
            temp_file2.path().to_path_buf(),
            "en".to_string(),
            "de".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            create_test_entries(),
        );

        manager.create_session(params1).await.unwrap();
        manager.create_session(params2).await.unwrap();

        let sessions = manager.list_sessions(None).await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_deleteSession_shouldRemoveSession() {
        let manager = create_test_manager().await;
        let temp_file = create_temp_file_with_content("delete test");

        let params = SessionCreateParams::new(
            temp_file.path().to_path_buf(),
            "en".to_string(),
            "pt".to_string(),
            "ollama".to_string(),
            "llama2".to_string(),
            create_test_entries(),
        );

        let session = manager.create_session(params).await.unwrap();
        manager.delete_session(&session.id).await.unwrap();

        let result = manager.get_session(&session.id).await.unwrap();
        assert!(result.is_none());
    }
}
