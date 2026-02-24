/*!
 * Speculative batching for improved throughput.
 *
 * This module implements speculative batching that keeps multiple batches
 * in-flight simultaneously to maximize API throughput, especially for
 * cloud providers with network latency.
 *
 * NOTE: This is an experimental feature that is not yet enabled.
 */

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::subtitle_processor::SubtitleEntry;

/// A window of entries to be translated
#[derive(Debug, Clone)]
pub struct BatchWindow {
    /// Entries in this window
    pub entries: Vec<SubtitleEntry>,
    /// Starting index in the overall entry list
    pub start_index: usize,
}

/// Speculative batcher that keeps N batches in-flight for better throughput
#[derive(Debug)]
pub struct SpeculativeBatcher {
    /// Maximum number of batches to keep in-flight
    max_in_flight: usize,
    /// Queue of prefetched windows ready for processing
    prefetch_queue: Arc<RwLock<VecDeque<BatchWindow>>>,
    /// Number of entries per batch
    entries_per_batch: usize,
}

impl SpeculativeBatcher {
    /// Create a new speculative batcher
    ///
    /// # Arguments
    /// * `max_in_flight` - Maximum number of batches to prefetch
    pub fn new(max_in_flight: usize) -> Self {
        Self {
            max_in_flight: max_in_flight.max(1),
            prefetch_queue: Arc::new(RwLock::new(VecDeque::with_capacity(max_in_flight))),
            entries_per_batch: 3,
        }
    }

    /// Create a speculative batcher with custom batch size
    pub fn with_batch_size(max_in_flight: usize, entries_per_batch: usize) -> Self {
        Self {
            max_in_flight: max_in_flight.max(1),
            prefetch_queue: Arc::new(RwLock::new(VecDeque::with_capacity(max_in_flight))),
            entries_per_batch: entries_per_batch.max(1),
        }
    }

    /// Prefetch next N windows from the entry list
    ///
    /// Fills the prefetch queue with up to `max_in_flight` windows
    /// starting from the given position.
    pub async fn prefetch_next(&self, entries: &[SubtitleEntry], start_pos: usize) {
        let mut queue = self.prefetch_queue.write().await;

        // Calculate how many more windows we can prefetch
        let current_count = queue.len();
        let to_prefetch = self.max_in_flight.saturating_sub(current_count);

        if to_prefetch == 0 {
            return;
        }

        // Calculate the position to start prefetching from
        // (after any already queued windows)
        let mut next_pos = if let Some(last) = queue.back() {
            last.start_index + last.entries.len()
        } else {
            start_pos
        };

        for _ in 0..to_prefetch {
            if next_pos >= entries.len() {
                break;
            }

            let end_pos = (next_pos + self.entries_per_batch).min(entries.len());
            let window = BatchWindow {
                entries: entries[next_pos..end_pos].to_vec(),
                start_index: next_pos,
            };

            queue.push_back(window);
            next_pos = end_pos;
        }
    }

    /// Get the next window from the prefetch queue
    pub async fn next_window(&self) -> Option<BatchWindow> {
        let mut queue = self.prefetch_queue.write().await;
        queue.pop_front()
    }

    /// Check if there are any prefetched windows available
    pub async fn has_prefetched(&self) -> bool {
        let queue = self.prefetch_queue.read().await;
        !queue.is_empty()
    }

    /// Get the number of prefetched windows
    pub async fn prefetch_count(&self) -> usize {
        let queue = self.prefetch_queue.read().await;
        queue.len()
    }

    /// Clear the prefetch queue
    pub async fn clear(&self) {
        let mut queue = self.prefetch_queue.write().await;
        queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entries(count: usize) -> Vec<SubtitleEntry> {
        (0..count)
            .map(|i| SubtitleEntry {
                seq_num: i,
                start_time_ms: i as u64 * 1000,
                end_time_ms: (i + 1) as u64 * 1000,
                text: format!("Entry {}", i),
            })
            .collect()
    }

    #[tokio::test]
    async fn test_speculativeBatcher_prefetchNext_shouldFillQueue() {
        let batcher = SpeculativeBatcher::new(3);
        let entries = make_entries(10);

        batcher.prefetch_next(&entries, 0).await;

        assert_eq!(batcher.prefetch_count().await, 3);
    }

    #[tokio::test]
    async fn test_speculativeBatcher_nextWindow_shouldReturnInOrder() {
        let batcher = SpeculativeBatcher::with_batch_size(3, 2);
        let entries = make_entries(10);

        batcher.prefetch_next(&entries, 0).await;

        let window1 = batcher.next_window().await.unwrap();
        assert_eq!(window1.start_index, 0);
        assert_eq!(window1.entries.len(), 2);

        let window2 = batcher.next_window().await.unwrap();
        assert_eq!(window2.start_index, 2);
    }

    #[tokio::test]
    async fn test_speculativeBatcher_prefetchNext_shouldNotExceedMax() {
        let batcher = SpeculativeBatcher::new(2);
        let entries = make_entries(20);

        batcher.prefetch_next(&entries, 0).await;
        assert_eq!(batcher.prefetch_count().await, 2);

        // Prefetching again shouldn't add more
        batcher.prefetch_next(&entries, 0).await;
        assert_eq!(batcher.prefetch_count().await, 2);
    }

    #[tokio::test]
    async fn test_speculativeBatcher_prefetchNext_shouldHandleEndOfEntries() {
        let batcher = SpeculativeBatcher::with_batch_size(5, 3);
        let entries = make_entries(7);

        batcher.prefetch_next(&entries, 0).await;

        // Should only create 3 windows (0-2, 3-5, 6)
        assert_eq!(batcher.prefetch_count().await, 3);
    }

    #[tokio::test]
    async fn test_speculativeBatcher_clear_shouldEmptyQueue() {
        let batcher = SpeculativeBatcher::new(3);
        let entries = make_entries(10);

        batcher.prefetch_next(&entries, 0).await;
        assert!(batcher.has_prefetched().await);

        batcher.clear().await;
        assert!(!batcher.has_prefetched().await);
    }
}
