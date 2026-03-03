//! Mini character panel OCR text parser.
//!
//! Extracts Health, Mana, and Endurance values from the OCR text dump of the
//! M&M mini character panel. The panel displays stats in a compact format like:
//!
//! ```text
//! Health 442/454
//! Mana 185/470
//! Endurance 80/100
//! ```
//!
//! Each field is optional — if a line can't be parsed, that field is `None`.
//! Includes OCR tolerance for common misreads (O→0, l→1, I→1).

use crate::types::VitalStats;
use regex::Regex;

/// Parses vital stats from the mini character panel OCR text.
///
/// Stateless — each call processes the full OCR text independently.
pub struct PanelParser {
    /// Matches "Health {current}/{max}" with OCR-tolerant numbers.
    health_re: Regex,
    /// Matches "Mana {current}/{max}" with OCR-tolerant numbers.
    mana_re: Regex,
    /// Matches "Endurance {current}/{max}" or "End {current}/{max}".
    endurance_re: Regex,
}

impl PanelParser {
    /// Create a new panel parser with compiled regex patterns.
    pub fn new() -> Self {
        // OCR-tolerant number pattern: digits plus common misreads O, l, I
        // These get cleaned up during numeric parsing.
        let num = r"[0-9OlI]+";

        Self {
            health_re: Regex::new(&format!(
                r"(?i)Health\s+({num})\s*/\s*({num})"
            )).expect("health regex"),
            mana_re: Regex::new(&format!(
                r"(?i)Mana\s+({num})\s*/\s*({num})"
            )).expect("mana regex"),
            endurance_re: Regex::new(&format!(
                r"(?i)(?:Endurance|End)\s+({num})\s*/\s*({num})"
            )).expect("endurance regex"),
        }
    }

    /// Parse the full OCR text from the mini panel and extract vital stats.
    ///
    /// Searches the entire text for Health, Mana, and Endurance patterns.
    /// Each field is independently optional — partial parses are fine.
    pub fn parse_panel_text(&self, ocr_text: &str) -> VitalStats {
        let mut stats = VitalStats::default();

        if let Some(caps) = self.health_re.captures(ocr_text) {
            stats.hp_current = Self::ocr_to_u32(&caps[1]);
            stats.hp_max = Self::ocr_to_u32(&caps[2]);
        }

        if let Some(caps) = self.mana_re.captures(ocr_text) {
            stats.mana_current = Self::ocr_to_u32(&caps[1]);
            stats.mana_max = Self::ocr_to_u32(&caps[2]);
        }

        if let Some(caps) = self.endurance_re.captures(ocr_text) {
            stats.endurance_current = Self::ocr_to_u32(&caps[1]);
            stats.endurance_max = Self::ocr_to_u32(&caps[2]);
        }

        stats
    }

    /// Convert an OCR-captured number string to u32, handling common misreads.
    ///
    /// Substitutes: O→0, l→1, I→1 before parsing.
    fn ocr_to_u32(s: &str) -> Option<u32> {
        let cleaned: String = s
            .chars()
            .map(|c| match c {
                'O' | 'o' => '0',
                'l' | 'I' => '1',
                other => other,
            })
            .collect();
        cleaned.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> PanelParser {
        PanelParser::new()
    }

    #[test]
    fn test_parse_full_panel() {
        let text = "Health 442/454\nMana 185/470\nEndurance 80/100\n";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.hp_current, Some(442));
        assert_eq!(stats.hp_max, Some(454));
        assert_eq!(stats.mana_current, Some(185));
        assert_eq!(stats.mana_max, Some(470));
        assert_eq!(stats.endurance_current, Some(80));
        assert_eq!(stats.endurance_max, Some(100));
    }

    #[test]
    fn test_parse_mana_only() {
        let text = "some garbage\nMana 300/500\nmore garbage";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.mana_current, Some(300));
        assert_eq!(stats.mana_max, Some(500));
        assert!(stats.hp_current.is_none());
        assert!(stats.endurance_current.is_none());
    }

    #[test]
    fn test_parse_endurance_abbreviated() {
        let text = "End 50/100";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.endurance_current, Some(50));
        assert_eq!(stats.endurance_max, Some(100));
    }

    #[test]
    fn test_ocr_tolerance_o_for_zero() {
        let text = "Health 44O/5OO\nMana 1O5/47O\n";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.hp_current, Some(440));
        assert_eq!(stats.hp_max, Some(500));
        assert_eq!(stats.mana_current, Some(105));
        assert_eq!(stats.mana_max, Some(470));
    }

    #[test]
    fn test_ocr_tolerance_l_for_one() {
        let text = "Mana l85/470";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.mana_current, Some(185));
        assert_eq!(stats.mana_max, Some(470));
    }

    #[test]
    fn test_ocr_tolerance_capital_i_for_one() {
        let text = "Mana I85/470";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.mana_current, Some(185));
        assert_eq!(stats.mana_max, Some(470));
    }

    #[test]
    fn test_empty_text_returns_all_none() {
        let stats = parser().parse_panel_text("");
        assert!(stats.hp_current.is_none());
        assert!(stats.hp_max.is_none());
        assert!(stats.mana_current.is_none());
        assert!(stats.mana_max.is_none());
        assert!(stats.endurance_current.is_none());
        assert!(stats.endurance_max.is_none());
    }

    #[test]
    fn test_garbage_text_returns_all_none() {
        let stats = parser().parse_panel_text("asdf jkl; 123 random garbage !@#$");
        assert!(stats.hp_current.is_none());
        assert!(stats.mana_current.is_none());
        assert!(stats.endurance_current.is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let text = "HEALTH 100/200\nmana 50/100\nENDURANCE 30/60";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.hp_current, Some(100));
        assert_eq!(stats.mana_current, Some(50));
        assert_eq!(stats.endurance_current, Some(30));
    }

    #[test]
    fn test_extra_whitespace() {
        let text = "Health  442 / 454\nMana   185  /  470";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.hp_current, Some(442));
        assert_eq!(stats.hp_max, Some(454));
        assert_eq!(stats.mana_current, Some(185));
        assert_eq!(stats.mana_max, Some(470));
    }

    #[test]
    fn test_panel_with_other_stats_interspersed() {
        let text = "\
STR 75  STA 80
AGI 85  DEX 90
INT 120 WIS 100
CHA 70
Health 442/454
Mana 185/470
Endurance 80/100
AC 350
Weight 45/120";
        let stats = parser().parse_panel_text(text);
        assert_eq!(stats.hp_current, Some(442));
        assert_eq!(stats.hp_max, Some(454));
        assert_eq!(stats.mana_current, Some(185));
        assert_eq!(stats.mana_max, Some(470));
        assert_eq!(stats.endurance_current, Some(80));
        assert_eq!(stats.endurance_max, Some(100));
    }
}
