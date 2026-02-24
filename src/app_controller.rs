use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Once;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::sync::Mutex;

use crate::app_config::Config;
use crate::file_utils::{self, FileManager, FileType};
use crate::language_utils;
use crate::session::{PendingEntry, SessionCreateParams, SessionInfo, SessionManager};
use crate::subtitle_processor::SubtitleCollection;
use crate::translation::core::LogEntry;
use crate::translation::{BatchTranslator, PipelineAdapter, PipelineMode, TranslationService};
use crate::subtitle_processor::SubtitleEntry;

// @module: Application controller for subtitle processing

/// Main application controller for subtitle translation
pub struct Controller {
    // @field: App configuration
    config: Config,
    // @field: Session manager for persistence (optional based on config)
    session_manager: Option<SessionManager>,
}

impl Controller {
    /// Create a new controller for test purposes with default configuration
    pub fn new_for_test() -> Result<Self> {
        Self::with_config(Config::default())
    }

    // @method: Create a new controller with the given configuration
    pub fn with_config(config: Config) -> Result<Self> {
        // Initialize session manager if persistence is enabled
        let session_manager = if config.session.enabled {
            match SessionManager::new_default() {
                Ok(sm) => {
                    debug!("Session manager initialized");
                    Some(sm)
                }
                Err(e) => {
                    warn!("Failed to initialize session manager: {}. Continuing without persistence.", e);
                    None
                }
            }
        } else {
            None
        };

        let controller = Self {
            config,
            session_manager,
        };

        Ok(controller)
    }

    /// Check if session persistence is available
    pub fn has_session_support(&self) -> bool {
        self.session_manager.is_some()
    }
    
    /// Check if the controller is properly initialized - used by tests
    pub fn is_initialized(&self) -> bool {
        !self.config.source_language.is_empty() && !self.config.target_language.is_empty()
    }
    
    /// Public method to write logs to a file - used by tests
    pub fn write_translation_logs(&self, logs: &[LogEntry], file_path: &str, translation_context: &str) -> Result<()> {
        self.write_logs_to_file(logs, file_path, translation_context)
    }

    /// Test version of run method - used by tests
    pub async fn test_run(&self, input_file: PathBuf, output_dir: PathBuf, force_overwrite: bool) -> Result<()> {
        // For testing purposes, just validate the configuration and simulate success
        info!("Test run initiated for file: {:?}", input_file);
        info!("Output directory: {:?}", output_dir);
        info!("Force overwrite: {}", force_overwrite);
        
        // Validate that we have a proper configuration
        if !self.is_initialized() {
            return Err(anyhow::anyhow!("Controller not properly initialized"));
        }
        
        // Simulate successful completion
        Ok(())
    }

    /// Test version of run_folder method - used by tests
    pub async fn test_run_folder(&self, input_dir: PathBuf, force_overwrite: bool) -> Result<()> {
        // For testing purposes, just validate the configuration and simulate success
        info!("Test run folder initiated for directory: {:?}", input_dir);
        info!("Force overwrite: {}", force_overwrite);
        
        // Validate that we have a proper configuration
        if !self.is_initialized() {
            return Err(anyhow::anyhow!("Controller not properly initialized"));
        }
        
        // Simulate successful completion
        Ok(())
    }

    /// Run the main workflow with input video file and output directory
    pub async fn run(&self, input_file: PathBuf, output_dir: PathBuf, force_overwrite: bool) -> Result<()> {
        let multi_progress = MultiProgress::new();
        self.run_with_progress(input_file, output_dir, &multi_progress, force_overwrite).await
    }
    
