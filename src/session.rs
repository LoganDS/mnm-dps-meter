//! Session accumulator for the mnm-dps-meter application.
//!
//! Maintains all session state: damage events, mana events, summary statistics,
//! and vital stats. Events are append-only. Summaries always derive from the
//! event list. Thread-safe access is provided via [`SharedSession`].

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::types::{DamageEvent, ManaEventType, ManaTickEvent, SessionStatus, VitalStats};

/// All session data: events, summaries, vital stats, and control state.
///
/// Invariants (from spec):
/// - Events are append-only, never modified (invariant 1).
/// - `total_damage` always equals sum of all `damage_value` (invariant 2).
/// - Reset wipes all session data; config persists (invariant 7).
/// - Clear removes the display log; summaries are unaffected (invariant 8).
pub struct SessionState {
    /// User-configured character name for You/Your translation.
    pub character_name: Option<String>,

    /// All parsed damage events (append-only).
    pub damage_events: Vec<DamageEvent>,
    /// All detected mana tick events (append-only).
    pub mana_events: Vec<ManaTickEvent>,

    /// Running total of all damage dealt.
    pub total_damage: u64,
    /// Damage totals keyed by player name.
    pub damage_by_player: BTreeMap<String, u64>,
    /// Damage totals keyed by player name, then by attack name.
    pub damage_by_player_attack: BTreeMap<String, BTreeMap<String, u64>>,

    /// Last known current mana from panel OCR.
    pub current_mana: Option<u32>,
    /// Last known max mana from panel OCR.
    pub max_mana: Option<u32>,
    /// Last known current HP from panel OCR.
    pub current_hp: Option<u32>,
    /// Last known max HP from panel OCR.
    pub max_hp: Option<u32>,
    /// Last known current endurance from panel OCR.
    pub current_endurance: Option<u32>,
    /// Last known max endurance from panel OCR.
    pub max_endurance: Option<u32>,

    /// Count of positive-delta mana events (regen ticks).
    pub mana_ticks_count: u32,
    /// Sum of all positive mana deltas.
    pub mana_regen_total: u32,
    /// Sum of all negative mana deltas (stored as absolute value).
    pub mana_spend_total: u32,

    /// Current session status (Running or Paused).
    pub status: SessionStatus,
    /// When the session was started (or last reset).
    pub started_at: Instant,

    /// Index into `damage_events` marking the start of visible display.
    /// Used by `clear_display` to hide old events without deleting them.
    display_offset: usize,
}

impl SessionState {
    /// Create a new empty session.
    pub fn new() -> Self {
        Self {
            character_name: None,
            damage_events: Vec::new(),
            mana_events: Vec::new(),
            total_damage: 0,
            damage_by_player: BTreeMap::new(),
            damage_by_player_attack: BTreeMap::new(),
            current_mana: None,
            max_mana: None,
            current_hp: None,
            max_hp: None,
            current_endurance: None,
            max_endurance: None,
            mana_ticks_count: 0,
            mana_regen_total: 0,
            mana_spend_total: 0,
            status: SessionStatus::Running,
            started_at: Instant::now(),
            display_offset: 0,
        }
    }

    /// Append a damage event and update all summary maps.
    ///
    /// The event is stored permanently. `total_damage`, `damage_by_player`,
    /// and `damage_by_player_attack` are updated to stay consistent.
    pub fn add_damage_event(&mut self, event: DamageEvent) {
        let dmg = event.damage_value as u64;
        self.total_damage += dmg;

        *self
            .damage_by_player
            .entry(event.source_player.clone())
            .or_insert(0) += dmg;

        *self
            .damage_by_player_attack
            .entry(event.source_player.clone())
            .or_default()
            .entry(event.attack_name.clone())
            .or_insert(0) += dmg;

        self.damage_events.push(event);
    }

    /// Append a mana tick event and update mana summary statistics.
    pub fn add_mana_tick(&mut self, event: ManaTickEvent) {
        match event.event_type {
            ManaEventType::Regen => {
                self.mana_ticks_count += 1;
                self.mana_regen_total += event.delta as u32;
            }
            ManaEventType::Spend => {
                self.mana_spend_total += event.delta.unsigned_abs();
            }
        }
        self.mana_events.push(event);
    }

