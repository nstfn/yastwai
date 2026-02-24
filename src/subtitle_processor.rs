use std::fs;
use std::fs::File;
use std::fmt;
use regex::Regex;
use std::sync::LazyLock;
use anyhow::{Result, Context, anyhow};
use std::io::Write;
use std::path::{Path, PathBuf};
use log::{error, warn, debug};
use serde_json::{Value, from_str};
use crate::app_config::SubtitleInfo;
use crate::language_utils;
use tokio::process::Command;

// @module: Subtitle processing and manipulation

// @const: SRT timestamp regex
static TIMESTAMP_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{2}):(\d{2}):(\d{2}),(\d{3}) --> (\d{2}):(\d{2}):(\d{2}),(\d{3})").unwrap()
});

// @struct: Single subtitle entry
#[derive(Debug, Clone)]
pub struct SubtitleEntry {
    // @field: Sequence number
    pub seq_num: usize,
    
    // @field: Start time in ms
    pub start_time_ms: u64,
    
    // @field: End time in ms
    pub end_time_ms: u64,
    
    // @field: Subtitle text
    pub text: String,
}

impl SubtitleEntry {
    /// Creates a new subtitle entry - used by tests and external consumers
    pub fn new(seq_num: usize, start_time_ms: u64, end_time_ms: u64, text: String) -> Self {
        SubtitleEntry {
            seq_num,
            start_time_ms,
            end_time_ms,
            text,
        }
    }
    
    // @creates: Validated subtitle entry
    // @validates: Time range and non-empty text
    pub fn new_validated(seq_num: usize, start_time_ms: u64, end_time_ms: u64, text: String) -> Result<Self> {
        // Validate time range
        if end_time_ms <= start_time_ms {
            return Err(anyhow!(
                "Invalid time range: end time {} <= start time {}",
                end_time_ms, start_time_ms
            ));
        }

        // Validate text is not empty (after trimming)
        let trimmed_text = text.trim();
        if trimmed_text.is_empty() {
            return Err(anyhow!("Empty subtitle text for entry {}", seq_num));
        }

        Ok(SubtitleEntry {
            seq_num,
            start_time_ms,
            end_time_ms,
            text: trimmed_text.to_string(),
        })
    }
    
    /// Parse an SRT timestamp to milliseconds - used by tests
    pub fn parse_timestamp(timestamp: &str) -> Result<u64> {
        // Parse HH:MM:SS,mmm format
        let parts: Vec<&str> = timestamp.split(&[':', ',', '.'][..]).collect();
        
        if parts.len() != 4 {
            return Err(anyhow!("Invalid timestamp format: {}", timestamp));
        }
        
        let hours: u64 = parts[0].parse().context("Failed to parse hours")?;
        let minutes: u64 = parts[1].parse().context("Failed to parse minutes")?;
        let seconds: u64 = parts[2].parse().context("Failed to parse seconds")?;
        let millis: u64 = parts[3].parse().context("Failed to parse milliseconds")?;
        
        // Validate time components
        if minutes >= 60 || seconds >= 60 || millis >= 1000 {
            return Err(anyhow!("Invalid time components in timestamp: {}", timestamp));
        }
        
        Ok(hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + millis)
    }
    
    /// Convert start time to formatted SRT timestamp
    pub fn format_start_time(&self) -> String {
        Self::format_timestamp(self.start_time_ms)
    }
    
    /// Convert end time to formatted SRT timestamp
    pub fn format_end_time(&self) -> String {
        Self::format_timestamp(self.end_time_ms)
    }
    
    /// Format a timestamp in milliseconds to SRT format (HH:MM:SS,mmm)
    pub fn format_timestamp(ms: u64) -> String {
        let hours = ms / 3_600_000;
        let minutes = (ms % 3_600_000) / 60_000;
        let seconds = (ms % 60_000) / 1_000;
        let millis = ms % 1_000;
        
        format!("{:02}:{:02}:{:02},{:03}", hours, minutes, seconds, millis)
    }
    
    
    
    
}

impl fmt::Display for SubtitleEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.seq_num)?;
        writeln!(f, "{} --> {}", self.format_start_time(), self.format_end_time())?;
        writeln!(f, "{}", self.text)?;
        writeln!(f)
    }
}

/// Collection of subtitle entries with metadata
#[derive(Debug)]
pub struct SubtitleCollection {
    /// Source filename
    pub source_file: PathBuf,
    
