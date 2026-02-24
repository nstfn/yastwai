/*!
 * Error types for the yastwai application.
 *
 * This module contains custom error types for different parts of the application,
 * using the thiserror crate for ergonomic error definitions.
 */

use thiserror::Error;

/// Errors that can occur when working with provider APIs
#[derive(Error, Debug)]
pub enum ProviderError {
    /// Error when making an API request fails
    #[error("API request failed: {0}")]
    RequestFailed(String),
    
    /// Error when parsing an API response fails
    #[error("Failed to parse API response: {0}")]
    ParseError(String),
    
    /// Error returned by the API itself
    #[error("API responded with error: {status_code} - {message}")]
    ApiError { 
        /// HTTP status code 
        status_code: u16, 
        /// Error message from the API
        message: String 
    },
    
    /// Error establishing or maintaining a connection
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    /// Error related to rate limiting
    #[error("Rate limit exceeded: {message}")]
    RateLimitExceeded {
        /// Error message from the API
        message: String,
        /// Optional retry-after duration in seconds (from `retry-after` header)
        retry_after_secs: Option<u64>,
    },
    
    /// Error with authentication
    #[error("Authentication error: {0}")]
    AuthenticationError(String),
}

/// Errors that can occur during subtitle processing
#[derive(Error, Debug)]
pub enum SubtitleError {
    /// Invalid or malformed timestamp
    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    /// Malformed SRT structure
    #[error("Malformed SRT: {0}")]
    MalformedSrt(String),

    /// Empty subtitle content
    #[error("Empty subtitle content at entry {0}")]
    EmptySubtitle(usize),
}

/// Errors that can occur during translation
#[derive(Error, Debug)]
pub enum TranslationError {
    /// Error from the provider API
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Error with subtitle processing
    #[error("Subtitle error: {0}")]
    Subtitle(#[from] SubtitleError),
}

/// Main application error type that wraps all other errors
#[derive(Error, Debug)]
pub enum AppError {
    /// Error from a file operation
    #[error("File error: {0}")]
    File(String),
    
    /// Error from a provider
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),
    
    /// Error from subtitle processing
    #[error("Subtitle error: {0}")]
    Subtitle(#[from] SubtitleError),
    
    /// Error from translation
    #[error("Translation error: {0}")]
    Translation(#[from] TranslationError),

    /// Any other error
    #[error("Unknown error: {0}")]
    Unknown(String),
}

// Utility functions for error conversion
impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self::Unknown(error.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::File(error.to_string())
    }
} 