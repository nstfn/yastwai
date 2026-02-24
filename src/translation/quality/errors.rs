/*!
 * Comprehensive error handling with recovery strategies.
 *
 * Provides structured error types and recovery mechanisms for:
 * - API/Network errors
 * - Parsing errors
 * - Validation errors
 * - Rate limiting
 * - Timeout handling
 */

use std::time::Duration;

/// Categories of translation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranslationErrorKind {
    /// Network connectivity issues
    Network,

    /// API rate limiting
    RateLimit,

    /// Request timeout
    Timeout,

    /// Invalid API response
    InvalidResponse,

    /// JSON parsing failed
    ParseError,

    /// Validation failed
    ValidationFailed,

    /// Provider-specific error
    ProviderError,

    /// Configuration error
    ConfigError,

    /// Resource exhausted (memory, disk)
    ResourceExhausted,

    /// Unknown/unexpected error
    Unknown,
}

impl TranslationErrorKind {
    /// Check if this error type is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            TranslationErrorKind::Network
                | TranslationErrorKind::RateLimit
                | TranslationErrorKind::Timeout
                | TranslationErrorKind::InvalidResponse
        )
    }

    /// Get recommended retry delay for this error type.
    pub fn recommended_delay(&self) -> Duration {
        match self {
            TranslationErrorKind::RateLimit => Duration::from_secs(60),
            TranslationErrorKind::Timeout => Duration::from_secs(5),
            TranslationErrorKind::Network => Duration::from_secs(10),
            TranslationErrorKind::InvalidResponse => Duration::from_secs(2),
            _ => Duration::from_secs(1),
        }
    }

    /// Get maximum recommended retries for this error type.
    pub fn max_retries(&self) -> usize {
        match self {
            TranslationErrorKind::RateLimit => 5,
            TranslationErrorKind::Network => 3,
            TranslationErrorKind::Timeout => 3,
            TranslationErrorKind::InvalidResponse => 2,
            _ => 1,
        }
    }
}

/// A translation error with context.
#[derive(Debug, Clone)]
pub struct TranslationError {
    /// The kind of error
    pub kind: TranslationErrorKind,

    /// Error message
    pub message: String,

    /// Source error message (if wrapped)
    pub source: Option<String>,

    /// Entry IDs affected (if applicable)
    pub affected_entries: Vec<usize>,

    /// Number of retries attempted
    pub retry_count: usize,

    /// Whether recovery was attempted
    pub recovery_attempted: bool,
}

impl TranslationError {
    /// Create a new error.
    pub fn new(kind: TranslationErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.to_string(),
            source: None,
            affected_entries: Vec::new(),
            retry_count: 0,
            recovery_attempted: false,
        }
    }

    /// Create from an underlying error.
    pub fn from_source(kind: TranslationErrorKind, message: &str, source: &str) -> Self {
        Self {
            kind,
            message: message.to_string(),
            source: Some(source.to_string()),
            affected_entries: Vec::new(),
            retry_count: 0,
            recovery_attempted: false,
        }
    }

    /// Add affected entries.
    pub fn with_entries(mut self, entries: Vec<usize>) -> Self {
        self.affected_entries = entries;
        self
    }

    /// Mark retry count.
    pub fn with_retries(mut self, count: usize) -> Self {
        self.retry_count = count;
        self
    }

    /// Check if more retries are recommended.
    pub fn should_retry(&self) -> bool {
        self.kind.is_retryable() && self.retry_count < self.kind.max_retries()
    }

    /// Get the recommended delay before retry.
    pub fn retry_delay(&self) -> Duration {
        // Exponential backoff based on retry count
        let base_delay = self.kind.recommended_delay();
        let multiplier = 2u32.saturating_pow(self.retry_count as u32);
        base_delay * multiplier
    }

    /// Get a user-friendly error description.
    pub fn user_message(&self) -> String {
        match self.kind {
            TranslationErrorKind::Network => {
                "Network connection error. Please check your internet connection.".to_string()
            }
            TranslationErrorKind::RateLimit => {
                "API rate limit reached. Please wait before retrying.".to_string()
            }
            TranslationErrorKind::Timeout => {
                "Request timed out. The server may be overloaded.".to_string()
            }
            TranslationErrorKind::InvalidResponse => {
                "Received invalid response from translation service.".to_string()
            }
            TranslationErrorKind::ParseError => {
                "Failed to parse translation response.".to_string()
            }
            TranslationErrorKind::ValidationFailed => {
                format!("Translation validation failed: {}", self.message)
            }
            TranslationErrorKind::ProviderError => {
                format!("Translation provider error: {}", self.message)
            }
            TranslationErrorKind::ConfigError => {
                format!("Configuration error: {}", self.message)
            }
            TranslationErrorKind::ResourceExhausted => {
                "System resources exhausted. Please free up memory or disk space.".to_string()
            }
            TranslationErrorKind::Unknown => {
                format!("Unexpected error: {}", self.message)
            }
        }
    }
}

