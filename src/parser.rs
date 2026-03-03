//! Regex-based prose combat log parser with OCR tolerance.
//!
//! Parses M&M natural English combat log lines into structured [`DamageEvent`]s
//! using anchor-based backwards extraction. The parser finds the damage number
//! first (`"for {N} point(s) of"`), then works backwards to extract the source,
//! attack, and target.

use crate::types::DamageEvent;
use regex::Regex;
use std::time::Instant;

/// Known melee verbs from the M&M combat log.
const MELEE_VERBS: &[&str] = &[
    "pierces",
    "hits",
    "slashes",
    "crushes",
    "bashes",
    "kicks",
    "bites",
    "claws",
    "stings",
    "mauls",
    "strikes",
    "smashes",
    "punches",
    "backstabs",
];

/// Regex-based parser for M&M prose combat log lines.
///
/// Uses anchor-based backwards extraction: finds the damage number first,
/// then works backwards to identify source, attack type, and target.
/// Handles common OCR misreads and translates "You"/"Your" to the
/// configured character name.
pub struct CombatLogParser {
    /// The player's character name for You/Your translation.
    /// If None, "You"/"Your" are kept as-is.
    character_name: Option<String>,
    /// Auto-incrementing event ID counter.
    next_id: u64,
    /// Matches the damage anchor: `for {N} point(s) of [element] damage.`
    /// OCR-tolerant: handles p0ints, polnts, darnage, etc.
    damage_anchor: Regex,
    /// Matches miss lines: `tries to hit/pierce/etc ... but misses!`
    miss_pattern: Regex,
    /// Matches heal lines: `heals ... for ... Health.`
    heal_pattern: Regex,
    /// Matches death lines: `has been slain!` or `You have been slain!`
    death_pattern: Regex,
    /// Matches experience lines: `You gain experience!` or `You gain {N} experience!`
    experience_pattern: Regex,
    /// Matches possessive: `Entity's` or `Your` (including OCR variants like `'5`, `'S`)
    possessive_pattern: Regex,
}

impl CombatLogParser {
    /// Create a new parser with an optional character name for You/Your translation.
    ///
    /// Compiles all regex patterns once at construction time for performance.
    pub fn new(character_name: Option<String>) -> Self {
        // Damage anchor: "for {N} point(s) of [element] damage."
        // OCR tolerance: p0ints, polnts, polnt, p0int, darnage, darnaqe, dam*
        // Both "damage." and "Damage." are matched (element lines use capital D)
        let damage_anchor = Regex::new(
            r"(?i)\bfor\s+(\S+)\s+p\w*nt\w*\s+of\s+(?:(\w+)\s+)?d\w*[gq]e[.\s]"
        ).expect("damage_anchor regex");

        let miss_pattern = Regex::new(
            r"(?i)tries\s+to\s+\w+\s+.+,?\s*but\s+misses?"
        ).expect("miss_pattern regex");

        let heal_pattern = Regex::new(
            r"(?i)\bheals?\b.+\bfor\b.+\bhealth\b"
        ).expect("heal_pattern regex");

        let death_pattern = Regex::new(
            r"(?i)\bhas\s+been\s+slain\s*!"
        ).expect("death_pattern regex");

        let experience_pattern = Regex::new(
            r"(?i)\bgain\b.*\bexperience\b"
        ).expect("experience_pattern regex");

        // Matches possessive forms: `Narky's`, `Creature's`, `Your`
        // OCR tolerance: `'5`, `'S` instead of `'s`
        let possessive_pattern = Regex::new(
            r"^(.+?)['\u{2018}\u{2019}][sS5]\s+"
        ).expect("possessive_pattern regex");

        Self {
            character_name,
            next_id: 0,
            damage_anchor,
            miss_pattern,
            heal_pattern,
            death_pattern,
            experience_pattern,
            possessive_pattern,
        }
    }

    /// Update the character name used for You/Your translation.
    pub fn set_character_name(&mut self, name: Option<String>) {
        self.character_name = name;
    }