    /// List of subtitle entries
    pub entries: Vec<SubtitleEntry>,
    
    /// Source language
    pub source_language: String,
}

impl SubtitleCollection {
    /// Create a new subtitle collection
    pub fn new(source_file: PathBuf, source_language: String) -> Self {
        SubtitleCollection {
            source_file,
            entries: Vec::new(),
            source_language,
        }
    }
    
    /// Extract subtitles from a video file
    pub async fn extract_from_video<P: AsRef<Path>>(video_path: P, track_id: usize, source_language: &str, output_path: P) -> Result<Self> {
        let video_path = video_path.as_ref();
        let output_path = output_path.as_ref();

        if !video_path.exists() {
            return Err(anyhow!("Video file does not exist: {:?}", video_path));
        }

        // Normalize language code if possible, but continue if not
        let normalized_language = match language_utils::normalize_to_part1_or_part2t(source_language) {
            Ok(lang) => lang,
            Err(e) => {
                warn!("Language code issue: {}", e);
                source_language.to_string()
            }
        };

        // Use ffmpeg to extract the subtitle directly to SRT file
        // Add timeout to prevent hanging on problematic files
        let ffmpeg_future = Command::new("ffmpeg")
            .args([
                "-y",                       // Overwrite existing file
                "-i", video_path.to_str().unwrap_or_default(),
                "-map", &format!("0:{}", track_id),
                "-c:s", "srt",              // SRT output format
                output_path.to_str().unwrap_or_default()
            ])
            .output();

        let timeout_duration = std::time::Duration::from_secs(120); // 2 minute timeout for ffmpeg
        let result = tokio::select! {
            result = ffmpeg_future => {
                result.map_err(|e| anyhow!("Failed to execute ffmpeg command for subtitle extraction: {}", e))?
            },
            _ = tokio::time::sleep(timeout_duration) => {
                return Err(anyhow!("ffmpeg command timed out after 2 minutes"));
            }
        };

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            let filtered = Self::filter_ffmpeg_stderr(&stderr);
            error!("Subtitle extraction failed: {}", filtered);
            return Err(anyhow!("ffmpeg extraction failed: {}", filtered));
        }
        
        // If no errors occurred, parse the output file
        let file_size = std::fs::metadata(output_path)?.len();
        if file_size == 0 {
            return Err(anyhow!("Extracted file is empty — no subtitles found in track {}", track_id));
        }

        let entries = Self::parse_srt_file(output_path)?;
        if entries.is_empty() {
            return Err(anyhow!("Failed to parse any subtitle entries from extracted track {}", track_id));
        }
                
