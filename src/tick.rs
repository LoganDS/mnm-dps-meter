//! Mana tick detector for the mini panel OCR pipeline.
//!
//! Compares successive mana readings to detect regen ticks (positive delta)
//! and spell spend events (negative delta). Zero delta produces no event.
//! Mana tracking is precise to the integer since we OCR exact numbers like
//! "185/470" from the mini character panel.

use crate::types::{ManaEventType, ManaTickEvent};
use std::time::Instant;

/// Detects mana tick events by comparing successive mana readings.
///
/// Maintains the previous mana reading and auto-increments event IDs.
/// Each non-zero delta produces a [`ManaTickEvent`] classified as either
/// [`ManaEventType::Regen`] (positive) or [`ManaEventType::Spend`] (negative).
pub struct TickDetector {
    /// Previous mana reading. `None` until the first reading is processed.
    last_mana: Option<u32>,
    /// Auto-incrementing event ID counter.
    next_id: u64,
}

impl TickDetector {
    /// Create a new tick detector with no previous reading.
    pub fn new() -> Self {
        Self {
            last_mana: None,
            next_id: 1,
        }
    }

    /// Process a new mana reading and return a tick event if mana changed.
    ///
    /// - First reading: stores the value and returns `None`.
    /// - Same as previous: returns `None` (no change).
    /// - Increased: returns `Some` with `ManaEventType::Regen`.
    /// - Decreased: returns `Some` with `ManaEventType::Spend`.
    pub fn process_reading(&mut self, current_mana: u32, max_mana: u32) -> Option<ManaTickEvent> {
        let previous = match self.last_mana {
            None => {
                self.last_mana = Some(current_mana);
                return None;
            }
            Some(prev) => prev,
        };

        if current_mana == previous {
            return None;
        }

        let delta = current_mana as i32 - previous as i32;
        let event_type = if delta > 0 {
            ManaEventType::Regen
        } else {
            ManaEventType::Spend
        };

        let event = ManaTickEvent {
            id: self.next_id,
            timestamp: Instant::now(),
            previous_mana: previous,
            current_mana,
            max_mana,
            delta,
            event_type,
        };

        self.next_id += 1;
        self.last_mana = Some(current_mana);

        Some(event)
    }

    /// Reset the detector state for a new session.
    ///
    /// Clears the previous mana reading so the next reading is treated
    /// as the first. Does not reset the ID counter.
    pub fn reset(&mut self) {
        self.last_mana = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_reading_returns_none() {
        let mut detector = TickDetector::new();
        assert!(detector.process_reading(185, 470).is_none());
    }

    #[test]
    fn test_no_change_returns_none() {
        let mut detector = TickDetector::new();
        detector.process_reading(185, 470);
        assert!(detector.process_reading(185, 470).is_none());
        assert!(detector.process_reading(185, 470).is_none());
    }

    #[test]
    fn test_regen_tick() {
        let mut detector = TickDetector::new();
        detector.process_reading(185, 470);

        let event = detector.process_reading(195, 470).expect("should detect regen");
        assert_eq!(event.id, 1);
        assert_eq!(event.previous_mana, 185);
        assert_eq!(event.current_mana, 195);
        assert_eq!(event.max_mana, 470);
        assert_eq!(event.delta, 10);
        assert_eq!(event.event_type, ManaEventType::Regen);
    }

    #[test]
    fn test_spend_event() {
        let mut detector = TickDetector::new();
        detector.process_reading(300, 470);

        let event = detector.process_reading(250, 470).expect("should detect spend");
        assert_eq!(event.previous_mana, 300);
        assert_eq!(event.current_mana, 250);
        assert_eq!(event.delta, -50);
        assert_eq!(event.event_type, ManaEventType::Spend);
    }

    #[test]
    fn test_sequential_regen_ticks() {
        let mut detector = TickDetector::new();
        detector.process_reading(100, 470);

        let e1 = detector.process_reading(110, 470).unwrap();
        assert_eq!(e1.id, 1);
        assert_eq!(e1.delta, 10);
        assert_eq!(e1.event_type, ManaEventType::Regen);

        let e2 = detector.process_reading(120, 470).unwrap();
        assert_eq!(e2.id, 2);
        assert_eq!(e2.previous_mana, 110);
        assert_eq!(e2.delta, 10);
    }

    #[test]
    fn test_spend_then_regen() {
        let mut detector = TickDetector::new();
        detector.process_reading(300, 470);

        // Spell cast
        let spend = detector.process_reading(250, 470).unwrap();
        assert_eq!(spend.event_type, ManaEventType::Spend);
        assert_eq!(spend.delta, -50);

        // Regen tick
        let regen = detector.process_reading(260, 470).unwrap();
        assert_eq!(regen.event_type, ManaEventType::Regen);
        assert_eq!(regen.previous_mana, 250);
        assert_eq!(regen.delta, 10);
    }

    #[test]
    fn test_ids_auto_increment() {
        let mut detector = TickDetector::new();
        detector.process_reading(100, 470);

        let e1 = detector.process_reading(110, 470).unwrap();
        let e2 = detector.process_reading(120, 470).unwrap();
        let e3 = detector.process_reading(100, 470).unwrap();

        assert_eq!(e1.id, 1);
        assert_eq!(e2.id, 2);
        assert_eq!(e3.id, 3);
    }

    #[test]
    fn test_reset_clears_last_mana() {
        let mut detector = TickDetector::new();
        detector.process_reading(185, 470);

        detector.reset();

        // After reset, next reading is treated as first
        assert!(detector.process_reading(185, 470).is_none());
    }

    #[test]
    fn test_reset_preserves_id_counter() {
        let mut detector = TickDetector::new();
        detector.process_reading(100, 470);
        let _ = detector.process_reading(110, 470); // id=1

        detector.reset();
        detector.process_reading(200, 470); // first after reset

        let event = detector.process_reading(210, 470).unwrap();
        assert_eq!(event.id, 2); // continues from 2, not 1
    }

    #[test]
    fn test_mana_at_zero() {
        let mut detector = TickDetector::new();
        detector.process_reading(0, 470);

        let event = detector.process_reading(10, 470).unwrap();
        assert_eq!(event.event_type, ManaEventType::Regen);
        assert_eq!(event.previous_mana, 0);
        assert_eq!(event.delta, 10);
    }

    #[test]
    fn test_mana_at_max() {
        let mut detector = TickDetector::new();
        detector.process_reading(470, 470);

        // No change at full mana
        assert!(detector.process_reading(470, 470).is_none());

        // Spend from full
        let event = detector.process_reading(420, 470).unwrap();
        assert_eq!(event.event_type, ManaEventType::Spend);
        assert_eq!(event.delta, -50);
    }

    #[test]
    fn test_single_point_change() {
        let mut detector = TickDetector::new();
        detector.process_reading(100, 470);

        let event = detector.process_reading(101, 470).unwrap();
        assert_eq!(event.delta, 1);
        assert_eq!(event.event_type, ManaEventType::Regen);

        let event = detector.process_reading(100, 470).unwrap();
        assert_eq!(event.delta, -1);
        assert_eq!(event.event_type, ManaEventType::Spend);
    }
}
