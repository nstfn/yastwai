/*!
 * # YASTwAI - Yet Another Subtitle Translator with AI
 * 
 * A Rust library for automatic translation of video subtitles using AI.
 * 
 * ## Features
 * 
 * - Extract subtitles from video files
 * - Translate subtitles using various AI providers:
 *   - Ollama (local LLM)
 *   - OpenAI API
 *   - Anthropic API
 * - Preserve subtitle formatting and timing
 * - Configurable translation parameters
 * - Batch processing for efficient translation
 * - ISO 639-1 and ISO 639-2 language code support
 * 
 * ## Architecture
 * 
 * The library is organized in these main modules:
 * - `app_config`: Configuration management
 * - `subtitle_processor`: Subtitle file handling and processing
 * - `translation`: AI-powered translation services:
 *   - `translation::core`: Core translation functionality
 *   - `translation::batch`: Batch processing of translations
 *   - `translation::cache`: Caching mechanisms for translations
 *   - `translation::formatting`: Format preservation and processing
 * - `file_utils`: File system operations
 * - `app_controller`: Main application controller
 * - `language_utils`: ISO language code utilities
 * - `providers`: Client implementations for various LLM providers:
 *   - `providers::ollama`: Ollama API client
 *   - `providers::openai`: OpenAI API client
 *   - `providers::anthropic`: Anthropic API client
 * - `errors`: Custom error types for the application
 * 
 * ## License
 * 
 * This project is licensed under the MIT License
 */


// Public modules
pub mod app_config;
pub mod file_utils;
pub mod subtitle_processor;
pub mod translation;
pub mod app_controller;
pub mod language_utils;
pub mod providers;
pub mod errors;
pub mod database;
pub mod session;
pub mod validation;
pub mod utils;

// Re-export main types for easier usage
pub use app_config::{Config, SessionConfig, CacheConfig, ValidationConfig};
pub use subtitle_processor::{SubtitleCollection, SubtitleEntry};
pub use translation::TranslationService;
pub use language_utils::{language_codes_match, normalize_to_part2t, get_language_name};
pub use errors::{AppError, ProviderError, SubtitleError, TranslationError};
pub use database::{DatabaseConnection, Repository};
pub use session::{SessionManager, SessionInfo};
pub use validation::{ValidationService, ValidationConfig as ValidationServiceConfig}; 