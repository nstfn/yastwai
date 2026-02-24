/*!
 * Database module for persistent storage of translations and sessions.
 *
 * This module provides SQLite-based persistence for:
 * - Translation sessions with resume capability
 * - Translation cache for cross-session deduplication
 * - Quality validation results
 */

#![allow(unused_imports)]

pub mod schema;
pub mod connection;
pub mod repository;
pub mod models;

// Re-export main types
pub use connection::DatabaseConnection;
pub use repository::Repository;