    /// Parse a single combat log line into a [`DamageEvent`], or `None` if the
    /// line is not a damage event (miss, heal, death, experience, or garbage).
    ///
    /// Uses anchor-based backwards extraction:
    /// 1. Discard partial lines (must end with `.` or `!`)
    /// 2. Skip non-damage lines (miss, heal, death, experience)
    /// 3. Find damage anchor: `for {N} point(s) of [element] damage`
    /// 4. Classify as spell or melee, extract fields accordingly
    pub fn parse_line(&mut self, line: &str) -> Option<DamageEvent> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        // Partial line check: must end with `.` or `!`
        let last_char = line.chars().last()?;
        if last_char != '.' && last_char != '!' {
            return None;
        }

        // Skip non-damage lines
        if self.miss_pattern.is_match(line) {
            return None;
        }
        if self.heal_pattern.is_match(line) {
            return None;
        }
        if self.death_pattern.is_match(line) {
            return None;
        }
        if self.experience_pattern.is_match(line) {
            return None;
        }

        // Find the damage anchor
        let caps = self.damage_anchor.captures(line)?;
        let full_match = caps.get(0)?;
        let raw_damage = caps.get(1)?.as_str();
        let element = caps.get(2).map(|m| m.as_str().to_string());

        // Parse damage value with OCR tolerance (O→0, l→1, I→1)
        let damage_value = parse_ocr_number(raw_damage)?;

        // Everything before `for {N}` is the "prefix" containing source, attack, and target
        let anchor_start = full_match.start();
        let prefix = line[..anchor_start].trim();

        // Filter out element "damage" — if the element is a loose match for "damage"
        // itself, that means there was no actual element (the regex over-captured)
        let element = element.and_then(|e| {
            if e.to_lowercase().starts_with("dam") || e.to_lowercase().starts_with("d") && e.len() <= 3 {
                None
            } else {
                Some(e)
            }
        });

        // Try to parse as a spell line first (possessive pattern), then as melee
        if let Some(event) = self.try_parse_spell(prefix, damage_value, &element, line) {
            return Some(event);
        }

        if let Some(event) = self.try_parse_melee(prefix, damage_value, &element, line) {
            return Some(event);
        }

        // Catch-all fallback: {player} {any_word} {target} for {N} point(s) of damage.
        self.try_parse_fallback(prefix, damage_value, &element, line)
    }

    /// Try to parse a spell line: `{entity}'s {spell_name} hits {target}`
    /// or `Your {spell_name} hits {target}`
    fn try_parse_spell(
        &mut self,
        prefix: &str,
        damage_value: u32,
        element: &Option<String>,
        raw_line: &str,
    ) -> Option<DamageEvent> {
        // Check for "Your " at the start
        let (source, remainder) = if prefix.starts_with("Your ") || prefix.starts_with("your ") {
            let source = self
                .character_name
                .clone()
                .unwrap_or_else(|| "You".to_string());
            (source, &prefix[5..])
        } else if let Some(caps) = self.possessive_pattern.captures(prefix) {
            let entity = caps.get(1)?.as_str().to_string();
            let remainder_start = caps.get(0)?.end();
            (entity, &prefix[remainder_start..])
        } else {
            return None;
        };

        // For spell lines, find the LAST "hits" before the end of the prefix
        // to split spell_name from target
        let remainder_lower = remainder.to_lowercase();
        let hits_pos = remainder_lower.rfind(" hits ")?;

        let spell_name = remainder[..hits_pos].trim().to_string();
        let target = remainder[hits_pos + 6..].trim().to_string();

        if spell_name.is_empty() || target.is_empty() {
            return None;
        }

        let id = self.next_id;
        self.next_id += 1;

        Some(DamageEvent {
            id,
            timestamp: Instant::now(),
            source_player: source,
            attack_name: spell_name,
            target,
            damage_value,
            damage_element: element.clone(),
            raw_line: raw_line.to_string(),
        })
    }

    /// Try to parse a melee line: `{player} {verb} {target}`
    fn try_parse_melee(
        &mut self,
        prefix: &str,
        damage_value: u32,
        element: &Option<String>,
        raw_line: &str,
    ) -> Option<DamageEvent> {
        // Find a known melee verb in the prefix
        let prefix_lower = prefix.to_lowercase();

        let mut best_match: Option<(usize, &str)> = None;

        for verb in MELEE_VERBS {
            // Search for " verb " pattern to match whole words
            let pattern = format!(" {} ", verb);
            if let Some(pos) = prefix_lower.find(&pattern) {
                // Use the first (leftmost) verb match — the verb comes right after the player name
                if best_match.is_none() || pos < best_match.unwrap().0 {
                    best_match = Some((pos, verb));
                }
            }
        }

        let (verb_pos, verb) = best_match?;

        let player = prefix[..verb_pos].trim().to_string();
        let target = prefix[verb_pos + verb.len() + 2..].trim().to_string();

        if player.is_empty() || target.is_empty() {
            return None;
        }

        // You/Your translation for melee
        let source_player = self.translate_you(&player);

        // Convert verb from third-person to base form for attack_name
        let attack_name = verb_to_base_form(verb);

        let id = self.next_id;
        self.next_id += 1;

        Some(DamageEvent {
            id,
            timestamp: Instant::now(),
            source_player,
            attack_name: attack_name.to_string(),
            target,
            damage_value,
            damage_element: element.clone(),
            raw_line: raw_line.to_string(),
        })
    }

    /// Fallback parser: `{player} {any_word} {target}` — for unknown verbs.
    fn try_parse_fallback(
        &mut self,
        prefix: &str,
        damage_value: u32,
        element: &Option<String>,
        raw_line: &str,
    ) -> Option<DamageEvent> {
        // Split on first space to get player, then next space for verb, rest is target
        let mut words = prefix.splitn(3, ' ');
        let player = words.next()?.trim().to_string();
        let verb = words.next()?.trim().to_string();
        let target = words.next()?.trim().to_string();

        if player.is_empty() || verb.is_empty() || target.is_empty() {
            return None;
        }

        let source_player = self.translate_you(&player);
        let attack_name = verb_to_base_form(&verb);

        let id = self.next_id;
        self.next_id += 1;

        Some(DamageEvent {
            id,
            timestamp: Instant::now(),
            source_player,
            attack_name,
            target,
            damage_value,
            damage_element: element.clone(),
            raw_line: raw_line.to_string(),
        })
    }

    /// Translate "You" to the configured character name.
    fn translate_you(&self, name: &str) -> String {
        if name.eq_ignore_ascii_case("you") {
            self.character_name
                .clone()
                .unwrap_or_else(|| name.to_string())
        } else {
            name.to_string()
        }
    }
}