    /// Update vital stats from the mini character panel OCR.
    pub fn update_vitals(&mut self, stats: VitalStats) {
        if let Some(v) = stats.hp_current {
            self.current_hp = Some(v);
        }
        if let Some(v) = stats.hp_max {
            self.max_hp = Some(v);
        }
        if let Some(v) = stats.mana_current {
            self.current_mana = Some(v);
        }
        if let Some(v) = stats.mana_max {
            self.max_mana = Some(v);
        }
        if let Some(v) = stats.endurance_current {
            self.current_endurance = Some(v);
        }
        if let Some(v) = stats.endurance_max {
            self.max_endurance = Some(v);
        }
    }

    /// Average mana regen per tick. Returns 0.0 if no ticks recorded.
    pub fn mana_avg_per_tick(&self) -> f32 {
        if self.mana_ticks_count == 0 {
            return 0.0;
        }
        self.mana_regen_total as f32 / self.mana_ticks_count as f32
    }

    /// Clear the event display log without affecting summaries (invariant 8).
    ///
    /// After this call, [`visible_damage_events`] returns an empty slice,
    /// but `total_damage`, `damage_by_player`, etc. are unchanged.
    pub fn clear_display(&mut self) {
        self.display_offset = self.damage_events.len();
    }

    /// Return the damage events visible in the display (post-clear offset).
    pub fn visible_damage_events(&self) -> &[DamageEvent] {
        &self.damage_events[self.display_offset..]
    }

    /// Reset all session data (invariant 7).
    ///
    /// Wipes events, summaries, mana history, and vitals. Config fields
    /// (character_name) are preserved — they come from AppConfig, not session.
    pub fn reset(&mut self) {
        self.damage_events.clear();
        self.mana_events.clear();
        self.total_damage = 0;
        self.damage_by_player.clear();
        self.damage_by_player_attack.clear();
        self.current_mana = None;
        self.max_mana = None;
        self.current_hp = None;
        self.max_hp = None;
        self.current_endurance = None;
        self.max_endurance = None;
        self.mana_ticks_count = 0;
        self.mana_regen_total = 0;
        self.mana_spend_total = 0;
        self.started_at = Instant::now();
        self.display_offset = 0;
    }

    /// Pause the session. Capture threads should check this status.
    pub fn pause(&mut self) {
        self.status = SessionStatus::Paused;
    }

    /// Resume the session from a paused state.
    pub fn resume(&mut self) {
        self.status = SessionStatus::Running;
    }

    /// Return player damage totals sorted descending by damage for display.
    pub fn damage_by_player_sorted(&self) -> Vec<(String, u64)> {
        let mut entries: Vec<_> = self
            .damage_by_player
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries
    }

    /// Return per-attack damage for a given player, sorted descending.
    pub fn attacks_for_player_sorted(&self, player: &str) -> Vec<(String, u64)> {
        let Some(attacks) = self.damage_by_player_attack.get(player) else {
            return Vec::new();
        };
        let mut entries: Vec<_> = attacks.iter().map(|(k, v)| (k.clone(), *v)).collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries
    }
}

/// Thread-safe shared session state for access from UI + pipeline threads.
pub type SharedSession = Arc<Mutex<SessionState>>;

