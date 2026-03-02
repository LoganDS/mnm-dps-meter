# Damage Meter (OCR) — System Specification
# Target Game: Monsters & Memories

## Metadata

```yaml
id: damage-meter-ocr
version: 0.2.0
status: draft
author: human + claude (initial session)
dependencies: []
last_refined: 2026-03-02
```

## Overview

### Purpose
Damage Meter (OCR) is a lightweight, high-performance Rust desktop application that reads on-screen combat text and mana bar state from the Monsters & Memories game client using optical character recognition. It parses individual damage events and mana regeneration ticks, then presents real-time and cumulative statistics. The system produces a live-updating damage breakdown by player and by attack type, plus mana tick tracking aligned to the game's 6-second server tick cycle — all suitable for group performance analysis during gameplay sessions.

### Scope

**Included:**
- User-driven, resettable screen region selection for the combat log window (drag-to-select overlay)
- User-driven, resettable screen region selection for the mana bar
- OCR of in-game combat log text from the user-defined region
- Pixel-level mana bar monitoring on the game's 6-second tick cycle
- Parsing of combat log lines into structured damage events (player, attack, target, damage value)
- Detection and recording of mana tick amounts (mana gained per tick)
- Real-time display of a scrolling combat event log
- Cumulative summary statistics: total damage, damage per player, damage per player per attack
- Mana tick summary: ticks observed, average mana per tick, total mana regenerated
- Session controls: pause, resume, clear (log only), and reset (full state wipe)
- Cross-platform support: Windows, macOS, Linux — compiled natively, no Docker, no container runtime

**Excluded (for now):**
- Healing, buff, or debuff tracking
- DPS (damage per second) calculation with time-series graphing
- Damage-taken / tanking metrics
- Overlay mode (transparent always-on-top HUD composited over the game client)
- Multi-encounter / boss-fight segmentation
- Export to file (CSV, JSON, clipboard)
- Direct log file parsing (M&M does not expose a parseable combat log file)
- Health bar tracking
- Automatic game window detection (user must manually select regions)

### Context
Damage Meter (OCR) is built for Monsters & Memories, a classic-era MMORPG currently in development by Niche Worlds Cult (Early Access scheduled June 2026). M&M features an EverQuest-inspired combat system with fully customizable UI — players can move, resize, and filter all UI windows including the combat log, chat, health/mana bars, and target frames. Because the UI layout varies per player, the damage meter must support user-driven region selection that can be reconfigured at any time.

M&M uses a server tick system (approximately 6 seconds) for mana and health regeneration. Mana regeneration rate depends on the Meditation skill (1 mana per tick per 10 skill, so 100 Meditation = +10 mana/tick), sitting status, campfire/hearth proximity, and active buffs. Tracking mana ticks is valuable for casters to understand their effective regen rate during a session.

This is a standalone tool built using the docs-first atomic pipeline architecture. Each component described below should decompose into one or more atomic tasks, each producing a self-contained micro-app or module.

---

## Interface Contracts

### Inbound: Combat Log Screen Capture / OCR

```yaml
combat_log_capture_interface:
  description: >
    Captures a user-defined rectangular region of the screen (the M&M
    combat log window) at a configurable interval and performs OCR to
    extract text lines. The region is set by the user via a visual
    drag-to-select overlay and can be reconfigured at any time.
  capture_contract:
    input:
      - name: region
        type: rectangle (x, y, width, height)
        constraints: >
          must be within screen bounds. Set by user via drag-select
          overlay. Persisted to config file. Resettable at any time
          via a "Set Combat Log Region" button.
      - name: capture_interval_ms
        type: integer
        constraints: ">= 50, default 250"
        description: >
          how often to capture and OCR the combat log region.
          Lower values increase responsiveness but CPU usage.
          250ms is aggressive enough to catch most combat text
          before it scrolls off in active combat.
    output:
      - name: text_lines
        type: array of string
        guarantees: >
          One string per detected line of text in the captured region.
          Lines are ordered top-to-bottom as they appear on screen.
          OCR errors are possible — downstream parser must be tolerant.
    error_conditions:
      - condition: capture_region_offscreen
        behavior: >
          return error, pause capture, alert user to reconfigure.
          This can happen if the user moves the game window or changes
          resolution.
      - condition: ocr_failure
        behavior: return empty array, log warning, continue on next interval
      - condition: no_new_text
        behavior: return empty array (no change detected), skip processing
```