/// Convert a third-person verb form to its base form for attack_name display.
///
/// e.g., "pierces" → "pierce", "slashes" → "slash", "hits" → "hit"
fn verb_to_base_form(verb: &str) -> String {
    let v = verb.to_lowercase();
    if v.ends_with("shes") || v.ends_with("ches") {
        // slashes → slash, crushes → crush, bashes → bash, smashes → smash, punches → punch
        v[..v.len() - 2].to_string()
    } else if v.ends_with("es") {
        // pierces → pierce, strikes → strike, maules → maul
        v[..v.len() - 1].to_string()
    } else if v.ends_with("s") {
        // hits → hit, kicks → kick, bites → bite, claws → claw
        v[..v.len() - 1].to_string()
    } else {
        v
    }
}

/// Parse a number from OCR text with tolerance for common misreads.
///
/// Substitutions: `O` → `0`, `o` → `0`, `l` → `1`, `I` → `1`
fn parse_ocr_number(s: &str) -> Option<u32> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a parser with no character name
    fn parser() -> CombatLogParser {
        CombatLogParser::new(None)
    }

    /// Helper: create a parser with a character name set
    fn parser_with_name(name: &str) -> CombatLogParser {
        CombatLogParser::new(Some(name.to_string()))
    }

    // ---------------------------------------------------------------
    // Basic melee patterns
    // ---------------------------------------------------------------

    #[test]
    fn parse_melee_pierce() {
        let mut p = parser();
        let event = p
            .parse_line("Narky pierces a cultist for 7 points of damage.")
            .expect("should parse");
        assert_eq!(event.source_player, "Narky");
        assert_eq!(event.attack_name, "pierce");
        assert_eq!(event.target, "a cultist");
        assert_eq!(event.damage_value, 7);
        assert!(event.damage_element.is_none());
    }

    #[test]
    fn parse_melee_slashes() {
        let mut p = parser();
        let event = p
            .parse_line("Creature slashes a cultist for 12 points of damage.")
            .expect("should parse");
        assert_eq!(event.source_player, "Creature");
        assert_eq!(event.attack_name, "slash");
        assert_eq!(event.target, "a cultist");
        assert_eq!(event.damage_value, 12);
    }

    #[test]
    fn parse_melee_singular_point() {
        let mut p = parser();
        let event = p
            .parse_line("Creature slashes a cultist for 1 point of damage.")
            .expect("should parse singular");
        assert_eq!(event.damage_value, 1);
    }

    #[test]
    fn parse_melee_hits() {
        let mut p = parser();
        let event = p
            .parse_line("Narky hits a skeleton for 15 points of damage.")
            .expect("should parse");
        assert_eq!(event.source_player, "Narky");
        assert_eq!(event.attack_name, "hit");
        assert_eq!(event.target, "a skeleton");
        assert_eq!(event.damage_value, 15);
    }

    #[test]
    fn parse_melee_crushes() {
        let mut p = parser();
        let event = p
            .parse_line("Narky crushes a fire beetle for 20 points of damage.")
            .expect("should parse");
        assert_eq!(event.attack_name, "crush");
        assert_eq!(event.target, "a fire beetle");
    }

    #[test]
    fn parse_melee_backstabs() {
        let mut p = parser();
        let event = p
            .parse_line("Narky backstabs a cultist for 35 points of damage.")
            .expect("should parse");
        assert_eq!(event.attack_name, "backstab");
    }

    // ---------------------------------------------------------------
    // Multi-word targets
    // ---------------------------------------------------------------

    #[test]
    fn parse_multiword_target() {
        let mut p = parser();
        let event = p
            .parse_line(
                "Narky pierces a greater ancient fire beetle warrior for 7 points of damage.",
            )
            .expect("should parse multi-word target");
        assert_eq!(event.target, "a greater ancient fire beetle warrior");
        assert_eq!(event.damage_value, 7);
    }

    // ---------------------------------------------------------------
    // Spell patterns
    // ---------------------------------------------------------------

    #[test]
    fn parse_spell_with_element() {
        let mut p = parser();
        let event = p
            .parse_line("Creature's Spirit Frost hits a cultist for 6 points of Cold Damage.")
            .expect("should parse spell");
        assert_eq!(event.source_player, "Creature");
        assert_eq!(event.attack_name, "Spirit Frost");
        assert_eq!(event.target, "a cultist");
        assert_eq!(event.damage_value, 6);
        assert_eq!(event.damage_element, Some("Cold".to_string()));
    }

    #[test]
    fn parse_spell_without_element() {
        let mut p = parser();
        let event = p
            .parse_line("Narky's Fire Blast hits a skeleton for 30 points of damage.")
            .expect("should parse spell without element");
        assert_eq!(event.source_player, "Narky");
        assert_eq!(event.attack_name, "Fire Blast");
        assert_eq!(event.target, "a skeleton");
        assert_eq!(event.damage_value, 30);
        assert!(event.damage_element.is_none());
    }

    #[test]
    fn parse_spell_multiword_name() {
        let mut p = parser();
        let event = p
            .parse_line("Narky's Greater Ice Comet hits a fire beetle for 60 points of Cold Damage.")
            .expect("should parse multi-word spell");
        assert_eq!(event.attack_name, "Greater Ice Comet");
        assert_eq!(event.target, "a fire beetle");
        assert_eq!(event.damage_element, Some("Cold".to_string()));
    }

    // ---------------------------------------------------------------
    // You/Your translation
    // ---------------------------------------------------------------

    #[test]
    fn parse_you_melee_with_name() {
        let mut p = parser_with_name("Narky");
        let event = p
            .parse_line("You pierce a cultist for 7 points of damage.")
            .expect("should parse You melee");
        assert_eq!(event.source_player, "Narky");
        assert_eq!(event.attack_name, "pierce");
        assert_eq!(event.target, "a cultist");
        assert_eq!(event.damage_value, 7);
    }

    #[test]
    fn parse_your_spell_with_name() {
        let mut p = parser_with_name("Narky");
        let event = p
            .parse_line("Your Spirit Frost hits a cultist for 6 points of Cold Damage.")
            .expect("should parse Your spell");
        assert_eq!(event.source_player, "Narky");
        assert_eq!(event.attack_name, "Spirit Frost");
        assert_eq!(event.target, "a cultist");
        assert_eq!(event.damage_value, 6);
        assert_eq!(event.damage_element, Some("Cold".to_string()));
    }

    #[test]
    fn parse_you_melee_without_name() {
        let mut p = parser();
        let event = p
            .parse_line("You pierce a cultist for 7 points of damage.")
            .expect("should parse You without character_name");
        assert_eq!(event.source_player, "You");
    }

    #[test]
    fn parse_your_spell_without_name() {
        let mut p = parser();
        let event = p
            .parse_line("Your Spirit Frost hits a cultist for 6 points of Cold Damage.")
            .expect("should parse Your without character_name");
        assert_eq!(event.source_player, "You");
    }

    // ---------------------------------------------------------------
    // OCR tolerance
    // ---------------------------------------------------------------

    #[test]
    fn ocr_number_o_for_zero() {
        let mut p = parser();
        let event = p
            .parse_line("Narky pierces a cultist for 1O points of damage.")
            .expect("should parse O as 0");
        assert_eq!(event.damage_value, 10);
    }

    #[test]
    fn ocr_number_l_for_one() {
        let mut p = parser();
        let event = p
            .parse_line("Narky pierces a cultist for l2 points of damage.")
            .expect("should parse l as 1");
        assert_eq!(event.damage_value, 12);
    }

    #[test]
    fn ocr_number_capital_i_for_one() {
        let mut p = parser();
        let event = p
            .parse_line("Narky pierces a cultist for I5 points of damage.")
            .expect("should parse I as 1");
        assert_eq!(event.damage_value, 15);
    }

    #[test]
    fn ocr_polnts() {
        let mut p = parser();
        let event = p
            .parse_line("Narky pierces a cultist for 7 polnts of damage.")
            .expect("should handle polnts");
        assert_eq!(event.damage_value, 7);
    }

    #[test]
    fn ocr_p0ints() {
        let mut p = parser();
        let event = p
            .parse_line("Narky pierces a cultist for 7 p0ints of damage.")
            .expect("should handle p0ints");
        assert_eq!(event.damage_value, 7);
    }

    #[test]
    fn ocr_darnage() {
        let mut p = parser();
        let event = p
            .parse_line("Narky pierces a cultist for 7 points of darnage.")
            .expect("should handle darnage");
        assert_eq!(event.damage_value, 7);
    }

    #[test]
    fn ocr_possessive_5() {
        let mut p = parser();
        let event = p
            .parse_line("Creature\u{2019}5 Spirit Frost hits a cultist for 6 points of Cold Damage.")
            .expect("should handle '5 possessive");
        assert_eq!(event.source_player, "Creature");
        assert_eq!(event.attack_name, "Spirit Frost");
    }

    // ---------------------------------------------------------------
    // Non-damage lines (should return None)
    // ---------------------------------------------------------------

    #[test]
    fn skip_miss_line() {
        let mut p = parser();
        assert!(p
            .parse_line("Narky tries to hit a cultist, but misses!")
            .is_none());
    }

    #[test]
    fn skip_miss_pierce_line() {
        let mut p = parser();
        assert!(p
            .parse_line("Narky tries to pierce a cultist, but misses!")
            .is_none());
    }

    #[test]
    fn skip_heal_line() {
        let mut p = parser();
        assert!(p
            .parse_line("Narky's Heal heals a cultist for 50 Health.")
            .is_none());
    }

    #[test]
    fn skip_death_line() {
        let mut p = parser();
        assert!(p.parse_line("a cultist has been slain!").is_none());
    }

    #[test]
    fn skip_you_have_been_slain() {
        let mut p = parser();
        assert!(p.parse_line("You have been slain!").is_none());
    }

    #[test]
    fn skip_experience_line() {
        let mut p = parser();
        assert!(p.parse_line("You gain experience!").is_none());
    }

    #[test]
    fn skip_experience_with_amount() {
        let mut p = parser();
        assert!(p.parse_line("You gain 150 experience!").is_none());
    }

    // ---------------------------------------------------------------
    // Edge cases
    // ---------------------------------------------------------------

    #[test]
    fn empty_line_returns_none() {
        let mut p = parser();
        assert!(p.parse_line("").is_none());
    }

    #[test]
    fn whitespace_only_returns_none() {
        let mut p = parser();
        assert!(p.parse_line("   ").is_none());
    }

    #[test]
    fn partial_line_no_period() {
        let mut p = parser();
        assert!(p
            .parse_line("Narky pierces a cultist for 7 points of dam")
            .is_none());
    }

    #[test]
    fn ocr_garbage_returns_none() {
        let mut p = parser();
        assert!(p.parse_line("asdkjfh aslkdjf 2398 !!!").is_none());
    }

    #[test]
    fn random_text_no_panic() {
        let mut p = parser();
        assert!(p.parse_line("The quick brown fox jumps over the lazy dog.").is_none());
    }

    #[test]
    fn missing_for_anchor_returns_none() {
        let mut p = parser();
        assert!(p
            .parse_line("Narky pierces a cultist 7 points of damage.")
            .is_none());
    }

    // ---------------------------------------------------------------
    // ID auto-increment
    // ---------------------------------------------------------------

    #[test]
    fn ids_auto_increment() {
        let mut p = parser();
        let e1 = p
            .parse_line("Narky pierces a cultist for 7 points of damage.")
            .unwrap();
        let e2 = p
            .parse_line("Narky slashes a cultist for 10 points of damage.")
            .unwrap();
        assert_eq!(e1.id, 0);
        assert_eq!(e2.id, 1);
    }

    // ---------------------------------------------------------------
    // parse_ocr_number unit tests
    // ---------------------------------------------------------------

    #[test]
    fn test_parse_ocr_number_normal() {
        assert_eq!(parse_ocr_number("42"), Some(42));
    }

    #[test]
    fn test_parse_ocr_number_with_o() {
        assert_eq!(parse_ocr_number("1O"), Some(10));
    }

    #[test]
    fn test_parse_ocr_number_with_l() {
        assert_eq!(parse_ocr_number("l5"), Some(15));
    }

    #[test]
    fn test_parse_ocr_number_garbage() {
        assert_eq!(parse_ocr_number("abc"), None);
    }

    // ---------------------------------------------------------------
    // verb_to_base_form unit tests
    // ---------------------------------------------------------------

    #[test]
    fn test_verb_base_forms() {
        assert_eq!(verb_to_base_form("pierces"), "pierce");
        assert_eq!(verb_to_base_form("hits"), "hit");
        assert_eq!(verb_to_base_form("slashes"), "slash");
        assert_eq!(verb_to_base_form("crushes"), "crush");
        assert_eq!(verb_to_base_form("bashes"), "bash");
        assert_eq!(verb_to_base_form("kicks"), "kick");
        assert_eq!(verb_to_base_form("bites"), "bite");
        assert_eq!(verb_to_base_form("claws"), "claw");
        assert_eq!(verb_to_base_form("stings"), "sting");
        assert_eq!(verb_to_base_form("mauls"), "maul");
        assert_eq!(verb_to_base_form("strikes"), "strike");
        assert_eq!(verb_to_base_form("smashes"), "smash");
        assert_eq!(verb_to_base_form("punches"), "punch");
        assert_eq!(verb_to_base_form("backstabs"), "backstab");
    }

    // ---------------------------------------------------------------
    // set_character_name
    // ---------------------------------------------------------------

    #[test]
    fn set_character_name_updates_translation() {
        let mut p = parser();
        let e1 = p
            .parse_line("You pierce a cultist for 7 points of damage.")
            .unwrap();
        assert_eq!(e1.source_player, "You");

        p.set_character_name(Some("Narky".to_string()));
        let e2 = p
            .parse_line("You pierce a cultist for 7 points of damage.")
            .unwrap();
        assert_eq!(e2.source_player, "Narky");
    }

    // ---------------------------------------------------------------
    // Raw line preserved
    // ---------------------------------------------------------------

    #[test]
    fn raw_line_is_preserved() {
        let mut p = parser();
        let line = "Narky pierces a cultist for 7 points of damage.";
        let event = p.parse_line(line).unwrap();
        assert_eq!(event.raw_line, line);
    }

    // ---------------------------------------------------------------
    // "You" as melee verb fallback (single-word player name "You")
    // ---------------------------------------------------------------

    #[test]
    fn parse_you_with_unknown_verb() {
        let mut p = parser_with_name("Narky");
        // Fallback: "You" + unknown verb + target
        let event = p
            .parse_line("You rend a cultist for 5 points of damage.")
            .expect("should parse unknown verb via fallback");
        assert_eq!(event.source_player, "Narky");
        assert_eq!(event.attack_name, "rend");
        assert_eq!(event.target, "a cultist");
    }
}