        Ok(SubtitleCollection {
            source_file: output_path.to_path_buf(),
            entries,
            source_language: normalized_language,
        })
    }
    
    /// Write subtitles to an SRT file
    pub fn write_to_srt<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        let _file_name = path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| String::from("Unknown file"));
            
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        
        // Write to file
        let mut file = File::create(path)
            .with_context(|| format!("Failed to create subtitle file: {}", path.display()))?;
        
        // Write each entry to the file
        for entry in &self.entries {
            write!(file, "{}", entry)?;
        }
        
        Ok(())
    }
    
    /// Split subtitles into chunks for translation
    /// 
    /// This method divides the subtitle entries into chunks that don't exceed the specified 
    /// maximum character count, ensuring that each chunk contains a coherent set of subtitle entries.
    /// The chunks are optimized to maximize batch size while respecting the character limit.
    pub fn split_into_chunks(&self, max_chars_per_chunk: usize) -> Vec<Vec<SubtitleEntry>> {
        if self.entries.is_empty() {
            warn!("No subtitle entries to split into chunks");
            return Vec::new();
        }
        
        // Protect against accidental loss of subtitles - count at the beginning
        let total_entries = self.entries.len();
        
        // Handle unreasonably small max_chars by enforcing a minimum
        let effective_max_chars = max_chars_per_chunk.max(100);
        
        // For Anthropic provider, consider using smaller chunks to improve reliability
        // We can infer this is likely an Anthropic request if the max_chars is very large (>8000)
        let is_likely_anthropic = max_chars_per_chunk > 8000;
        
        // If this appears to be an Anthropic request, use a more conservative size limit
        // This helps prevent truncated responses by keeping chunks smaller
        let actual_max_chars = if is_likely_anthropic {
            // Use a smaller effective size for Anthropic to improve reliability
            // For Claude-3-Haiku, keep chunks especially small to avoid max_tokens errors
            (effective_max_chars / 3).min(2500)
        } else {
            // For other providers, use a more conservative limit to reduce the chances of missing entries
            // This helps ensure more entries are translated in batches rather than individually
            (effective_max_chars * 2 / 3).min(4000)
        };
        
        // Calculate a target entries per chunk (not more than 40 entries per chunk to avoid missing entries)
        // This helps prevent the LLM from dropping entries in large batches
        let target_max_entries_per_chunk = 40;
        
        let mut chunks = Vec::new();
        let mut current_chunk = Vec::with_capacity(target_max_entries_per_chunk);
        let mut current_size = 0;
        let mut current_entries = 0;
        
        for entry in &self.entries {
            let entry_size = entry.text.len();
            
            // If a single entry exceeds the limit, it needs its own chunk
            if entry_size > actual_max_chars {
                // If we have entries in the current chunk, finalize it first
                if !current_chunk.is_empty() {
                    chunks.push(current_chunk);
                    current_chunk = Vec::with_capacity(1);
                    current_size = 0;
                    current_entries = 0;
                }
                
                // Add the oversized entry as its own chunk
                debug!("Entry {} is oversized ({} chars), placing in its own chunk", 
                       entry.seq_num, entry_size);
                chunks.push(vec![entry.clone()]);
                continue;
            }
            
            // If adding this entry would exceed the limit by characters or entry count, finalize the current chunk
            if (current_size + entry_size > actual_max_chars || current_entries >= target_max_entries_per_chunk) && !current_chunk.is_empty() {
                chunks.push(current_chunk);
                current_chunk = Vec::with_capacity(target_max_entries_per_chunk);
                current_size = 0;
                current_entries = 0;
            }
            
            // Add the entry to the current chunk
            current_chunk.push(entry.clone());
            current_size += entry_size;
            current_entries += 1;
        }
        
        // Add the last chunk if it's not empty
        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }
        
        // Verify that all entries have been included in the chunks
        let total_chunked_entries: usize = chunks.iter().map(|chunk| chunk.len()).sum();
        if total_chunked_entries != total_entries {
            error!("CRITICAL ERROR: Lost entries during chunking! Original: {}, After chunking: {}", 
                   total_entries, total_chunked_entries);
        } else {
            // Add detailed chunk information in debug mode
            if log::max_level() >= log::LevelFilter::Debug {
                for (i, chunk) in chunks.iter().enumerate() {
                    let chunk_seq_nums: Vec<usize> = chunk.iter().map(|e| e.seq_num).collect();
                    let chunk_chars: usize = chunk.iter().map(|e| e.text.len()).sum();
                    debug!("Chunk {}: {} entries (seq_nums: {:?}, total {} chars)", 
                           i+1, chunk.len(), chunk_seq_nums, chunk_chars);
                }
            }
        }
        
        chunks
    }
    
    /// List subtitle tracks in a video file
    pub async fn list_subtitle_tracks<P: AsRef<Path>>(video_path: P) -> Result<Vec<SubtitleInfo>> {
        let video_path = video_path.as_ref();
        
        // Check if the file exists
        if !video_path.exists() {
            return Err(anyhow!("Video file not found: {:?}", video_path));
        }
        
        // Remove verbose stack trace and unnecessary ffprobe details logs
        // Add timeout to prevent hanging on problematic files
        let ffprobe_future = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-print_format", "json",
                "-show_streams",
                "-select_streams", "s",
                video_path.to_str().unwrap_or("")
            ])
            .output();
        
        let timeout_duration = std::time::Duration::from_secs(60); // 1 minute timeout
        let output = tokio::select! {
            result = ffprobe_future => {
                result.map_err(|e| anyhow!("Failed to execute ffprobe command: {}", e))?
            },
            _ = tokio::time::sleep(timeout_duration) => {
                return Err(anyhow!("ffprobe command timed out after 60 seconds"));
            }
        };
            
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("ffprobe failed: {}", stderr);
            return Err(anyhow!("ffprobe command failed: {}", stderr));
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        if stdout.trim().is_empty() {
            // Removed: warn!("ffprobe returned empty output");
            return Ok(Vec::new());
        }
        
        let json: Value = from_str(&stdout)
            .context("Failed to parse ffprobe JSON output")?;
        
        let mut tracks = Vec::new();
        
        if let Some(streams) = json.get("streams").and_then(|s| s.as_array()) {
            // Instead of logging each stream detail, just log the total count
            // Removed: warn!("Found {} subtitle streams", streams.len());
            
            for stream in streams.iter() {
                let index = stream.get("index")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(0);
                
                let codec_name = stream.get("codec_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                
                let language = stream.get("tags")
                    .and_then(|t| t.get("language"))
                    .and_then(|l| l.as_str())
                    .map(|s| s.to_string());
                
                let title = stream.get("tags")
                    .and_then(|t| t.get("title"))
                    .and_then(|l| l.as_str())
                    .map(|s| s.to_string());
                
                let track = SubtitleInfo {
                    index,
                    codec_name: codec_name.to_string(),
                    language,
                    title,
                };
                
                tracks.push(track);
            }
        } else {
            // Removed: warn!("No subtitle streams found in video");
        }
        
        Ok(tracks)
    }
    
    /// Check if a subtitle codec is bitmap-based (cannot be converted to text SRT)
    fn is_bitmap_codec(codec_name: &str) -> bool {
        matches!(
            codec_name,
            "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub"
        )
    }

    /// Filter ffmpeg stderr to only show meaningful error lines, stripping the
    /// version banner, build configuration, and stream metadata noise.
    fn filter_ffmpeg_stderr(stderr: &str) -> String {
        let dominated_prefixes = [
            "ffmpeg version",
            "  built with",
            "  configuration:",
            "  lib",
            "Input #",
            "  Metadata:",
            "  Duration:",
            "  Chapter",
            "    Chapter",
            "  Stream #",
            "      Metadata:",
            "        title",
            "        BPS",
            "        DURATION",
            "        NUMBER_OF",
            "        _STATISTICS",
            "Output #",
            "Stream mapping:",
            "Press [q]",
        ];

        let meaningful: Vec<&str> = stderr
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return false;
                }
                !dominated_prefixes.iter().any(|p| trimmed.starts_with(p))
            })
            .collect();

        if meaningful.is_empty() {
            "unknown ffmpeg error (stderr was empty after filtering)".to_string()
        } else {
            meaningful.join("\n")
        }
    }

    /// Select a subtitle track based on preferred language
    pub fn select_subtitle_track(tracks: &[SubtitleInfo], preferred_language: &str) -> Option<usize> {
        if tracks.is_empty() {
            return None;
        }
        
        // Try to find the preferred language - first check for ISO language code match
        for track in tracks {
            if let Some(track_lang) = &track.language {
                // Use language_utils to compare ISO codes (handles both 639-1 and 639-2 codes)
                if language_utils::language_codes_match(track_lang, preferred_language) {             
                    return Some(track.index);
                }
            }
            
            // Also check title for language mention
            if let Some(title) = &track.title {
                // Try to normalize preferred language to get the name
                if let Ok(pref_name) = language_utils::get_language_name(preferred_language) {
                    let title_lower = title.to_lowercase();
                    let name_lower = pref_name.to_lowercase();
                    
                    // Check if the language name is in the title
                    if title_lower.contains(&name_lower) {
                        return Some(track.index);
                    }
                }
                
                // Also check for language code in title
                let title_lower = title.to_lowercase();
                if title_lower.contains(&preferred_language.to_lowercase()) {
                    return Some(track.index);
                }
            }
        }
        
        // Try to find English if preferred language not found (using ISO codes)
        if !language_utils::language_codes_match(preferred_language, "en") {
            for track in tracks {
                if let Some(lang) = &track.language {
                    if language_utils::language_codes_match(lang, "en") {
                        return Some(track.index);
                    }
                }
                
                // Also check title for English mention
                if let Some(title) = &track.title {
                    if title.to_lowercase().contains("english") {
                        return Some(track.index);
                    }
                }
            }
        }
        
        // If neither preferred nor English found, use the first track
        if !tracks.is_empty() {
            let first_track = tracks.first().unwrap().index;
            return Some(first_track);
        }
        
        None
    }
    
    /// Extract subtitles from a video file with automatic track selection
    pub async fn extract_with_auto_track_selection<P: AsRef<Path>>(
        video_path: P, 
        preferred_language: &str,
        output_path: Option<&Path>,
        source_language: &str
    ) -> Result<Self> {
        let video_path = video_path.as_ref();
        
        // List all subtitle tracks
        let tracks = Self::list_subtitle_tracks(video_path).await?;
        
        // Exit early if no subtitle streams found
        if tracks.is_empty() {
            return Err(anyhow::anyhow!("No subtitle tracks found in the video"));
        }

        // Check if all tracks are bitmap-based (cannot be converted to text)
        let text_tracks: Vec<&SubtitleInfo> = tracks.iter()
            .filter(|t| !Self::is_bitmap_codec(&t.codec_name))
            .collect();

        if text_tracks.is_empty() {
            let codec_list: Vec<String> = tracks.iter()
                .map(|t| {
                    let lang = t.language.as_deref().unwrap_or("?");
                    format!("track {} ({}, {})", t.index, lang, t.codec_name)
                })
                .collect();
            return Err(anyhow::anyhow!(
                "All subtitle tracks are bitmap-based (image) and cannot be converted to text SRT. \
                 Found: {}. Bitmap subtitles (PGS/VobSub) require OCR to convert to text.",
                codec_list.join(", ")
            ));
        }

        // Warn about skipped bitmap tracks
        let bitmap_count = tracks.len() - text_tracks.len();
        if bitmap_count > 0 {
            warn!(
                "Skipping {} bitmap subtitle track(s) (PGS/VobSub) — only text-based tracks can be extracted",
                bitmap_count
            );
        }

        // Select from text tracks only
        let track_id = Self::select_subtitle_track(&text_tracks.iter().map(|t| (*t).clone()).collect::<Vec<_>>(), preferred_language)
            .ok_or_else(|| anyhow::anyhow!("No text-based subtitle track found for language: {}", preferred_language))?;
        
        // Extract the selected track
        if let Some(output_path) = output_path {
            Self::extract_from_video(video_path, track_id, source_language, output_path).await
        } else {
            // Extract to a temporary file first
            let temp_filename = format!("extracted_subtitle_{}.srt", track_id);
            let temp_path = std::env::temp_dir().join(&temp_filename);

            let result = Self::extract_from_video(video_path, track_id, source_language, &temp_path).await;

            // Clean up temporary file
            if temp_path.exists() {
                let _ = std::fs::remove_file(&temp_path);
            }

            // Update source_file to point to the original video file (not the deleted temp file)
            // This is important for session management - the session should be identified by
            // the source video, not the temporary extracted subtitle file
            result.map(|mut collection| {
                collection.source_file = video_path.to_path_buf();
                collection
            })
        }
    }

    /// Extract source language subtitle to memory
    pub async fn extract_source_language_subtitle_to_memory<P: AsRef<Path>>(video_path: P, source_language: &str) -> Result<Self> {
        let video_path = video_path.as_ref();

        debug!("Extracting {source_language} subtitles from video (in-memory)");
        
        // Avoiding additional logs by passing directly to extract_with_auto_track_selection
        Self::extract_with_auto_track_selection(video_path, source_language, None, source_language).await
    }
    
    /// Fast extraction using ffmpeg subtitle copy
    pub async fn fast_extract_source_subtitles<P: AsRef<Path>>(video_path: P, source_language: &str) -> Result<Self> {
        debug!("Fast extracting subtitles directly for language: {}", source_language);
        
        // Call extract_with_auto_track_selection directly
        Self::extract_with_auto_track_selection(video_path, source_language, None, source_language).await
    }

    
    /// Parse SRT file content to subtitle entries
    fn parse_srt_file(path: &Path) -> Result<Vec<SubtitleEntry>> {
        let content = fs::read_to_string(path)?;
        Self::parse_srt_string(&content)
    }
    
    /// Parse SRT format string into subtitle entries
    pub fn parse_srt_string(content: &str) -> Result<Vec<SubtitleEntry>> {
        let mut entries = Vec::new();
        let lines = content.lines().peekable();
        
        // State variables for parsing
        let mut current_seq_num: Option<usize> = None;
        let mut current_start_time_ms: Option<u64> = None;
        let mut current_end_time_ms: Option<u64> = None;
        let mut current_text = String::new();
        let mut line_count = 0;
        
        // Helper function to add the current entry if complete
        let mut add_current_entry = |seq_num: usize, start_ms: u64, end_ms: u64, text: &str| {
            if !text.trim().is_empty() {
                match SubtitleEntry::new_validated(seq_num, start_ms, end_ms, text.trim().to_string()) {
                    Ok(entry) => {
                        entries.push(entry);
                        true
                    },
                    Err(e) => {
                        warn!("Skipping invalid subtitle entry {}: {}", seq_num, e);
                        false
                    }
                }
            } else {
                warn!("Skipping empty subtitle entry {}", seq_num);
                false
            }
        };
        
        for line in lines {
            line_count += 1;
            let trimmed = line.trim();
            
            // Skip empty lines, but check if we need to finalize the current entry
            if trimmed.is_empty() {
                if let (Some(seq_num), Some(start_ms), Some(end_ms)) = (current_seq_num, current_start_time_ms, current_end_time_ms) {
                    if !current_text.is_empty() {
                        add_current_entry(seq_num, start_ms, end_ms, &current_text);
                        
                        // Reset state for next entry
                        current_seq_num = None;
                        current_start_time_ms = None;
                        current_end_time_ms = None;
                        current_text.clear();
                    }
                }
                continue;
            }
            
            // Try to parse as sequence number (only if we're starting a new entry)
            if current_seq_num.is_none() && current_text.is_empty() {
                if let Ok(num) = trimmed.parse::<usize>() {
                    current_seq_num = Some(num);
                    continue;
                }
            }
            
            // Try to parse as timestamp
            if current_seq_num.is_some() && current_start_time_ms.is_none() && current_end_time_ms.is_none() {
                if let Some(caps) = TIMESTAMP_REGEX.captures(trimmed) {
                    match (Self::parse_timestamp_to_ms(&caps, 1), Self::parse_timestamp_to_ms(&caps, 5)) {
                        (Ok(start_ms), Ok(end_ms)) => {
                            current_start_time_ms = Some(start_ms);
                            current_end_time_ms = Some(end_ms);
                            continue;
                        },
                        _ => {
                            // Invalid timestamp format, but we'll treat it as text
                            warn!("Invalid timestamp format at line {}: {}", line_count, trimmed);
                        }
                    }
                }
            }
            
            // If we have a sequence number and timestamps, this must be subtitle text
            if current_seq_num.is_some() && current_start_time_ms.is_some() && current_end_time_ms.is_some() {
                if !current_text.is_empty() {
                    current_text.push('\n');
                }
                current_text.push_str(trimmed);
            } else {
                // We have text but no sequence number or timestamps yet
                // This is likely malformed SRT, but we'll try to recover
                warn!("Unexpected text at line {} before sequence number or timestamp: {}", line_count, trimmed);
            }
        }
        
        // Add the last entry if there is one
        if let (Some(seq_num), Some(start_ms), Some(end_ms)) = (current_seq_num, current_start_time_ms, current_end_time_ms) {
            if !current_text.is_empty() {
                add_current_entry(seq_num, start_ms, end_ms, &current_text);
            }
        }
        
        // Validate and sort entries
        if entries.is_empty() {
            warn!("No valid subtitle entries found in content");
            return Err(anyhow::anyhow!("No valid subtitle entries were found in the SRT content"));
        }
        
        // Sort by start time to ensure correct order
        entries.sort_by_key(|entry| entry.start_time_ms);
        
        // Check for overlapping entries
        let mut overlap_count = 0;
        for i in 0..entries.len().saturating_sub(1) {
            if entries[i].end_time_ms > entries[i+1].start_time_ms {
                overlap_count += 1;
            }
        }
        
        if overlap_count > 0 {
            warn!("Found {} overlapping subtitle entries", overlap_count);
        }
        
        // Renumber entries to ensure sequential order
        for (i, entry) in entries.iter_mut().enumerate() {
            entry.seq_num = i + 1;
        }
        
        Ok(entries)
    }
    
    /// Parse timestamp to milliseconds
    fn parse_timestamp_to_ms(caps: &regex::Captures, start_idx: usize) -> Result<u64> {
        let hours: u64 = caps.get(start_idx)
            .map_or(0, |m| m.as_str().parse().unwrap_or(0));
        let minutes: u64 = caps.get(start_idx + 1)
            .map_or(0, |m| m.as_str().parse().unwrap_or(0));
        let seconds: u64 = caps.get(start_idx + 2)
            .map_or(0, |m| m.as_str().parse().unwrap_or(0));
        let millis: u64 = caps.get(start_idx + 3)
            .map_or(0, |m| m.as_str().parse().unwrap_or(0));
            
        Ok((hours * 3600 + minutes * 60 + seconds) * 1000 + millis)
    }
}

impl fmt::Display for SubtitleCollection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Subtitle Collection")?;
        writeln!(f, "Source: {:?}", self.source_file)?;
        writeln!(f, "Language: {}", self.source_language)?;
        writeln!(f, "Entries: {}", self.entries.len())?;
        Ok(())
    }
} 