### Inbound: Mana Bar Screen Capture

```yaml
mana_bar_capture_interface:
  description: >
    Captures a user-defined rectangular region covering the player's
    mana bar. Unlike the combat log OCR, this does NOT use text
    recognition. Instead, it samples the pixel color/fill level of
    the mana bar to detect changes in mana amount. The mana bar in
    M&M is a horizontal fill bar (typically blue). Monitoring its
    fill percentage at intervals aligned with the 6-second server
    tick detects mana regeneration events.
  capture_contract:
    input:
      - name: region
        type: rectangle (x, y, width, height)
        constraints: >
          must be within screen bounds. Set by user via drag-select
          overlay. Persisted to config file. Resettable at any time
          via a "Set Mana Bar Region" button.
      - name: sample_interval_ms
        type: integer
        constraints: ">= 100, default 500"
        description: >
          how often to sample the mana bar. We sample faster than
          the 6-second tick to catch the exact moment of change.
          The tick detector smooths over multiple samples.
      - name: bar_color
        type: color range (hue, saturation, value ranges)
        constraints: "configurable, default blue range for M&M mana bar"
        description: >
          the color range that identifies the "filled" portion of
          the mana bar. Used to calculate fill percentage.
    output:
      - name: fill_percentage
        type: float (0.0 to 1.0)
        guarantees: >
          approximate fill level of the mana bar based on the
          ratio of bar-colored pixels to total pixels in the region.
          Precision depends on bar width in pixels.
    error_conditions:
      - condition: capture_region_offscreen
        behavior: return error, pause mana tracking, alert user
      - condition: no_bar_detected
        behavior: >
          if the sampled region contains no pixels matching the
          bar color range, assume the bar is empty (0.0) or
          the region is misconfigured. Log warning.
```

### Internal: Damage Event Schema

```yaml
damage_event:
  description: >
    The normalized internal representation of a single parsed damage
    event from the Monsters & Memories combat log.
  fields:
    - name: id
      type: u64 (auto-incremented)
      constraints: unique per session
    - name: timestamp
      type: Instant (monotonic clock)
      constraints: not null, time the event was captured
    - name: source_player
      type: String
      constraints: not null
      description: >
        the player or entity dealing damage. In M&M combat text,
        "you" refers to the local player. Other group members appear
        by name. Examples from screenshot: "you", "santa", "holkan",
        "demma", "your" (possessive for pet/proc damage).
    - name: attack_name
      type: String
      constraints: not null
      description: >
        the name of the attack or ability used. May include possessive
        forms for abilities (e.g., "bash's", "slam's", "pierce's").
        Also includes spell names (e.g., "fire blast", "ice comet").
    - name: target
      type: String
      constraints: not null
      description: >
        the entity receiving damage. In M&M these are typically
        formatted with articles (e.g., "a fire beetle", "a skeleton",
        "a ghoul").
    - name: damage_value
      type: u32
      constraints: not null, >= 0
    - name: raw_line
      type: String
      constraints: not null
      description: the original OCR text line for debugging/audit
```

### Internal: Mana Tick Event Schema

```yaml
mana_tick_event:
  description: >
    A detected mana regeneration tick based on mana bar fill change.
  fields:
    - name: id
      type: u64 (auto-incremented)
      constraints: unique per session
    - name: timestamp
      type: Instant (monotonic clock)
      constraints: not null
    - name: previous_fill
      type: f32 (0.0 to 1.0)
      constraints: not null
      description: mana bar fill percentage before the tick
    - name: current_fill
      type: f32 (0.0 to 1.0)
      constraints: not null
      description: mana bar fill percentage after the tick
    - name: delta_percent
      type: f32
      constraints: not null
      description: >
        change in fill percentage (current - previous). Positive
        values indicate mana gain (regen). Negative values indicate
        mana expenditure (spell cast). Only positive deltas are
        counted as mana ticks.
    - name: estimated_tick_amount
      type: Option<u32>
      description: >
        if the user has configured their max mana pool, this
        is the estimated mana gained in absolute terms
        (delta_percent * max_mana). Otherwise null.
```

### Internal: Session State