    /// Run the controller with progress reporting
    async fn run_with_progress(&self, input_file: PathBuf, output_dir: PathBuf, multi_progress: &MultiProgress, force_overwrite: bool) -> Result<()> {
        // Start timing the process
        let start_time = std::time::Instant::now();
        
        // Check if the input file exists
        if !input_file.exists() {
            return Err(anyhow::anyhow!("Input file does not exist: {:?}", input_file));
        }
        
        // Ensure the output directory exists
        file_utils::FileManager::ensure_dir(&output_dir)?;
        
        // Check if translation already exists
        let output_path = output_dir.join(self.get_subtitle_output_filename(&input_file, &self.config.target_language));
        if output_path.exists() && !force_overwrite {
            // Skip if translation already exists and no force flag
            warn!("Skipping file, translation already exists (use -f to force overwrite)");
            return Ok(());
        } else if output_path.exists() && force_overwrite {
            // Indicate that we'll overwrite
        }
        
        // Detect file type
        let file_type = FileManager::detect_file_type(&input_file).await?;
        
        // If it's a subtitle file, process it directly without extraction
        if file_type == FileType::Subtitle {
            info!("Detected subtitle file, skipping extraction process");
            
            // Parse the subtitle file directly
            let content = FileManager::read_to_string(&input_file)?;
            let source_file = input_file.clone();
            
            // Parse the SRT content to get subtitle entries
            let entries = SubtitleCollection::parse_srt_string(&content)
                .context("Failed to parse subtitle file")?;
            
            // Create a new SubtitleCollection
            // Note: We ignore the source language from config since we're processing the subtitle file directly
            let subtitles = SubtitleCollection {
                source_file,
                entries,
                source_language: "auto".to_string(), // Using "auto" to indicate we don't know the actual source language
            };
            
            // Translate the subtitles
            let (translated_subtitles, translation_duration) = self.translate_subtitles_with_progress(
                subtitles, 
                multi_progress, 
                &output_dir
            ).await?;
            
            // Save translated subtitles
            self.save_translated_subtitles(translated_subtitles, &input_file, &output_dir)?;
            
            info!(
                "Translation completed in {}.",
                Self::format_duration(translation_duration)
            );
            
            return Ok(());
        }
        
        // First check if the target language is already available as a subtitle track
        if !force_overwrite {
            if let Some(track_id) = self.find_target_language_track(&input_file).await? {
                
                // Extract the existing subtitle track
                if let Ok(subtitles) = self.extract_target_subtitles_to_memory(&input_file, track_id).await {
                    // If extraction was successful, save the existing subtitles
                    self.save_translated_subtitles(subtitles, &input_file, &output_dir)?;
                    return Ok(());
                }
            }
        } else if (self.find_target_language_track(&input_file).await?).is_some() {
            warn!("Skipping file, translation already exists (use -f to force overwrite)");
            return Ok(());
        }
        
        // Initialize translation testing once per run
        static INIT_TEST: Once = Once::new();
        INIT_TEST.call_once(|| {
            // Skip translation test for better performance, will fail later if there's an issue
            
            // Run test in a background task using tokio::spawn
            let config_clone = self.config.clone();
            let source_lang = self.config.source_language.clone();
            let target_lang = self.config.target_language.clone();
            tokio::spawn(async move {
                if let Ok(translation_service) = TranslationService::new(config_clone.translation) {
                    if let Err(e) = translation_service.test_connection(&source_lang, &target_lang, None).await {
                        warn!("Provider connection test failed: {}", e);
                    }
                }
            });
        });
        
        // Log the extraction step
        
        // Extract subtitles from the input file
        let subtitles = self.extract_subtitles_to_memory(&input_file).await?;
        
        // Log the subtitle count
        
        // Start the translation process
        
        // Translate the subtitles
        let (translated, translation_elapsed) = self.translate_subtitles_with_progress(subtitles, multi_progress, &output_dir).await?;
        
        // Save the translated subtitles
        self.save_translated_subtitles(translated, &input_file, &output_dir)?;
        
        // Calculate and display the elapsed time
        let elapsed = start_time.elapsed();
        
        // Calculate extraction time (subtract translation time from total time)
        let extraction_time = elapsed.checked_sub(translation_elapsed).unwrap_or_default();
        
        // Log completion time metrics
        info!(
            "Translation complete. Extraction: {} - Translation: {}", 
            Self::format_duration(extraction_time),
            Self::format_duration(translation_elapsed)
        );
        
        Ok(())
    }
    