/// Create a new shared session wrapped in `Arc<Mutex>`.
pub fn new_shared_session() -> SharedSession {
    Arc::new(Mutex::new(SessionState::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Helper: create a DamageEvent with the given fields.
    fn make_damage_event(
        id: u64,
        source: &str,
        attack: &str,
        target: &str,
        damage: u32,
        element: Option<&str>,
    ) -> DamageEvent {
        DamageEvent {
            id,
            timestamp: Instant::now(),
            source_player: source.to_string(),
            attack_name: attack.to_string(),
            target: target.to_string(),
            damage_value: damage,
            damage_element: element.map(|s| s.to_string()),
            raw_line: format!("{source} {attack} {target} for {damage} points of damage."),
        }
    }

    /// Helper: create a ManaTickEvent.
    fn make_mana_tick(
        id: u64,
        prev: u32,
        curr: u32,
        max: u32,
        event_type: ManaEventType,
    ) -> ManaTickEvent {
        ManaTickEvent {
            id,
            timestamp: Instant::now(),
            previous_mana: prev,
            current_mana: curr,
            max_mana: max,
            delta: curr as i32 - prev as i32,
            event_type,
        }
    }

    #[test]
    fn test_new_session_is_empty() {
        let session = SessionState::new();
        assert_eq!(session.total_damage, 0);
        assert!(session.damage_events.is_empty());
        assert!(session.mana_events.is_empty());
        assert!(session.damage_by_player.is_empty());
        assert!(session.damage_by_player_attack.is_empty());
        assert_eq!(session.mana_ticks_count, 0);
        assert_eq!(session.mana_regen_total, 0);
        assert_eq!(session.mana_spend_total, 0);
        assert_eq!(session.status, SessionStatus::Running);
        assert_eq!(session.current_mana, None);
        assert_eq!(session.max_mana, None);
    }

    #[test]
    fn test_add_damage_event_updates_summaries() {
        let mut session = SessionState::new();

        session.add_damage_event(make_damage_event(
            1, "Narky", "pierce", "a cultist", 7, None,
        ));
        session.add_damage_event(make_damage_event(
            2, "Narky", "slash", "a cultist", 12, None,
        ));
        session.add_damage_event(make_damage_event(
            3, "Bork", "crush", "a cultist", 20, None,
        ));

        assert_eq!(session.damage_events.len(), 3);
        assert_eq!(session.total_damage, 39);
        assert_eq!(*session.damage_by_player.get("Narky").unwrap(), 19);
        assert_eq!(*session.damage_by_player.get("Bork").unwrap(), 20);

        let narky_attacks = session.damage_by_player_attack.get("Narky").unwrap();
        assert_eq!(*narky_attacks.get("pierce").unwrap(), 7);
        assert_eq!(*narky_attacks.get("slash").unwrap(), 12);
    }

    #[test]
    fn test_total_damage_invariant() {
        let mut session = SessionState::new();

        for i in 0..100 {
            session.add_damage_event(make_damage_event(
                i,
                &format!("Player{}", i % 5),
                "hit",
                "a mob",
                (i as u32 + 1) * 3,
                None,
            ));
        }

        // Invariant 2: total_damage == sum of all damage_value
        let sum: u64 = session
            .damage_events
            .iter()
            .map(|e| e.damage_value as u64)
            .sum();
        assert_eq!(session.total_damage, sum);

        // Also verify per-player sums
        let player_sum: u64 = session.damage_by_player.values().sum();
        assert_eq!(session.total_damage, player_sum);
    }

    #[test]
    fn test_damage_by_player_sorted_descending() {
        let mut session = SessionState::new();

        session.add_damage_event(make_damage_event(1, "Alice", "hit", "mob", 10, None));
        session.add_damage_event(make_damage_event(2, "Bob", "hit", "mob", 30, None));
        session.add_damage_event(make_damage_event(3, "Carol", "hit", "mob", 20, None));

        let sorted = session.damage_by_player_sorted();
        assert_eq!(sorted[0], ("Bob".to_string(), 30));
        assert_eq!(sorted[1], ("Carol".to_string(), 20));
        assert_eq!(sorted[2], ("Alice".to_string(), 10));
    }

    #[test]
    fn test_attacks_for_player_sorted() {
        let mut session = SessionState::new();

        session.add_damage_event(make_damage_event(1, "Narky", "pierce", "mob", 5, None));
        session.add_damage_event(make_damage_event(2, "Narky", "slash", "mob", 15, None));
        session.add_damage_event(make_damage_event(3, "Narky", "pierce", "mob", 8, None));

        let attacks = session.attacks_for_player_sorted("Narky");
        assert_eq!(attacks[0], ("slash".to_string(), 15));
        assert_eq!(attacks[1], ("pierce".to_string(), 13));

        // Non-existent player returns empty
        assert!(session.attacks_for_player_sorted("Nobody").is_empty());
    }

    #[test]
    fn test_add_mana_tick_regen() {
        let mut session = SessionState::new();

        session.add_mana_tick(make_mana_tick(1, 100, 120, 470, ManaEventType::Regen));
        session.add_mana_tick(make_mana_tick(2, 120, 140, 470, ManaEventType::Regen));

        assert_eq!(session.mana_events.len(), 2);
        assert_eq!(session.mana_ticks_count, 2);
        assert_eq!(session.mana_regen_total, 40);
        assert_eq!(session.mana_spend_total, 0);
    }

    #[test]
    fn test_add_mana_tick_spend() {
        let mut session = SessionState::new();

        session.add_mana_tick(make_mana_tick(1, 200, 150, 470, ManaEventType::Spend));

        assert_eq!(session.mana_ticks_count, 0);
        assert_eq!(session.mana_regen_total, 0);
        assert_eq!(session.mana_spend_total, 50);
    }

    #[test]
    fn test_mana_mixed_regen_and_spend() {
        let mut session = SessionState::new();

        session.add_mana_tick(make_mana_tick(1, 100, 120, 470, ManaEventType::Regen));
        session.add_mana_tick(make_mana_tick(2, 120, 50, 470, ManaEventType::Spend));
        session.add_mana_tick(make_mana_tick(3, 50, 70, 470, ManaEventType::Regen));

        assert_eq!(session.mana_ticks_count, 2);
        assert_eq!(session.mana_regen_total, 40); // 20 + 20
        assert_eq!(session.mana_spend_total, 70);
        assert!((session.mana_avg_per_tick() - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_mana_avg_per_tick_no_ticks() {
        let session = SessionState::new();
        assert_eq!(session.mana_avg_per_tick(), 0.0);
    }

    #[test]
    fn test_update_vitals() {
        let mut session = SessionState::new();

        session.update_vitals(VitalStats {
            hp_current: Some(442),
            hp_max: Some(454),
            mana_current: Some(185),
            mana_max: Some(470),
            endurance_current: None,
            endurance_max: None,
        });

        assert_eq!(session.current_hp, Some(442));
        assert_eq!(session.max_hp, Some(454));
        assert_eq!(session.current_mana, Some(185));
        assert_eq!(session.max_mana, Some(470));
        assert_eq!(session.current_endurance, None);
        assert_eq!(session.max_endurance, None);

        // Partial update: only mana changes, HP preserved
        session.update_vitals(VitalStats {
            hp_current: None,
            hp_max: None,
            mana_current: Some(200),
            mana_max: None,
            endurance_current: Some(80),
            endurance_max: Some(100),
        });

        assert_eq!(session.current_hp, Some(442)); // preserved
        assert_eq!(session.current_mana, Some(200)); // updated
        assert_eq!(session.max_mana, Some(470)); // preserved
        assert_eq!(session.current_endurance, Some(80));
        assert_eq!(session.max_endurance, Some(100));
    }

    #[test]
    fn test_clear_display_preserves_summaries() {
        let mut session = SessionState::new();

        session.add_damage_event(make_damage_event(1, "Narky", "pierce", "mob", 10, None));
        session.add_damage_event(make_damage_event(2, "Narky", "slash", "mob", 20, None));

        // Before clear: 2 visible events
        assert_eq!(session.visible_damage_events().len(), 2);

        session.clear_display();

        // After clear: 0 visible events
        assert!(session.visible_damage_events().is_empty());

        // But summaries unchanged (invariant 8)
        assert_eq!(session.total_damage, 30);
        assert_eq!(*session.damage_by_player.get("Narky").unwrap(), 30);
        assert_eq!(session.damage_events.len(), 2); // underlying data preserved

        // New events after clear are visible
        session.add_damage_event(make_damage_event(3, "Narky", "hit", "mob", 5, None));
        assert_eq!(session.visible_damage_events().len(), 1);
        assert_eq!(session.total_damage, 35);
    }

    #[test]
    fn test_reset_wipes_everything() {
        let mut session = SessionState::new();
        session.character_name = Some("Narky".to_string());

        session.add_damage_event(make_damage_event(1, "Narky", "pierce", "mob", 10, None));
        session.add_mana_tick(make_mana_tick(1, 100, 120, 470, ManaEventType::Regen));
        session.update_vitals(VitalStats {
            hp_current: Some(400),
            hp_max: Some(450),
            mana_current: Some(200),
            mana_max: Some(470),
            endurance_current: Some(80),
            endurance_max: Some(100),
        });

        session.reset();

        // All session data wiped (invariant 7)
        assert!(session.damage_events.is_empty());
        assert!(session.mana_events.is_empty());
        assert_eq!(session.total_damage, 0);
        assert!(session.damage_by_player.is_empty());
        assert!(session.damage_by_player_attack.is_empty());
        assert_eq!(session.current_mana, None);
        assert_eq!(session.max_mana, None);
        assert_eq!(session.current_hp, None);
        assert_eq!(session.max_hp, None);
        assert_eq!(session.current_endurance, None);
        assert_eq!(session.max_endurance, None);
        assert_eq!(session.mana_ticks_count, 0);
        assert_eq!(session.mana_regen_total, 0);
        assert_eq!(session.mana_spend_total, 0);
        assert!(session.visible_damage_events().is_empty());

        // character_name is preserved — it's config, not session data
        assert_eq!(session.character_name, Some("Narky".to_string()));
    }

    #[test]
    fn test_pause_resume() {
        let mut session = SessionState::new();

        assert_eq!(session.status, SessionStatus::Running);
        session.pause();
        assert_eq!(session.status, SessionStatus::Paused);
        session.resume();
        assert_eq!(session.status, SessionStatus::Running);
    }

    #[test]
    fn test_elemental_damage_tracking() {
        let mut session = SessionState::new();

        session.add_damage_event(make_damage_event(
            1,
            "Creature",
            "Spirit Frost",
            "a cultist",
            6,
            Some("Cold"),
        ));
        session.add_damage_event(make_damage_event(
            2,
            "Creature",
            "Spirit Frost",
            "a cultist",
            8,
            Some("Cold"),
        ));
        session.add_damage_event(make_damage_event(
            3, "Narky", "pierce", "a cultist", 7, None,
        ));

        assert_eq!(session.total_damage, 21);
        assert_eq!(*session.damage_by_player.get("Creature").unwrap(), 14);
        assert_eq!(*session.damage_by_player.get("Narky").unwrap(), 7);

        let creature_attacks = session.attacks_for_player_sorted("Creature");
        assert_eq!(creature_attacks.len(), 1);
        assert_eq!(creature_attacks[0], ("Spirit Frost".to_string(), 14));
    }

    #[test]
    fn test_shared_session_thread_safety() {
        let shared = new_shared_session();

        // Verify we can lock and use from the current thread
        {
            let mut session = shared.lock().unwrap();
            session.add_damage_event(make_damage_event(
                1, "Narky", "pierce", "a cultist", 7, None,
            ));
        }

        // Verify data persists across lock/unlock cycles
        {
            let session = shared.lock().unwrap();
            assert_eq!(session.total_damage, 7);
            assert_eq!(session.damage_events.len(), 1);
        }
    }

    #[test]
    fn test_shared_session_across_threads() {
        let shared = new_shared_session();
        let shared_clone = Arc::clone(&shared);

        let handle = std::thread::spawn(move || {
            let mut session = shared_clone.lock().unwrap();
            session.add_damage_event(make_damage_event(
                1, "ThreadPlayer", "hit", "mob", 42, None,
            ));
        });

        handle.join().unwrap();

        let session = shared.lock().unwrap();
        assert_eq!(session.total_damage, 42);
        assert_eq!(session.damage_events[0].source_player, "ThreadPlayer");
    }

    #[test]
    fn test_clear_then_reset() {
        let mut session = SessionState::new();

        session.add_damage_event(make_damage_event(1, "A", "hit", "mob", 10, None));
        session.clear_display();
        assert!(session.visible_damage_events().is_empty());
        assert_eq!(session.total_damage, 10);

        session.reset();
        assert!(session.visible_damage_events().is_empty());
        assert_eq!(session.total_damage, 0);
        assert!(session.damage_events.is_empty());
    }

    #[test]
    fn test_many_players_summary_consistency() {
        let mut session = SessionState::new();
        let players = ["Alice", "Bob", "Carol", "Dave", "Eve"];

        for (i, &player) in players.iter().enumerate() {
            for j in 0..10 {
                let id = (i * 10 + j) as u64;
                let dmg = ((i + 1) * (j + 1)) as u32;
                session.add_damage_event(make_damage_event(id, player, "hit", "mob", dmg, None));
            }
        }

        // Verify invariant 2: total equals sum of all events
        let event_sum: u64 = session
            .damage_events
            .iter()
            .map(|e| e.damage_value as u64)
            .sum();
        assert_eq!(session.total_damage, event_sum);

        // Verify per-player sums match
        let player_sum: u64 = session.damage_by_player.values().sum();
        assert_eq!(session.total_damage, player_sum);

        // Verify per-attack sums match per-player sums
        for (player, &player_dmg) in &session.damage_by_player {
            let attack_sum: u64 = session
                .damage_by_player_attack
                .get(player)
                .unwrap()
                .values()
                .sum();
            assert_eq!(player_dmg, attack_sum);
        }
    }
}