```yaml
session_state:
  description: >
    The cumulative state of the current damage and mana tracking
    session. Derived entirely from the ordered lists of events.
  fields:
    - name: damage_events
      type: Vec<DamageEvent>
      description: all parsed damage events in chronological order
    - name: mana_tick_events
      type: Vec<ManaTick>
      description: all detected mana ticks in chronological order
    - name: total_damage
      type: u64
      description: sum of all damage_value across all damage events
    - name: damage_by_player
      type: BTreeMap<String, u64>
      description: total damage per source_player, sorted descending by value
    - name: damage_by_player_attack
      type: BTreeMap<String, BTreeMap<String, u64>>
      description: >
        per source_player, total damage per attack_name,
        sorted descending by value within each player
    - name: mana_ticks_total
      type: u32
      description: count of detected positive mana ticks
    - name: mana_regen_total_percent
      type: f32
      description: sum of all positive delta_percent values
    - name: mana_avg_tick_percent
      type: f32
      description: average delta_percent per mana tick
    - name: status
      type: enum
      constraints: "Running | Paused"
    - name: started_at
      type: Instant
      description: when the session began or was last reset
```

### Outbound: User Interface

```yaml
ui_interface:
  description: >
    Desktop window with two main panels, a control bar, and a mana
    tick indicator. Built with a Rust-native GUI framework.
  layout:
    title_bar:
      text: "Damage Meter (OCR) - v{version}"
      status_indicator: "Running" or "Paused." shown next to title
    control_bar:
      position: top
      buttons:
        - name: Pause
          action: pause all capture (combat log OCR + mana bar), freeze event log
        - name: Resume
          action: resume all capture
        - name: Clear
          action: clear the event log display (top panel) but retain summary stats
        - name: Reset
          action: wipe all state — events, summaries, counters, mana ticks — back to zero
      region_buttons:
        - name: Set Combat Log Region
          action: >
            opens a translucent fullscreen overlay. User drags a rectangle
            over the M&M combat log window. Coordinates are saved to config.
            Capture restarts with the new region.
        - name: Set Mana Bar Region
          action: >
            opens a translucent fullscreen overlay. User drags a rectangle
            over the M&M mana bar. Coordinates are saved to config.
            Mana tracking restarts with the new region.
    top_panel:
      content: scrolling event log
      format: "source_player | attack_name | target | damage_value"
      behavior: >
        New events append at the bottom. Panel scrolls to show latest.
        Each line is pipe-delimited for readability.
    bottom_panel:
      content: cumulative summary statistics
      sections:
        - header: "TOTAL DAMAGE: {total_damage}"
        - divider: "---"
        - header: "Damage by player:"
          content: >
            indented list of "player: total" sorted descending by damage
        - header: "Damage by player -> attack:"
          content: >
            for each player (sorted descending by total damage):
              player_name:
                attack_name: total_damage_for_that_attack
        - divider: "---"
        - header: "Mana Ticks:"
          content: >
            ticks observed: {count}
            avg per tick: {avg_percent}% ({estimated_amount} mana if max_mana configured)
            total regen: {total_percent}% ({estimated_total} mana if max_mana configured)
```

---

## Invariants

These must always hold true. They become assertion targets and verification criteria.

1. **Event immutability.** Once a damage event or mana tick event is added to the session, it is never modified. Events are append-only within a session.
2. **Summary consistency.** `total_damage` always equals the sum of all `damage_value` in `damage_events`. `damage_by_player[p]` always equals the sum of `damage_value` for all events where `source_player == p`. Same for `damage_by_player_attack`. Mana summary stats always derive from `mana_tick_events`.
3. **No duplicate damage events.** The same OCR text line is not parsed into multiple events. Deduplication accounts for the same text remaining on screen across multiple capture intervals.
4. **Mana tick alignment.** Mana tick events are only emitted when a positive fill change is detected. The detector must debounce rapid fluctuations and only register a tick when the bar fill increases and stabilizes (within a tolerance window). Mana expenditure (negative delta from casting) is tracked separately and not counted as a tick.
5. **Pause halts all capture.** When status is `Paused`, no screen captures, OCR operations, or mana bar samples occur. The UI remains interactive (buttons work, panels are scrollable).
6. **Reset is total.** After a reset, the session state is identical to a fresh application launch. No residual data from the prior session. Region configurations are NOT reset (they persist across sessions).
7. **Clear preserves state.** Clear only affects the visible event log in the top panel. Summary statistics, mana tick data, and the underlying event lists are unaffected.
8. **OCR tolerance.** The parser must not crash on malformed or partially-recognized OCR text. Unparseable lines are silently dropped (optionally logged at debug level).
9. **Config is external and persistent.** Capture region coordinates, capture interval, mana bar color range, and max mana pool setting are stored in a config file (TOML or JSON). Config survives application restarts. Region selection via the overlay updates the config file.
10. **No runtime LLM dependency.** The application is fully deterministic compiled software. No inference, no model loading, no network calls to AI services at runtime. OCR is performed by a traditional OCR engine compiled into or linked by the binary.
11. **Single binary distribution.** The application ships as a single compiled binary per platform (Windows .exe, macOS binary, Linux binary). No Docker, no container runtime, no JVM, no interpreter required.