impl std::fmt::Display for TranslationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)?;
        if let Some(ref source) = self.source {
            write!(f, " (caused by: {})", source)?;
        }
        Ok(())
    }
}

impl std::error::Error for TranslationError {}

/// Recovery action to take after an error.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryAction {
    /// Retry the same operation
    Retry {
        delay: Duration,
        modified_params: bool,
    },

    /// Skip the affected entries
    Skip {
        entries: Vec<usize>,
    },

    /// Use fallback translation (original text)
    UseFallback {
        entries: Vec<usize>,
    },

    /// Reduce batch size and retry
    ReduceBatchSize {
        new_size: usize,
    },

    /// Switch to a different provider
    SwitchProvider {
        reason: String,
    },

    /// Abort the operation
    Abort {
        reason: String,
    },

    /// Continue with partial results
    ContinuePartial {
        completed: Vec<usize>,
        failed: Vec<usize>,
    },
}

impl RecoveryAction {
    /// Check if this action allows continuing.
    pub fn allows_continuation(&self) -> bool {
        !matches!(self, RecoveryAction::Abort { .. })
    }

    /// Get a description of this action.
    pub fn description(&self) -> String {
        match self {
            RecoveryAction::Retry { delay, modified_params } => {
                if *modified_params {
                    format!("Retry with modified parameters after {:?}", delay)
                } else {
                    format!("Retry after {:?}", delay)
                }
            }
            RecoveryAction::Skip { entries } => {
                format!("Skip {} entries", entries.len())
            }
            RecoveryAction::UseFallback { entries } => {
                format!("Use original text for {} entries", entries.len())
            }
            RecoveryAction::ReduceBatchSize { new_size } => {
                format!("Reduce batch size to {}", new_size)
            }
            RecoveryAction::SwitchProvider { reason } => {
                format!("Switch provider: {}", reason)
            }
            RecoveryAction::Abort { reason } => {
                format!("Abort: {}", reason)
            }
            RecoveryAction::ContinuePartial { completed, failed } => {
                format!(
                    "Continue with {} completed, {} failed",
                    completed.len(),
                    failed.len()
                )
            }
        }
    }
}

/// Strategy for error recovery.
#[derive(Debug, Clone)]
pub struct RecoveryStrategy {
    /// Maximum total retries
    pub max_retries: usize,

    /// Maximum time to spend on retries
    pub max_retry_duration: Duration,

    /// Whether to use fallback on persistent failure
    pub use_fallback: bool,

    /// Whether to allow partial completion
    pub allow_partial: bool,

    /// Minimum batch size (for reduction strategy)
    pub min_batch_size: usize,

    /// Whether to switch providers on repeated failure
    pub allow_provider_switch: bool,
}

impl Default for RecoveryStrategy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            max_retry_duration: Duration::from_secs(300), // 5 minutes
            use_fallback: true,
            allow_partial: true,
            min_batch_size: 1,
            allow_provider_switch: false,
        }
    }
}

impl RecoveryStrategy {
    /// Create an aggressive strategy that tries hard to recover.
    pub fn aggressive() -> Self {
        Self {
            max_retries: 5,
            max_retry_duration: Duration::from_secs(600),
            use_fallback: true,
            allow_partial: true,
            min_batch_size: 1,
            allow_provider_switch: true,
        }
    }

    /// Create a fast-fail strategy.
    pub fn fast_fail() -> Self {
        Self {
            max_retries: 1,
            max_retry_duration: Duration::from_secs(30),
            use_fallback: false,
            allow_partial: false,
            min_batch_size: 10,
            allow_provider_switch: false,
        }
    }
}

/// Error recovery handler.
pub struct ErrorRecovery {
    strategy: RecoveryStrategy,
    total_retries: usize,
    errors_seen: Vec<TranslationError>,
}

