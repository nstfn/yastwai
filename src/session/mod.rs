/*!
 * Session management module for translation sessions.
 *
 * This module provides:
 * - Session creation and tracking
 * - Resume capability for interrupted translations
 * - Progress tracking and state management
 */

pub mod manager;
pub mod models;

// Re-export main types
pub use manager::SessionManager;
pub use models::{PendingEntry, SessionCreateParams, SessionInfo};