---

## Acceptance Criteria

Each criterion maps to a verification step.

### Region Selection
- [ ] The user can define the combat log capture region via a visual drag-select overlay
- [ ] The user can define the mana bar capture region via a separate drag-select overlay
- [ ] Region coordinates are persisted to a config file and survive application restarts
- [ ] The user can reconfigure either region at any time via dedicated buttons
- [ ] The overlay is translucent and shows the selected rectangle with a visible border
- [ ] If the game window moves or resolution changes, the user is alerted to reconfigure

### OCR Capture & Parsing
- [ ] The application captures the combat log region at a configurable interval (default 250ms)
- [ ] Captured images are processed through OCR to extract text lines
- [ ] The system detects when no new text has appeared and skips redundant processing
- [ ] Each combat log line matching the M&M damage format is parsed into a structured event
- [ ] The parser correctly extracts: source player, attack name, target name, and damage value
- [ ] Lines that do not match the expected format are discarded without error
- [ ] Duplicate lines (same text across consecutive captures) are deduplicated
- [ ] OCR errors do not crash the application

### Mana Bar Tracking
- [ ] The application samples the mana bar region at a configurable interval (default 500ms)
- [ ] Mana bar fill percentage is calculated from the ratio of bar-colored pixels
- [ ] Positive fill changes (mana regen) are detected and recorded as mana tick events
- [ ] Negative fill changes (mana expenditure from casting) are tracked but not counted as ticks
- [ ] Rapid fluctuations are debounced to avoid false tick detection
- [ ] If the user configures their max mana pool, estimated absolute mana values are shown
- [ ] Mana tracking is independent of combat log OCR (both run concurrently)

### Display
- [ ] The top panel shows a scrolling log of parsed damage events in pipe-delimited format
- [ ] The bottom panel shows total damage, per-player totals, and per-player-per-attack breakdowns
- [ ] The bottom panel shows mana tick statistics
- [ ] The display updates in real-time as new events are captured
- [ ] Summary statistics are always mathematically consistent with the underlying event data

### Controls
- [ ] Pause stops all capture activity (OCR and mana bar); the UI remains responsive
- [ ] Resume restarts all capture from the current screen state
- [ ] Clear empties the event log display but preserves all summary statistics
- [ ] Reset wipes all session data but preserves region configuration

### Build & Distribution
- [ ] The application compiles to a single native binary on Windows, macOS, and Linux
- [ ] No Docker, container runtime, JVM, or interpreter is required to run
- [ ] The binary starts in under 2 seconds on modest hardware
- [ ] CPU usage stays below 5% during idle (no combat text on screen)
- [ ] CPU usage stays below 15% during active combat with rapid text scrolling

---

## Design Decisions

### 1. Rust as Implementation Language
- **Decision:** Build the entire application in Rust.
- **Rationale:** Rust produces small, fast, single-binary executables with no runtime dependency. Cross-compilation to Windows, macOS, and Linux is well-supported. The performance characteristics (zero-cost abstractions, no GC pauses) are ideal for a real-time screen capture and OCR pipeline that must stay below 15% CPU. The ownership model prevents the memory leaks common in long-running desktop applications.
- **Alternatives considered:** Python + tkinter (fast to build but requires Python runtime, poor performance for OCR loop), C# + WinForms (Windows-only), Go (viable but weaker GUI ecosystem, GC pauses could affect capture timing).
- **Reversibility:** Difficult. Language choice is foundational. However, the modular architecture means individual components (parser, deduplicator) are logic-only and could be ported independently.