impl ErrorRecovery {
    /// Create a new recovery handler.
    pub fn new() -> Self {
        Self {
            strategy: RecoveryStrategy::default(),
            total_retries: 0,
            errors_seen: Vec::new(),
        }
    }

    /// Create with custom strategy.
    pub fn with_strategy(strategy: RecoveryStrategy) -> Self {
        Self {
            strategy,
            total_retries: 0,
            errors_seen: Vec::new(),
        }
    }

    /// Determine recovery action for an error.
    pub fn handle_error(&mut self, error: &TranslationError) -> RecoveryAction {
        self.errors_seen.push(error.clone());

        // Check if we've exceeded total retries
        if self.total_retries >= self.strategy.max_retries {
            return self.final_action(error);
        }

        // Determine action based on error kind
        match error.kind {
            TranslationErrorKind::RateLimit => {
                self.total_retries += 1;
                RecoveryAction::Retry {
                    delay: error.retry_delay(),
                    modified_params: false,
                }
            }

            TranslationErrorKind::Network | TranslationErrorKind::Timeout => {
                if error.should_retry() {
                    self.total_retries += 1;
                    RecoveryAction::Retry {
                        delay: error.retry_delay(),
                        modified_params: false,
                    }
                } else if self.strategy.use_fallback {
                    RecoveryAction::UseFallback {
                        entries: error.affected_entries.clone(),
                    }
                } else {
                    RecoveryAction::Abort {
                        reason: error.message.clone(),
                    }
                }
            }

            TranslationErrorKind::InvalidResponse | TranslationErrorKind::ParseError => {
                // Try reducing batch size
                if self.strategy.min_batch_size < error.affected_entries.len() {
                    self.total_retries += 1;
                    RecoveryAction::ReduceBatchSize {
                        new_size: (error.affected_entries.len() / 2).max(self.strategy.min_batch_size),
                    }
                } else if error.should_retry() {
                    self.total_retries += 1;
                    RecoveryAction::Retry {
                        delay: error.retry_delay(),
                        modified_params: true,
                    }
                } else {
                    self.final_action(error)
                }
            }

            TranslationErrorKind::ValidationFailed => {
                if self.strategy.allow_partial && !error.affected_entries.is_empty() {
                    // Continue with what we have, mark failures
                    RecoveryAction::ContinuePartial {
                        completed: Vec::new(), // Caller should fill this
                        failed: error.affected_entries.clone(),
                    }
                } else if self.strategy.use_fallback {
                    RecoveryAction::UseFallback {
                        entries: error.affected_entries.clone(),
                    }
                } else {
                    RecoveryAction::Abort {
                        reason: error.message.clone(),
                    }
                }
            }

            TranslationErrorKind::ProviderError => {
                if self.strategy.allow_provider_switch {
                    RecoveryAction::SwitchProvider {
                        reason: error.message.clone(),
                    }
                } else if error.should_retry() {
                    self.total_retries += 1;
                    RecoveryAction::Retry {
                        delay: error.retry_delay(),
                        modified_params: false,
                    }
                } else {
                    self.final_action(error)
                }
            }

            TranslationErrorKind::ConfigError => {
                RecoveryAction::Abort {
                    reason: format!("Configuration error: {}", error.message),
                }
            }

            TranslationErrorKind::ResourceExhausted => {
                RecoveryAction::Abort {
                    reason: "System resources exhausted".to_string(),
                }
            }

            TranslationErrorKind::Unknown => {
                if error.should_retry() {
                    self.total_retries += 1;
                    RecoveryAction::Retry {
                        delay: error.retry_delay(),
                        modified_params: false,
                    }
                } else {
                    self.final_action(error)
                }
            }
        }
    }

    /// Determine final action when retries are exhausted.
    fn final_action(&self, error: &TranslationError) -> RecoveryAction {
        if self.strategy.use_fallback && !error.affected_entries.is_empty() {
            RecoveryAction::UseFallback {
                entries: error.affected_entries.clone(),
            }
        } else if self.strategy.allow_partial {
            RecoveryAction::Skip {
                entries: error.affected_entries.clone(),
            }
        } else {
            RecoveryAction::Abort {
                reason: format!("Max retries exceeded: {}", error.message),
            }
        }
    }

    /// Reset the recovery state.
    pub fn reset(&mut self) {
        self.total_retries = 0;
        self.errors_seen.clear();
    }

    /// Get all errors seen.
    pub fn errors(&self) -> &[TranslationError] {
        &self.errors_seen
    }

