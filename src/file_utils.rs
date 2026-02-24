use anyhow::{Result, Context};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use std::fs::OpenOptions;
use std::io::Write;
use chrono::Local;
use tokio::process::Command;
use regex::Regex;
use std::sync::LazyLock;

/// Regex for detecting SRT subtitle format
static SRT_FORMAT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d+\s*\r?\n\d{2}:\d{2}:\d{2},\d{3}\s+-->\s+\d{2}:\d{2}:\d{2},\d{3}").unwrap()
});

// @module: File and directory utilities

// @struct: File operations utility
pub struct FileManager;

impl FileManager {
    /// Checks file existence - used by tests
    pub fn file_exists<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref().exists() && path.as_ref().is_file()
    }
    
    /// Checks directory existence - used by tests
    pub fn dir_exists<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref().exists() && path.as_ref().is_dir()
    }
    
    // @creates: Directory and parents if needed
    pub fn ensure_dir<P: AsRef<Path>>(path: P) -> Result<()> {
        let path = path.as_ref();
        if !path.exists() {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }
    
    /// Generates output path for translated subtitle - used by tests
    pub fn generate_output_path<P1: AsRef<Path>, P2: AsRef<Path>>(
        input_file: P1,
        output_dir: P2,
        target_language: &str,
        extension: &str,
    ) -> PathBuf {
        let input_file = input_file.as_ref();
        let output_dir = output_dir.as_ref();
        
        // Get the file stem (filename without extension)
        let stem = input_file.file_stem().unwrap_or_default();
        
        // Create the output filename with language code and extension
        let mut output_filename = stem.to_string_lossy().to_string();
        output_filename.push('.');
        output_filename.push_str(target_language);
        output_filename.push('.');
        output_filename.push_str(extension);
        
        // Join with the output directory
        output_dir.join(output_filename)
    }
    
    /// Find files with a specific extension in a directory
    pub fn find_files<P: AsRef<Path>>(dir: P, extension: &str) -> Result<Vec<PathBuf>> {
        let mut result = Vec::new();
        let normalized_ext = if extension.starts_with('.') {
            extension.to_string()
        } else {
            format!(".{}", extension)
        };
        
        for entry in WalkDir::new(dir.as_ref()).follow_links(true) {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().eq_ignore_ascii_case(&normalized_ext[1..]) {
                        result.push(path.to_path_buf());
                    }
                }
            }
        }
        
        Ok(result)
    }
    
    /// Read a file to a string
    pub fn read_to_string<P: AsRef<Path>>(path: P) -> Result<String> {
        fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file: {:?}", path.as_ref()))
    }
    
    /// Write a string to a file
    pub fn write_to_file<P: AsRef<Path>>(path: P, content: &str) -> Result<()> {
        // Ensure the parent directory exists
        if let Some(parent) = path.as_ref().parent() {
            Self::ensure_dir(parent)?;
        }
        
        fs::write(&path, content)
            .with_context(|| format!("Failed to write to file: {:?}", path.as_ref()))?;
        
        // No need to log every file write operation
        Ok(())
    }
    
    /// Copy a file from one location to another - used by tests
    pub fn copy_file<P1: AsRef<Path>, P2: AsRef<Path>>(from: P1, to: P2) -> Result<()> {
        let from = from.as_ref();
        let to = to.as_ref();
        
        if !from.exists() {
            return Err(anyhow::anyhow!("Source file does not exist: {:?}", from));
        }
        
        // Ensure the target directory exists
        if let Some(parent) = to.parent() {
            Self::ensure_dir(parent)?;
        }
        
        // Perform the copy
        fs::copy(from, to)?;
        
        Ok(())
    }
    
    /// Append content to a log file with timestamp - utility method
    pub fn append_to_log_file<P: AsRef<Path>>(path: P, content: &str) -> Result<()> {
        // Get current timestamp
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        
        // Ensure the parent directory exists
        if let Some(parent) = path.as_ref().parent() {
            Self::ensure_dir(parent)?;
        }
        
        // Open file in append mode, create if it doesn't exist
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open log file: {:?}", path.as_ref()))?;
        
        // Write content with timestamp
        writeln!(file, "[{}] {}", timestamp, content)
            .with_context(|| format!("Failed to write to log file: {:?}", path.as_ref()))?;
        
        Ok(())
    }

    /// Detect if a file is a subtitle file (SRT) or a video file supported by ffmpeg
    pub async fn detect_file_type<P: AsRef<Path>>(path: P) -> Result<FileType> {
        let path = path.as_ref();
        
        if !path.exists() {
            return Err(anyhow::anyhow!("File does not exist: {:?}", path));
        }
        
        // Check file extension
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            
            // Check if it's a subtitle file
            if ext_str == "srt" {
                return Ok(FileType::Subtitle);
            }
            
            // Common video file extensions supported by ffmpeg
            // This list is not exhaustive but covers the most common formats
            let video_extensions = [
                "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", 
                "mpg", "mpeg", "ogv", "ts", "mts", "m2ts"
            ];
            
            if video_extensions.contains(&ext_str.as_str()) {
                return Ok(FileType::Video);
            }
        }
        
        // If extension check doesn't work, try to examine the file with ffprobe
        // Use timeout to prevent hanging on problematic files
        let ffprobe_future = Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("format=format_name")
            .arg("-of")
            .arg("default=noprint_wrappers=1:nokey=1")
            .arg(path)
            .output();
        
        let timeout_duration = std::time::Duration::from_secs(30); // 30 second timeout
        let output = tokio::select! {
            result = ffprobe_future => result,
            _ = tokio::time::sleep(timeout_duration) => {
                return Err(anyhow::anyhow!("ffprobe command timed out after 30 seconds"));
            }
        };
        
        if let Ok(output) = output {
            if output.status.success() {
                let format = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
                
                // Check if the format is a known video format
                if !format.is_empty() {
                    return Ok(FileType::Video);
                }
            }
        }
        
        // Fall back to examining file contents
        if let Ok(content) = fs::read_to_string(path) {
            // Check for SRT format pattern (sequence number followed by timestamp)
            if content.contains("-->") {
                // Simple check for SRT format: contains "-->" and has a pattern of numbers followed by timestamps
                if SRT_FORMAT_REGEX.is_match(&content) {
                    return Ok(FileType::Subtitle);
                }
            }
        }
        
        // Default to unknown if we couldn't determine the type
        Ok(FileType::Unknown)
    }
}

/// Enum representing different file types
#[derive(Debug, PartialEq, Eq)]
pub enum FileType {
    /// Subtitle file (SRT)
    Subtitle,
    /// Video file supported by ffmpeg
    Video,
    /// Unknown file type
    Unknown,
} 