### 2. No Docker, No Containers
- **Decision:** Distribute as native compiled binaries only. No Docker images, no container runtime.
- **Rationale:** The target users are MMO gamers, not developers. Requiring Docker to run a damage meter is a non-starter for adoption. A single .exe (Windows) or binary (macOS/Linux) that runs on double-click is the expected UX. Docker also adds unnecessary overhead for a desktop GUI application that needs direct screen access.
- **Alternatives considered:** Docker with X11/Wayland forwarding (complex, poor performance), AppImage/Flatpak for Linux (reasonable for Linux distribution, can be added later).
- **Reversibility:** Easy. Containerization can always be added as an optional distribution method.

### 3. OCR-Based Input (Necessity, Not Choice)
- **Decision:** Use screen capture + OCR to read combat text.
- **Rationale:** Monsters & Memories does not expose a parseable combat log file. The game's chat system supports filters (CombatHitOther, CombatMissPet, etc.) that can isolate damage text into a dedicated chat window, but the only way to read this externally is screen capture. OCR is the sole viable approach.
- **Alternatives considered:** Memory reading (fragile, violates ToS), network packet inspection (encrypted Unity networking), log file tailing (no log file exists).
- **Reversibility:** Easy. If M&M ever adds a log file or API, a new input module replaces the OCR pipeline. The parser accepts text lines regardless of source.

### 4. User-Driven Dynamic Region Selection
- **Decision:** The user defines capture regions via a visual drag-select overlay, resettable at any time.
- **Rationale:** M&M features a fully customizable UI. Players can move, resize, and reposition all windows — combat log, chat, health/mana bars, target frames. There is no fixed screen position for any element. The damage meter must adapt to each player's layout, and re-adapt when they change it. A visual overlay where the user draws a rectangle is the most intuitive approach.
- **Alternatives considered:** Manual coordinate entry (poor UX), automatic detection via template matching (fragile across UI themes and resolutions, M&M UI can be restyled), fixed presets (impossible given customizable UI).
- **Reversibility:** Easy. Region selection is purely a configuration concern.