    /// Get total retries performed.
    pub fn retry_count(&self) -> usize {
        self.total_retries
    }

    /// Check if any errors have occurred.
    pub fn has_errors(&self) -> bool {
        !self.errors_seen.is_empty()
    }

    /// Get summary of errors.
    pub fn error_summary(&self) -> String {
        if self.errors_seen.is_empty() {
            return "No errors".to_string();
        }

        let mut by_kind: std::collections::HashMap<TranslationErrorKind, usize> =
            std::collections::HashMap::new();
        for err in &self.errors_seen {
            *by_kind.entry(err.kind).or_insert(0) += 1;
        }

        let parts: Vec<String> = by_kind
            .iter()
            .map(|(kind, count)| format!("{:?}: {}", kind, count))
            .collect();

        format!(
            "{} errors ({} retries): {}",
            self.errors_seen.len(),
            self.total_retries,
            parts.join(", ")
        )
    }
}

impl Default for ErrorRecovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translationErrorKind_isRetryable_shouldIdentifyCorrectly() {
        assert!(TranslationErrorKind::Network.is_retryable());
        assert!(TranslationErrorKind::RateLimit.is_retryable());
        assert!(TranslationErrorKind::Timeout.is_retryable());
        assert!(!TranslationErrorKind::ConfigError.is_retryable());
        assert!(!TranslationErrorKind::ResourceExhausted.is_retryable());
    }

    #[test]
    fn test_translationError_shouldRetry_shouldRespectLimits() {
        let error = TranslationError::new(TranslationErrorKind::Network, "test");
        assert!(error.should_retry());

        let error_at_limit = error.with_retries(10);
        assert!(!error_at_limit.should_retry());
    }

    #[test]
    fn test_translationError_retryDelay_shouldUseExponentialBackoff() {
        let error = TranslationError::new(TranslationErrorKind::RateLimit, "test");
        let delay0 = error.retry_delay();

        let error1 = error.clone().with_retries(1);
        let delay1 = error1.retry_delay();

        let error2 = error.clone().with_retries(2);
        let delay2 = error2.retry_delay();

        assert!(delay1 > delay0);
        assert!(delay2 > delay1);
    }

    #[test]
    fn test_errorRecovery_handleError_shouldRetryNetwork() {
        let mut recovery = ErrorRecovery::new();
        let error = TranslationError::new(TranslationErrorKind::Network, "connection failed");

        let action = recovery.handle_error(&error);

        assert!(matches!(action, RecoveryAction::Retry { .. }));
    }

    #[test]
    fn test_errorRecovery_handleError_shouldAbortOnConfig() {
        let mut recovery = ErrorRecovery::new();
        let error = TranslationError::new(TranslationErrorKind::ConfigError, "invalid config");

        let action = recovery.handle_error(&error);

        assert!(matches!(action, RecoveryAction::Abort { .. }));
    }

    #[test]
    fn test_errorRecovery_handleError_shouldReduceBatchOnParse() {
        let mut recovery = ErrorRecovery::new();
        let error = TranslationError::new(TranslationErrorKind::ParseError, "invalid json")
            .with_entries(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

        let action = recovery.handle_error(&error);

        assert!(matches!(action, RecoveryAction::ReduceBatchSize { .. }));
    }

    #[test]
    fn test_errorRecovery_handleError_shouldRespectMaxRetries() {
        let mut recovery = ErrorRecovery::with_strategy(RecoveryStrategy {
            max_retries: 2,
            use_fallback: true,
            ..Default::default()
        });

        let error = TranslationError::new(TranslationErrorKind::Network, "failed")
            .with_entries(vec![1, 2]);

        // First retry
        let action1 = recovery.handle_error(&error);
        assert!(matches!(action1, RecoveryAction::Retry { .. }));

        // Second retry
        let action2 = recovery.handle_error(&error);
        assert!(matches!(action2, RecoveryAction::Retry { .. }));

        // Should now use fallback
        let action3 = recovery.handle_error(&error);
        assert!(matches!(action3, RecoveryAction::UseFallback { .. }));
    }

    #[test]
    fn test_recoveryAction_description_shouldBeReadable() {
        let action = RecoveryAction::Retry {
            delay: Duration::from_secs(5),
            modified_params: false,
        };
        assert!(action.description().contains("5s"));

        let action = RecoveryAction::ReduceBatchSize { new_size: 5 };
        assert!(action.description().contains("5"));
    }
}
