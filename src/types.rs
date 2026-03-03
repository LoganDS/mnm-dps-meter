//! Core types and traits for the mnm-dps-meter application.
//!
//! Defines all shared data structures, event schemas, and trait boundaries
//! used across the OCR pipelines, session accumulator, and UI.

use anyhow::Result;
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// A rectangular region of the screen defined by absolute coordinates.
///
/// Used to specify both the combat log capture area and the mini character
/// panel capture area. Coordinates are screen-absolute (not window-relative).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A single parsed damage event from the M&M combat log.
///
/// Represents one line of combat text that has been successfully parsed
/// into structured data. Events are append-only once added to a session.
#[derive(Debug, Clone)]
pub struct DamageEvent {
    /// Unique identifier within the session (auto-incremented).
    pub id: u64,
    /// Time the event was captured.
    pub timestamp: Instant,
    /// The entity dealing damage. "You"/"Your" is translated to the
    /// configured character name if set.
    pub source_player: String,
    /// For melee: the verb form (pierce, slash, etc.).
    /// For spells: the spell name (Spirit Frost, Ice Comet, etc.).
    pub attack_name: String,
    /// The entity receiving damage, including article (e.g., "a cultist").
    pub target: String,
    /// The numeric damage value.
    pub damage_value: u32,
    /// Element type if specified (Cold, Fire, Magic, Poison), or None for melee.
    pub damage_element: Option<String>,
    /// The original OCR text line for debugging/audit.
    pub raw_line: String,
}

/// Classification of a mana change event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaEventType {
    /// Positive delta — mana regeneration tick occurred.
    Regen,
    /// Negative delta — spell cast or mana expenditure.
    Spend,
}

/// A detected mana change event based on the character panel's mana numbers.
///
/// Because we OCR exact numbers (e.g., "185/470"), mana tracking is precise
/// to the integer — no estimation needed.
#[derive(Debug, Clone)]
pub struct ManaTickEvent {
    /// Unique identifier within the session (auto-incremented).
    pub id: u64,
    /// Time the event was detected.
    pub timestamp: Instant,
    /// Mana value before the change.
    pub previous_mana: u32,
    /// Mana value after the change.
    pub current_mana: u32,
    /// Max mana at time of reading.
    pub max_mana: u32,
    /// Change in mana (current - previous). Positive = regen, negative = spend.
    pub delta: i32,
    /// Classification: Regen if delta > 0, Spend if delta < 0.
    pub event_type: ManaEventType,
}

/// Vital stats parsed from the mini character panel OCR.
///
/// Each field is optional — if a particular line can't be parsed from the
/// OCR output, that field is None.
#[derive(Debug, Clone, Default)]
pub struct VitalStats {
    pub hp_current: Option<u32>,
    pub hp_max: Option<u32>,
    pub mana_current: Option<u32>,
    pub mana_max: Option<u32>,
    pub endurance_current: Option<u32>,
    pub endurance_max: Option<u32>,
}

/// Current status of the capture session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// Capture threads are actively running.
    Running,
    /// Capture is paused; UI remains interactive.
    Paused,
}

/// Messages sent from capture pipeline threads to the session accumulator via mpsc channels.
#[derive(Debug)]
pub enum PipelineMessage {
    /// A batch of parsed damage events from the combat log pipeline.
    DamageEvents(Vec<DamageEvent>),
    /// Updated vital stats from the mini panel pipeline.
    ManaUpdate(VitalStats),
    /// A detected mana tick event from the tick detector.
    ManaTick(ManaTickEvent),
}

/// Trait for OCR engines that convert images to text.
///
/// Implementations are platform-specific (WinRT on Windows, Tesseract on Linux/macOS).
/// Mock implementations are provided for testing.
pub trait OcrEngine: Send {
    /// Perform OCR on the given image and return the extracted text.
    fn ocr_image(&self, image: &DynamicImage) -> Result<String>;
}

/// Trait for screen capture backends.
///
/// Implementations capture a rectangular region of the screen and return it
/// as a `DynamicImage`. Mock implementations are provided for testing.
pub trait ScreenCapture: Send {
    /// Capture the specified screen region and return the image.
    fn capture_region(&self, region: &CaptureRegion) -> Result<DynamicImage>;
}