### 5. Mana Tick Detection via Pixel Sampling (Not OCR)
- **Decision:** Detect mana ticks by monitoring the mana bar's pixel fill level, not by OCR-ing a mana number.
- **Rationale:** The mana bar is a visual fill bar where the fill percentage is directly readable from pixel color ratios. This is faster and more reliable than trying to OCR a small numeric mana value from the UI, which would be error-prone at small font sizes. Pixel sampling is simpler and more robust than text OCR for this use case. M&M's mana ticks occur on a ~6-second server cycle, so even coarse sampling at 500ms easily captures the change.
- **Alternatives considered:** OCR of mana text value (fragile, small text, font-dependent), memory reading (ToS violation), fixed tick timer without visual confirmation (doesn't account for variable regen rates from Meditation skill, buffs, campfire proximity).
- **Reversibility:** Easy. The mana tracking module is independent. A more precise approach can replace it.

### 6. In-Memory Session State with Persistent Config
- **Decision:** Session data (events, summaries) lives in memory only. Configuration (regions, intervals, preferences) persists to a TOML config file.
- **Rationale:** Damage meter data is session-scoped. Players care about the current fight or grind session, not historical data. Config persistence means the user sets their regions once and they survive restarts. Session data is ephemeral by design.
- **Alternatives considered:** SQLite for session history (future feature), JSON export (future feature).
- **Reversibility:** Easy. Adding persistence means writing a storage module that serializes session state.

### 7. Pipe-Delimited Log Display Format
- **Decision:** Display events as `player | attack | target | damage` in the log panel.
- **Rationale:** Matches the existing v4 screenshot format. Clear, scannable, compact. Pipe delimiter avoids ambiguity with spaces in M&M entity names like "a fire beetle" or "ice comet".
- **Reversibility:** Easy. Display format is a UI concern only.

---

## M&M-Specific Combat Log Format

### Observed Format (from Screenshot)

The combat log lines in M&M appear in the following pattern when filtered to a combat-only chat window:

```
you | pierce | a fire beetle | 6
santa | bash's | a fire beetle | 10
your | fire blast | a skeleton | 2
holkan | slam's | a fire beetle | 4
demma | ice comet | a ghoul | 5
```

### Parsing Grammar

```
damage_line := source_player SEPARATOR attack_name SEPARATOR target SEPARATOR damage_value

source_player := word+              # "you", "santa", "holkan", "your" (possessive)
attack_name   := word+              # "pierce", "bash's", "fire blast", "ice comet", "slam's"
target        := word+              # "a fire beetle", "a skeleton", "a ghoul"
damage_value  := digit+             # "6", "10", "2"

SEPARATOR     := " | "              # space-pipe-space (but OCR may produce variants)
```

### OCR Tolerance Rules

The parser must handle common OCR misreads:
- Pipe `|` misread as `l`, `I`, `1`, or `!` — treat any of these as a potential separator when flanked by spaces
- Leading/trailing whitespace variations around separators
- Case variations (OCR may capitalize inconsistently)
- Partial lines (truncated at region boundary) — discard if fewer than 4 segments
- Numeric confusion: `0` vs `O`, `1` vs `l` vs `I` in the damage value field — attempt numeric parse with substitution

### Known M&M Combat Text Variations to Handle

Based on the game's chat filter system and EQ-heritage combat model:

```yaml
combat_text_patterns:
  melee_hit: "you | pierce | a fire beetle | 6"
  ability_hit: "santa | bash's | a fire beetle | 10"
  spell_hit: "your | fire blast | a skeleton | 2"
  pet_hit: "your pet | bite | a fire beetle | 3"    # if pets are tracked
  
  # Lines to IGNORE (not damage events — different format, no pipe delimiters):
  miss: "you try to pierce a fire beetle, but miss!"
  dodge: "a fire beetle dodges your attack!"
  resist: "a skeleton resists your fire blast!"
  death: "a fire beetle has been slain!"
  xp: "you gain experience!"
```

The parser focuses on lines matching the pipe-delimited damage format and ignores everything else. Non-matching lines are silently dropped.

---

## Mana Tick Detection Details

### M&M Mana Regeneration Model

Mana regeneration in Monsters & Memories operates on a server tick of approximately 6 seconds. The amount of mana regenerated per tick depends on several factors:

- **Base regen:** Small amount when standing (~1 mana/tick), increased when sitting (~2 mana/tick)
- **Meditation skill:** +1 mana per tick per 10 skill points (e.g., 100 Meditation = +10 mana/tick)
- **Campfire/Hearth buff:** Passive mana regeneration bonus when near a player-made campfire or inside an inn/tavern
- **Spell buffs:** Various class buffs (Clarity-style effects) that increase mana regen per tick
- **Combat state:** Mana regen is typically reduced or paused during active combat (varies by implementation)

### Detection Algorithm

```
1. Sample mana bar fill percentage every 500ms
2. Maintain a rolling buffer of the last 15 samples (~7.5 seconds, covering one full tick cycle plus margin)
3. Detect a "stable increase" event when:
   a. The fill percentage increases by more than a minimum threshold (e.g., > 0.5% to filter noise)
   b. The new level remains stable for at least 2 consecutive samples (1 second, debounce)
   c. The increase was not preceded by a rapid decrease (which would indicate mana spend + regen, where we only want the net regen tick)
4. Record the tick with the delta and timestamp
5. If the fill decreases (spell cast), record a spend event separately but do not emit a tick
```

### Edge Cases

- **Mana bar at 100%:** No tick can be detected (no visible change). The detector should note "bar full" state.
- **Rapid casting during regen:** Cast (bar drops) → tick (bar increases) → cast (bar drops). The detector must disambiguate the regen tick from the post-cast level.
- **Bar empty (0%):** If the player is completely OOM, a tick is a small increase from 0.
- **UI obstruction:** If another window partially covers the mana bar, the fill calculation is corrupted. The detector should notice anomalous readings (sudden large jumps or drops) and flag them.

---

## Dependencies

### External
- **OCR Engine:** leptess/tesseract-rs (Rust bindings to Tesseract) or windows-rs for Windows OCR API, or rusty-tesseract. Must be statically linkable or bundled with the binary.
- **Screen Capture:** scrap, xcap, or platform-native APIs via Rust bindings (Win32 BitBlt, macOS CGWindowListCreateImage, X11 XGetImage / Wayland wlr-screencopy)
- **GUI Framework:** egui (via eframe) for immediate-mode cross-platform GUI, or iced for a retained-mode alternative. Both produce native binaries with no runtime dependency.

### Internal (Modules Produced by This Spec)

```
┌─────────────────────────────────────────────────────────┐
│                                                         │
│  ┌───────────────────┐                                  │
│  │  Config Manager    │──────────────────────┐          │
│  │  (TOML read/write, │                      │          │
│  │   region coords,   │                      │          │
│  │   intervals, prefs)│                      │          │
│  └───────────────────┘                       │          │
│           │                                  │          │
│           ↓                                  ↓          │
│  ┌───────────────────┐            ┌────────────────┐    │
│  │  Region Selector   │            │  Region Selector│   │
│  │  Overlay           │            │  Overlay        │   │
│  │  (combat log)      │            │  (mana bar)     │   │
│  └────────┬──────────┘            └───────┬────────┘    │
│           │                               │             │
│           ↓                               ↓             │
│  ┌───────────────────┐            ┌────────────────┐    │
│  │  Screen Capture    │            │  Screen Capture │   │
│  │  (combat log       │            │  (mana bar      │   │
│  │   region, 250ms)   │            │   region, 500ms)│   │
│  └────────┬──────────┘            └───────┬────────┘    │
│           │                               │             │
│           ↓                               ↓             │
│  ┌───────────────────┐            ┌────────────────┐    │
│  │  OCR Engine        │            │  Pixel Sampler  │   │
│  │  (image → text     │            │  (image → fill  │   │
│  │   lines)           │            │   percentage)   │   │
│  └────────┬──────────┘            └───────┬────────┘    │
│           │                               │             │
│           ↓                               ↓             │
│  ┌───────────────────┐            ┌────────────────┐    │
│  │  Line Parser       │            │  Tick Detector  │   │
│  │  (text → damage    │            │  (fill deltas → │   │
│  │   event or null)   │            │   mana ticks)   │   │
│  └────────┬──────────┘            └───────┬────────┘    │
│           │                               │             │
│           ↓                               │             │
│  ┌───────────────────┐                    │             │
│  │  Deduplicator      │                    │             │
│  │  (filters repeated │                    │             │
│  │   lines via hash   │                    │             │
│  │   window)          │                    │             │
│  └────────┬──────────┘                    │             │
│           │                               │             │
│           └──────────────┬────────────────┘             │
│                          ↓                              │
│               ┌─────────────────────┐                   │
│               │  Session Accumulator │                   │
│               │  (damage events +    │                   │
│               │   mana ticks →       │                   │
│               │   summary stats)     │                   │
│               └──────────┬──────────┘                   │
│                          │                              │
│                          ↓                              │
│  ┌──────────────────────────────────────────────────┐   │
│  │  UI Renderer (egui/eframe)                       │   │
│  │  ┌────────────────────────────────────────────┐  │   │
│  │  │  Control Bar: Pause|Resume|Clear|Reset     │  │   │
│  │  │  Region Buttons: Set Log Region | Set Mana │  │   │
│  │  ├────────────────────────────────────────────┤  │   │
│  │  │  Top Panel: Scrolling Damage Event Log     │  │   │
│  │  ├────────────────────────────────────────────┤  │   │
│  │  │  Bottom Panel: Damage Summary + Mana Ticks │  │   │
│  │  └────────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────┘   │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

Each box is a candidate atomic module. They compose within a single Rust binary but maintain clear trait/interface boundaries for independent testing and replacement.

---

## Open Questions

> These must be resolved before implementation tasks are decomposed.

### RESOLVED
- ~~Which game?~~ **Monsters & Memories**
- ~~Programming language?~~ **Rust**
- ~~Docker?~~ **No Docker, native binaries only**

### 4. GUI framework selection?
- **Question:** Which Rust GUI framework?
- **Options:**
  - **egui (via eframe)** — immediate-mode, very fast to build, good for tool-style apps, cross-platform, renders via wgpu or glow. Aesthetic is "developer tool" which matches the damage meter use case well.
  - **iced** — elm-inspired retained-mode, cleaner native look, but slower iteration and more boilerplate.
  - **Slint** — declarative UI with a markup language, good cross-platform story, but adds a DSL dependency.
- **Recommendation:** egui — most pragmatic for a real-time data display tool. The immediate-mode pattern naturally handles the "redraw summaries every frame" requirement.
- **Impact:** Affects UI module structure and the overlay implementation for region selection.

### 5. OCR engine bundling strategy?
- **Question:** How to bundle Tesseract (or alternative) with the binary?
- **Options:**
  - **Static link Tesseract + Leptonica** — produces a true single binary but complex cross-compilation
  - **Ship Tesseract as a sidecar** — binary + tesseract data files in a zip
  - **Windows OCR API on Windows, Tesseract on Linux/macOS** — platform-specific backends behind a trait
  - **Embedded lightweight OCR** — a simpler, purpose-built recognizer tuned for M&M's specific UI font (faster, smaller, but less generalizable)
- **Impact:** Affects binary size, cross-platform build complexity, and OCR accuracy.

### 6. M&M combat text format verification?
- **Question:** Is the pipe-delimited format (`player | attack | target | damage`) the actual raw in-game format, or is the screenshot showing output that was already processed by a prior version of this tool?
- **Impact:** If the raw game text uses a prose format (e.g., "You pierce a fire beetle for 6 points of damage!"), the parser needs a completely different grammar. The pipe-delimited format may already be the output of an earlier OCR parser.
- **Action needed:** Screenshot of a raw M&M combat chat window with damage filter enabled, with no external tools processing it.

### 7. Mana bar visual characteristics?
- **Question:** What exactly does the M&M mana bar look like? Solid blue fill? Gradient? Does it have a border? What color is the empty portion?
- **Impact:** Determines the pixel sampling algorithm's color matching logic.
- **Action needed:** Screenshot of the mana bar at various fill levels (full, ~50%, ~25%, empty).

### 8. Max mana pool input method?
- **Question:** How does the user tell the damage meter their max mana pool for absolute mana-per-tick estimates?
- **Options:**
  - Manual entry in a settings field
  - OCR of the mana text if M&M displays it as "250/500" somewhere in the UI
  - Infer from observing bar at full (100% fill = whatever max is, but we'd need a reference point)
- **Impact:** Determines whether the mana tick display shows only percentages or absolute values too.

---

## Decomposition Preview

> Preliminary breakdown of atomic tasks. Final decomposition after open questions are resolved.

### Phase 1: Foundation
1. **Config Manager module** — TOML config read/write for regions, intervals, mana color range, max mana pool
2. **Core types module** — DamageEvent, ManaTick, SessionState, CaptureRegion, AppConfig structs and traits
3. **Session Accumulator module** — append events, compute damage summaries, compute mana tick stats

### Phase 2: Screen Capture Pipeline
4. **Screen Capture module** — cross-platform screen region capture (Win32/macOS/X11 behind a trait)
5. **OCR Engine Wrapper module** — image-to-text-lines via Tesseract or platform OCR behind a trait
6. **Line Parser module** — text line → DamageEvent or None, with OCR tolerance rules
7. **Deduplicator module** — sliding hash window to filter repeated lines across captures

### Phase 3: Mana Tracking Pipeline
8. **Pixel Sampler module** — mana bar image → fill percentage via color ratio analysis
9. **Tick Detector module** — fill percentage stream → ManaTick events, with debounce logic aligned to ~6-second server tick

### Phase 4: User Interface
10. **Main Window app** — egui/eframe window with control bar, event log panel, summary panel
11. **Region Selector Overlay** — translucent fullscreen overlay with drag-to-select rectangle, returns coordinates
12. **Event Log renderer** — scrolling `player | attack | target | damage` display
13. **Summary renderer** — total damage, per-player, per-player-per-attack, mana tick stats
14. **Control bar** — Pause, Resume, Clear, Reset, Set Combat Log Region, Set Mana Bar Region

### Phase 5: Integration & Wiring
15. **Capture-to-display pipeline** — wires capture → OCR → parse → dedup → accumulate → render loop (async channels or message passing)
16. **Mana-to-display pipeline** — wires capture → sample → detect → accumulate → render loop
17. **Integration test suite** — test with sample screenshot images and expected parse/mana output

### Phase 6: Distribution
18. **Cross-platform build scripts** — cargo build targets for Windows (x86_64-pc-windows-msvc), macOS (aarch64-apple-darwin, x86_64-apple-darwin), Linux (x86_64-unknown-linux-gnu)
19. **CI/CD pipeline config** — GitHub Actions or similar for automated cross-platform builds and release artifacts

Each of these is a candidate atomic task producing a self-contained, testable Rust module.