    /// Extract subtitles from a video file to memory
    async fn extract_subtitles_to_memory(&self, input_file: &Path) -> Result<SubtitleCollection> {
        // First check if we can find the source language track
        let source_language = &self.config.source_language;
        
        // Try to automatically select the right subtitle track
        match SubtitleCollection::extract_with_auto_track_selection(
            input_file,
            source_language,
            None,
            source_language
        ).await {
            Ok(subtitles) => Ok(subtitles),
            Err(e) => {
                warn!("Auto-selection failed: {}", e);
                // If auto-selection failed, fall back to the extract_source_language_subtitle_to_memory method
                SubtitleCollection::extract_source_language_subtitle_to_memory(
                    input_file,
                    source_language
                ).await
            }
        }
    }
    
    /// Internal method to translate subtitles with a progress bar from the provided MultiProgress
    async fn translate_subtitles_with_progress(
        &self,
        subtitles: SubtitleCollection,
        multi_progress: &MultiProgress,
        output_dir: &Path,
    ) -> Result<(SubtitleCollection, std::time::Duration)> {
        // Start timing the translation process
        let translation_start_time = std::time::Instant::now();

        // Log the number of entries we're about to translate
        let total_entries_count = subtitles.entries.len();

        // Create or resume session if session manager is available
        let (session_info, pending_entries, seq_to_source_id) =
            self.setup_session(&subtitles).await;

        // Determine which entries need translation
        let (entries_to_translate, already_translated) = self
            .filter_entries_for_translation(&subtitles.entries, &pending_entries, &session_info)
            .await;

        // If we have already translated entries from a resumed session, collect them
        let mut translated_entries: Vec<SubtitleEntry> = already_translated;

        // If nothing to translate, we're done (fully resumed session)
        if entries_to_translate.is_empty() {
            info!("All entries already translated from previous session");

            // Create result collection
            let mut translated_collection = SubtitleCollection::new(
                PathBuf::from("translated.srt"),
                self.config.target_language.clone(),
            );

            // Sort and renumber
            translated_entries.sort_by_key(|entry| entry.start_time_ms);
            for (i, entry) in translated_entries.iter_mut().enumerate() {
                entry.seq_num = i + 1;
            }
            translated_collection.entries = translated_entries;

            // Mark session complete
            if let Some(ref session) = session_info {
                if let Some(ref session_manager) = self.session_manager {
                    if let Err(e) = session_manager.complete_session(&session.id).await {
                        warn!("Failed to mark session as complete: {}", e);
                    }
                }
            }

            return Ok((translated_collection, translation_start_time.elapsed()));
        }

        // Get max characters per chunk from the config
        let max_chars_per_chunk = self.config.translation.get_max_chars_per_request();

        // Split the entries to translate into chunks
        let temp_collection = SubtitleCollection {
            source_file: subtitles.source_file.clone(),
            entries: entries_to_translate,
            source_language: subtitles.source_language.clone(),
        };
        let chunks = temp_collection.split_into_chunks(max_chars_per_chunk);

        // Log that we're starting translation with provider and model info
        info!(
            "🚀 YASTwAI: {} - {}",
            self.config.translation.provider.display_name(),
            self.config.translation.get_model()
        );

        // Calculate parallel config early so we can log it before the progress bar
        let pending_count = chunks.iter().map(|c| c.len()).sum::<usize>();
        let entries_per_request = self.config.translation.common.entries_per_request.max(1);
        let max_concurrent = self.config.translation.optimal_concurrent_requests().max(5);
        let total_work_items = pending_count.div_ceil(entries_per_request);
        let use_parallel = self.config.translation.common.parallel_mode;

        // Create a progress bar for translation tracking - use work items for accurate ETA
        let progress_total = if use_parallel { total_work_items as u64 } else { chunks.len() as u64 };
        let progress_bar = multi_progress.add(ProgressBar::new(progress_total));
        let progress_label = if use_parallel { "requests" } else { "chunks" };
        let template = format!(
            "{{spinner:.green}} [{{elapsed_precise}}] [{{bar:40.cyan/blue}}] {{pos}}/{{len}} {} ({{percent}}%) {{msg}} {{eta}}",
            progress_label
        );
        let template_result = ProgressStyle::default_bar()
            .template(&template)
            .or_else(|_| {
                ProgressStyle::default_bar()
                    .template("{spinner} [{elapsed_precise}] [{bar:40}] {pos}/{len} ({percent}%) {msg}")
            })
            .unwrap_or_else(|_| ProgressStyle::default_bar());
        progress_bar.set_style(template_result.progress_chars("█▓▒░"));

        // Log translation info including parallel mode before progress bar starts
        if pending_count < total_entries_count {
            info!(
                "Translating {} remaining entries ({} already done)…",
                pending_count,
                total_entries_count - pending_count
            );
        } else {
            info!("Translating, please wait…");
        }
        
        // Log parallel mode info before progress bar is active
        if use_parallel {
            info!(
                "⚡ Parallel mode: {} entries → {} requests ({} concurrent)",
                pending_count,
                total_work_items,
                max_concurrent
            );
        }
        
        progress_bar.set_message("Translating");

        // Create log capture for storing warnings during translation
        let log_capture = Arc::new(Mutex::new(Vec::new()));
        let log_capture_clone = Arc::clone(&log_capture);

        // Use the translation service to translate all chunks
        let translation_service = TranslationService::new(self.config.translation.clone())?
            .with_experimental_features(&self.config.experimental);

        // Clone the progress_bar for use in the callback
        let pb = progress_bar.clone();

        // Setup batch complete callback for incremental recording
        let session_manager_clone = self.session_manager.clone();
        let session_id_clone = session_info.as_ref().map(|s| s.id.clone());
        let seq_to_source_id_clone = seq_to_source_id.clone();

        let batch_complete_callback = move |entries: Vec<SubtitleEntry>| {
            // Record translations to database if we have a session
            if let (Some(sm), Some(session_id)) = (&session_manager_clone, &session_id_clone) {
                let translations: Vec<(i64, String)> = entries
                    .iter()
                    .filter_map(|e| {
                        seq_to_source_id_clone
                            .get(&(e.seq_num as i64))
                            .map(|id| (*id, e.text.clone()))
                    })
                    .collect();

                if !translations.is_empty() {
                    let sm = sm.clone();
                    let session_id = session_id.clone();
                    // Spawn a task to record translations (fire-and-forget)
                    tokio::spawn(async move {
                        if let Err(e) = sm.record_translations(&session_id, translations).await {
                            debug!("Failed to record translations: {}", e);
                        }
                    });
                }
            }
        };

        // Check pipeline mode configuration
        let pipeline_mode: PipelineMode = self.config.translation.common.pipeline_mode.parse().unwrap_or_default();

        // Translate using either new pipeline or legacy batch translator
        let (mut new_translated_entries, token_usage) = if pipeline_mode.is_pipeline_enabled() {
            // Use new multi-pass translation pipeline
            info!("Using new translation pipeline (mode: {:?})", pipeline_mode);

            let adapter = match pipeline_mode {
                PipelineMode::Fast => PipelineAdapter::fast(
                    translation_service,
                    &self.config.source_language,
                    &self.config.target_language,
                ),
                PipelineMode::Quality => PipelineAdapter::quality(
                    translation_service,
                    &self.config.source_language,
                    &self.config.target_language,
                ),
                _ => PipelineAdapter::with_defaults(
                    translation_service,
                    &self.config.source_language,
                    &self.config.target_language,
                ),
            };

            let progress_callback = {
                let pb = pb.clone();
                move |completed: usize, _total: usize| {
                    pb.set_position(completed as u64);
                }
            };

            adapter
                .translate_chunks(
                    &chunks,
                    &self.config.source_language,
                    &self.config.target_language,
                    Some(progress_callback),
                    Some(batch_complete_callback),
                )
                .await?
        } else {
            // Use legacy batch translator
            let context_entries_count = self.config.translation.common.context_entries_count;
            let parallel_config = crate::translation::batch::ParallelTranslationConfig {
                max_concurrent_requests: max_concurrent,
                entries_per_request,
                use_legacy_batch_mode: !use_parallel,
                context_entries_count,
            };

            let batch_translator =
                BatchTranslator::with_parallel_config(translation_service, parallel_config);

            batch_translator
                .translate_batches_with_callback(
                    &chunks,
                    &self.config.source_language,
                    &self.config.target_language,
                    log_capture_clone,
                    move |completed, _total| {
                        pb.set_position(completed as u64);
                    },
                    Some(batch_complete_callback),
                )
                .await?
        };

        // Combine already translated with newly translated
        translated_entries.append(&mut new_translated_entries);
        
        // Finish and clear the progress bar instead of just finishing it
        // This ensures only the folder progress bar remains visible when processing multiple files
        progress_bar.finish_and_clear();
        
        // Now that the progress bar is finished, print any captured logs
        let logs = {
            let logs_guard = log_capture.lock().await;
            logs_guard.clone()
        };
        
        // Display captured logs if we're in debug mode or there were errors
        let error_logs = logs.iter().filter(|log| log.level == "ERROR").count();
        let warning_logs = logs.iter().filter(|log| log.level == "WARN").count();
        
        if error_logs > 0 || warning_logs > 0 {
            info!("Translation completed with {} errors and {} warnings.", error_logs, warning_logs);
            
            // In debug mode, or if explicitly requested, show all logs
            if log::max_level() >= log::LevelFilter::Debug {
                for log in &logs {
                    match log.level.as_str() {
                        "ERROR" => error!("{}", log.message),
                        "WARN" => warn!("{}", log.message),
                        "INFO" => info!("{}", log.message),
                        "DEBUG" => debug!("{}", log.message),
                        _ => info!("{}", log.message),
                    }
                }
            }
            
            // Write logs to yastwai.issues.log file
            let log_file_path = output_dir.join("yastwai.issues.log").to_string_lossy().to_string();
            let context = format!("{} - {} ({})",
                self.config.translation.provider.display_name(), 
                self.config.translation.get_model(),
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
                
            if let Err(e) = self.write_logs_to_file(&logs, &log_file_path, &context) {
                warn!("Failed to write logs to file: {}", e);
            } else {
                info!("Logs written to {}", log_file_path);
            }
        }
        
        // Sort entries by start time to ensure correct order
        translated_entries.sort_by_key(|entry| entry.start_time_ms);
        
        // Log the number of entries after translation
        let translated_entries_count = translated_entries.len();
        if translated_entries_count != total_entries_count {
            error!("WARNING: Number of entries changed during translation! Before: {}, After: {}", 
                  total_entries_count, translated_entries_count);
        } else {
            info!("Successfully translated all {} subtitle entries", translated_entries_count);
        }
        
        // Renumber entries to ensure sequential order
        for (i, entry) in translated_entries.iter_mut().enumerate() {
            entry.seq_num = i + 1;
        }
        
        // Create a new subtitle collection with the translated entries
        let mut translated_collection = SubtitleCollection::new(
            PathBuf::from("translated.srt"),
            self.config.target_language.clone(),
        );
        translated_collection.entries = translated_entries;

        // Log translation metrics
        let translation_elapsed = translation_start_time.elapsed();

        // Mark session as complete if we have session support
        if let Some(ref session) = session_info {
            if let Some(ref session_manager) = self.session_manager {
                if let Err(e) = session_manager.complete_session(&session.id).await {
                    warn!("Failed to mark session as complete: {}", e);
                } else {
                    debug!("Session {} marked as complete", &session.id[..8]);
                }
            }
        }

        // Only log the token usage information at the end of the translation process
        if token_usage.total_tokens > 0 {
            info!("🔢 {}", token_usage.summary());
        }

        Ok((translated_collection, translation_elapsed))
    }
    
    /// Save the translated subtitles to files
    fn save_translated_subtitles(&self, subtitles: SubtitleCollection, input_file: &Path, output_dir: &Path) -> Result<PathBuf> {
        // Generate an appropriate output filename
        let output_filename = self.get_subtitle_output_filename(
            input_file, 
            &self.config.target_language
        );
        
        let output_path = output_dir.join(output_filename);
        
        // Save the subtitle collection to the output path
        subtitles.write_to_srt(&output_path)?;
        
        // Log that we saved the subtitle file
        info!("Success: {}", output_path.display());
                
        Ok(output_path)
    }
    
    // Format duration in a human-readable format (HH:MM:SS)
    fn format_duration(duration: std::time::Duration) -> String {
        let total_seconds = duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        
        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}.{:03}s", seconds, duration.subsec_millis())
        }
    }
    
    /// Run the workflow in folder mode, processing all video files in a directory
    /// Files that already have translated subtitles will be skipped
    pub async fn run_folder(&self, input_dir: PathBuf, force_overwrite: bool) -> Result<()> {
        // Start timing the process
        let start_time = std::time::Instant::now();
        
        // Check if the input directory exists
        if !input_dir.exists() {
            return Err(anyhow::anyhow!("Input directory does not exist: {:?}", input_dir));
        }
        
        // Find all video files in the directory (recursive)
        let mut video_files = Vec::new();
        for ext in &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm"] {
            let mut files = file_utils::FileManager::find_files(&input_dir, ext)?;
            video_files.append(&mut files);
        }
        
        // If no video files found, return error
        if video_files.is_empty() {
            return Err(anyhow::anyhow!("No video files found in directory: {:?}", input_dir));
        }
        
        // Create multi-progress instance for multiple file processing
        let multi_progress = MultiProgress::new();
        
        // Create a progress bar for folder processing
        let folder_pb = multi_progress.add(ProgressBar::new(video_files.len() as u64));
        let template_result = ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} files ({percent}%) {msg} {eta}")
            .or_else(|_| ProgressStyle::default_bar().template("{spinner} [{elapsed_precise}] [{bar:40}] {pos}/{len} ({percent}%) {msg}"))
            .unwrap_or_else(|_| ProgressStyle::default_bar());
        folder_pb.set_style(template_result.progress_chars("█▓▒░"));
        folder_pb.set_message("Processing files");
        
        // Track success and failure counts
        let mut success_count = 0;
        let mut error_count = 0;
        let mut skip_count = 0;
        
        // Process each video file
        for video_file in video_files.iter() {
            // Get the file name for display
            let file_name = video_file.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            
            // Update the folder progress bar to show current file
            folder_pb.set_message(format!("Processing: {}", file_name));
            
            // Get output directory (use input dir)
            let output_dir = match video_file.parent() {
                Some(parent) => parent.to_path_buf(),
                None => input_dir.clone(),
            };
            
            // Check if translation already exists
            let output_path = output_dir.join(self.get_subtitle_output_filename(video_file, &self.config.target_language));
            if output_path.exists() && !force_overwrite {
                // Skip if translation already exists and no force flag
                warn!("Skipping file, translation already exists (use -f to force overwrite)");
                skip_count += 1;
                folder_pb.inc(1);
                continue;
            } else if output_path.exists() && force_overwrite {
                // Indicate that we'll overwrite
            }
            
            // Run the translation for this file
            match self.run_with_progress(video_file.clone(), output_dir, &multi_progress, force_overwrite).await {
                Ok(_) => {
                    success_count += 1;
                },
                Err(e) => {
                    error!("Error processing file {}: {}", file_name, e);
                    error_count += 1;
                }
            }
            
            // Update the folder progress bar
            folder_pb.inc(1);
        }
        
        // Finish the folder progress bar
        folder_pb.finish_with_message("Folder processing complete");
        
        // Calculate and display the total elapsed time
        let duration = start_time.elapsed();
        
        // Give summary results - important for batch operations
        let summary_message = format!("Folder processing completed: {} processed, {} skipped, {} errors", 
             success_count, skip_count, error_count);
        info!("{}", summary_message);
        
        // Write summary to log file
        let log_file_path = input_dir.join("yastwai.issues.log").to_string_lossy().to_string();
        let context = format!("Folder Processing: {} ({})",
            input_dir.display(),
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
            
        let folder_log_entry = LogEntry {
            level: "INFO".to_string(),
            message: format!("{} - Duration: {}", summary_message, Self::format_duration(duration))
        };
        
        // Create a vector with just the summary log entry for folder processing
        let folder_logs = vec![folder_log_entry];
        
        if let Err(e) = self.write_logs_to_file(&folder_logs, &log_file_path, &context) {
            warn!("Failed to write folder logs to file: {}", e);
        } else {
            info!("Folder processing logs written to {}", log_file_path);
        }
        
        Ok(())
    }
    
    /// Get the expected subtitle output filename for a video file
    fn get_subtitle_output_filename(&self, input_file: &Path, target_language: &str) -> String {
        // Check if this is an SRT file and handle appropriately
        if input_file.extension().and_then(|ext| ext.to_str()) == Some("srt") {
            // For SRT files, we need to keep the full path and replace the language code
            let _input_str = input_file.to_string_lossy().to_string();
            
            // If this is a path with directories
            if let Some(filename) = input_file.file_name().map(|f| f.to_string_lossy()) {
                // Split the filename by dots
                let parts: Vec<&str> = filename.split('.').collect();
                
                if parts.len() >= 3 {
                    // Format with multiple dots: "video.source.en.srt"
                    // Replace the language code (second to last part) with target language
                    let mut new_parts = parts.clone();
                    new_parts[parts.len() - 2] = target_language;
                    let new_filename = new_parts.join(".");
                    
                    // Replace the old filename with the new one, keeping the path
                    if let Some(parent) = input_file.parent() {
                        return parent.join(new_filename).to_string_lossy().to_string();
                    }
                    return new_filename.to_string();
                } else if parts.len() == 2 {
                    // Simple case: "single.srt"
                    // Append the target language before the extension
                    let new_filename = format!("{}.{}.srt", parts[0], target_language);
                    
                    // Replace the old filename with the new one, keeping the path
                    if let Some(parent) = input_file.parent() {
                        return parent.join(new_filename).to_string_lossy().to_string();
                    }
                    return new_filename;
                }
            }
        } else {
            // For video files, just extract the filename (no path) and append the target language
            if let Some(_filename) = input_file.file_name() {
                if let Some(stem) = input_file.file_stem() {
                    return format!("{}.{}.srt", stem.to_string_lossy(), target_language);
                }
            }
        }
        
        // Fallback: use the file stem if available, or a default name
        if let Some(stem) = input_file.file_stem() {
            format!("{}.{}.srt", stem.to_string_lossy(), target_language)
        } else {
            format!("output.{}.srt", target_language)
        }
    }

    


    /// Find a subtitle track in the target language if one exists
    async fn find_target_language_track(&self, input_file: &Path) -> Result<Option<usize>> {
        let tracks = SubtitleCollection::list_subtitle_tracks(input_file).await?;
        
        if tracks.is_empty() {
            return Ok(None);
        }
        
        // Try to find a track in the target language
        for track in &tracks {
            if let Some(track_lang) = &track.language {
                if language_utils::language_codes_match(track_lang, &self.config.target_language) {
                    return Ok(Some(track.index));
                }
            }
            
            // Also check title for language mention
            if let Some(title) = &track.title {
                if let Ok(target_name) = language_utils::get_language_name(&self.config.target_language) {
                    let title_lower = title.to_lowercase();
                    let name_lower = target_name.to_lowercase();
                    
                    if title_lower.contains(&name_lower) {
                        return Ok(Some(track.index));
                    }
                }
            }
        }
        
        Ok(None)
    }
    
    /// Extract subtitles in target language from the video file directly to memory
    async fn extract_target_subtitles_to_memory(&self, input_file: &Path, track_id: usize) -> Result<SubtitleCollection> {
        // Extract the subtitle track
        let output_path = input_file.with_extension("extracted.srt");
        let subtitles = SubtitleCollection::extract_from_video(
            input_file, 
            track_id, 
            &self.config.target_language, 
            &output_path
        ).await?;
        
        // Delete the temporary file
        if output_path.exists() {
            if let Err(_e) = std::fs::remove_file(&output_path) {
                // Removed warn log about failing to remove temp file, it's not critical
            }
        }
        
        Ok(subtitles)
    }

    /// Write translation logs to a log file
    fn write_logs_to_file(
        &self,
        logs: &[LogEntry],
        file_path: &str,
        translation_context: &str,
    ) -> Result<()> {
        let mut log_content = String::new();

        // Add header
        log_content.push_str(&format!(
            "Translation Log - {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        ));
        log_content.push_str(&format!("Context: {}\n\n", translation_context));

        // Add each log entry
        for entry in logs {
            log_content.push_str(&format!("[{}] {}\n", entry.level, entry.message));
        }

        // Write to file
        FileManager::write_to_file(file_path, &log_content)?;

        Ok(())
    }

    // =========================================================================
    // Session Management Helpers
    // =========================================================================

    /// Setup session for translation, returning session info and pending entries
    async fn setup_session(
        &self,
        subtitles: &SubtitleCollection,
    ) -> (
        Option<SessionInfo>,
        Vec<PendingEntry>,
        HashMap<i64, i64>,
    ) {
        let total_entries_count = subtitles.entries.len();

        if let Some(ref session_manager) = self.session_manager {
            let params = SessionCreateParams::new(
                subtitles.source_file.clone(),
                self.config.source_language.clone(),
                self.config.target_language.clone(),
                self.config.translation.provider.to_lowercase_string(),
                self.config.translation.get_model(),
                subtitles.entries.clone(),
            );

            match session_manager.resume_or_create(params).await {
                Ok((session, pending)) => {
                    let is_resume = pending.len() < total_entries_count;

                    if is_resume {
                        info!(
                            "📂 Resuming session {} ({}/{} entries completed)",
                            &session.id[..8],
                            total_entries_count - pending.len(),
                            total_entries_count
                        );
                    } else {
                        debug!("Created new session {}", &session.id[..8]);
                    }

                    // Build seq_num -> source_entry_id mapping
                    let seq_to_source_id: HashMap<i64, i64> = pending
                        .iter()
                        .map(|p| (p.seq_num, p.source_entry_id))
                        .collect();

                    (Some(session), pending, seq_to_source_id)
                }
                Err(e) => {
                    warn!(
                        "Failed to create/resume session: {}. Continuing without persistence.",
                        e
                    );
                    (None, vec![], HashMap::new())
                }
            }
        } else {
            (None, vec![], HashMap::new())
        }
    }

    /// Filter entries based on what's already translated in the session
    async fn filter_entries_for_translation(
        &self,
        all_entries: &[SubtitleEntry],
        pending_entries: &[PendingEntry],
        session_info: &Option<SessionInfo>,
    ) -> (Vec<SubtitleEntry>, Vec<SubtitleEntry>) {
        // If no session, translate everything
        if session_info.is_none() || pending_entries.is_empty() {
            return (all_entries.to_vec(), vec![]);
        }

        // Build set of pending seq_nums
        let pending_seq_nums: std::collections::HashSet<i64> =
            pending_entries.iter().map(|p| p.seq_num).collect();

        let mut to_translate = Vec::new();
        let mut already_done = Vec::new();

        // Get already translated entries from session if we have a session manager
        if let (Some(sm), Some(session)) = (&self.session_manager, session_info) {
            match sm.get_translated_entries(&session.id).await {
                Ok(translated) => {
                    already_done = translated;
                }
                Err(e) => {
                    debug!("Failed to get translated entries: {}", e);
                }
            }
        }

        // Filter entries: only translate those that are pending
        for entry in all_entries {
            if pending_seq_nums.contains(&(entry.seq_num as i64)) {
                to_translate.push(entry.clone());
            }
        }

        (to_translate, already_done)
    }
} 