//! Line deduplication for the combat log OCR pipeline.
//!
//! Uses a time-based sliding window (2-second) to filter duplicate OCR lines
//! that persist on screen across multiple capture intervals. Deduplication is
//! based on exact raw line hash — two different OCR strings that happen to parse
//! to the same damage event are treated as distinct events.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

/// Duration of the dedup sliding window (spec invariant 3).
const DEDUP_WINDOW: Duration = Duration::from_secs(2);

/// Filters duplicate OCR lines within a 2-second time window.
///
/// Each raw line is hashed and tracked with a timestamp. If the same hash
/// appears within the window, the line is considered a duplicate. After the
/// window expires, the same line is treated as a new occurrence.
pub struct LineDeduplicator {
    /// Maps line hash → timestamp of first occurrence within the current window.
    seen: HashMap<u64, Instant>,
}

impl LineDeduplicator {
    /// Create a new deduplicator with an empty window.
    pub fn new() -> Self {
        Self {
            seen: HashMap::new(),
        }
    }

    /// Check whether a line is new (not seen within the 2-second window).
    ///
    /// Returns `true` if the line has not been seen within the window (new),
    /// `false` if it's a duplicate. Automatically prunes expired entries.
    pub fn is_new(&mut self, line: &str) -> bool {
        let now = Instant::now();
        self.prune(now);

        let hash = Self::hash_line(line);
        if self.seen.contains_key(&hash) {
            false
        } else {
            self.seen.insert(hash, now);
            true
        }
    }

    /// Remove all entries older than the dedup window.
    fn prune(&mut self, now: Instant) {
        self.seen.retain(|_, timestamp| now.duration_since(*timestamp) < DEDUP_WINDOW);
    }

    /// Hash a line using the default hasher.
    fn hash_line(line: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        line.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_new_line_returns_true() {
        let mut dedup = LineDeduplicator::new();
        assert!(dedup.is_new("Narky pierces a cultist for 7 points of damage."));
    }

    #[test]
    fn test_duplicate_within_window_returns_false() {
        let mut dedup = LineDeduplicator::new();
        let line = "Narky pierces a cultist for 7 points of damage.";
        assert!(dedup.is_new(line));
        assert!(!dedup.is_new(line));
        assert!(!dedup.is_new(line));
    }

    #[test]
    fn test_different_lines_are_independent() {
        let mut dedup = LineDeduplicator::new();
        assert!(dedup.is_new("Narky pierces a cultist for 7 points of damage."));
        assert!(dedup.is_new("Narky slashes a cultist for 10 points of damage."));
        // First line is still duplicate
        assert!(!dedup.is_new("Narky pierces a cultist for 7 points of damage."));
    }

    #[test]
    fn test_same_line_after_window_expires_is_new() {
        let mut dedup = LineDeduplicator::new();
        let line = "Narky pierces a cultist for 7 points of damage.";
        assert!(dedup.is_new(line));
        assert!(!dedup.is_new(line));

        // Wait for the window to expire
        thread::sleep(Duration::from_millis(2100));

        assert!(dedup.is_new(line));
    }

    #[test]
    fn test_empty_line() {
        let mut dedup = LineDeduplicator::new();
        assert!(dedup.is_new(""));
        assert!(!dedup.is_new(""));
    }

    #[test]
    fn test_whitespace_variations_are_distinct() {
        let mut dedup = LineDeduplicator::new();
        assert!(dedup.is_new("Narky pierces a cultist for 7 points of damage."));
        assert!(dedup.is_new(" Narky pierces a cultist for 7 points of damage."));
        assert!(dedup.is_new("Narky pierces a cultist for 7 points of damage. "));
    }

    #[test]
    fn test_prune_removes_old_entries() {
        let mut dedup = LineDeduplicator::new();
        dedup.is_new("line1");
        dedup.is_new("line2");
        assert_eq!(dedup.seen.len(), 2);

        // Wait for window to expire and trigger prune
        thread::sleep(Duration::from_millis(2100));
        dedup.is_new("line3");
        // Old entries pruned, only line3 remains
        assert_eq!(dedup.seen.len(), 1);
    }
}
