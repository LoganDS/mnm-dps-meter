//! Main UI window for the mnm-dps-meter application.
//!
//! Implements the egui/eframe application shell with control bar,
//! scrolling event log, statistics panels, and region selection.
//! Receives pipeline messages via mpsc channel and updates the session
//! accumulator in real-time. Manual coordinate entry is the primary
//! (reliable) region selection method. Drag-select overlay is the
//! secondary (best-effort) method.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use eframe::egui;
use tracing::warn;

use crate::config::AppConfig;
use crate::overlay::{OverlayState, RegionTarget};
use crate::pipeline::{self, PipelineHandle};
use crate::session::SharedSession;
use crate::types::{CaptureRegion, PipelineMessage, SessionStatus};

/// Action produced by the manual entry dialog.
enum DialogAction {
    /// Save the entered region.
    Save(RegionTarget, CaptureRegion),
    /// Close without saving.
    Close,
}

/// State for the manual coordinate entry dialog.
struct RegionEntryDialog {
    /// Which region is being edited.
    target: RegionTarget,
    /// Input field: X coordinate.
    x: String,
    /// Input field: Y coordinate.
    y: String,
    /// Input field: width.
    width: String,
    /// Input field: height.
    height: String,
    /// Validation error message.
    error: Option<String>,
}

impl RegionEntryDialog {
    /// Create a new dialog, pre-filling with existing region values if available.
    fn new(target: RegionTarget, existing: Option<CaptureRegion>) -> Self {
        let (x, y, w, h) = match existing {
            Some(r) => (
                r.x.to_string(),
                r.y.to_string(),
                r.width.to_string(),
                r.height.to_string(),
            ),
            None => (String::new(), String::new(), String::new(), String::new()),
        };
        Self {
            target,
            x,
            y,
            width: w,
            height: h,
            error: None,
        }
    }

    /// Validate input fields and return a [`CaptureRegion`] or error message.
    fn validate(&self) -> Result<CaptureRegion, String> {
        let x: i32 = self
            .x
            .trim()
            .parse()
            .map_err(|_| "X must be a valid integer".to_string())?;
        let y: i32 = self
            .y
            .trim()
            .parse()
            .map_err(|_| "Y must be a valid integer".to_string())?;
        let width: u32 = self
            .width
            .trim()
            .parse()
            .map_err(|_| "Width must be a positive integer".to_string())?;
        let height: u32 = self
            .height
            .trim()
            .parse()
            .map_err(|_| "Height must be a positive integer".to_string())?;

        if x < 0 {
            return Err("X must be non-negative".to_string());
        }
        if y < 0 {
            return Err("Y must be non-negative".to_string());
        }
        if width == 0 {
            return Err("Width must be greater than zero".to_string());
        }
        if height == 0 {
            return Err("Height must be greater than zero".to_string());
        }

        Ok(CaptureRegion {
            x,
            y,
            width,
            height,
        })
    }
}

/// Main application struct implementing [`eframe::App`].
///
/// Holds a shared reference to the session state, pipeline communication
/// channels, pipeline thread handles, and the user's persisted config.
pub struct DamageMeterApp {
    /// Thread-safe shared session state.
    session: SharedSession,
    /// Persisted application config (regions, character name, intervals).
    config: AppConfig,
    /// Working copy of the character name text field.
    character_name_buf: String,
    /// Whether the event log should auto-scroll to the bottom.
    auto_scroll: bool,
    /// Active overlay state (if drag-select is in progress).
    overlay: Option<OverlayState>,
    /// Active manual entry dialog (if open).
    region_dialog: Option<RegionEntryDialog>,

    // --- Pipeline integration ---

    /// Receives messages from pipeline threads.
    receiver: mpsc::Receiver<PipelineMessage>,
    /// Sender clone for spawning new pipeline threads.
    sender: mpsc::Sender<PipelineMessage>,
    /// Shared pause flag checked by pipeline threads.
    paused: Arc<AtomicBool>,
    /// Handle to the combat log pipeline thread, if running.
    combat_pipeline: Option<PipelineHandle>,
    /// Handle to the mini panel pipeline thread, if running.
    mini_panel_pipeline: Option<PipelineHandle>,
    /// Startup error message (OCR health check failure, etc.).
    startup_error: Option<String>,
}

impl DamageMeterApp {
    /// Create a new application instance with pipeline infrastructure.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session: SharedSession,
        config: AppConfig,
        receiver: mpsc::Receiver<PipelineMessage>,
        sender: mpsc::Sender<PipelineMessage>,
        paused: Arc<AtomicBool>,
        combat_pipeline: Option<PipelineHandle>,
        mini_panel_pipeline: Option<PipelineHandle>,
        startup_error: Option<String>,
    ) -> Self {
        let character_name_buf = config.character_name.clone().unwrap_or_default();
        Self {
            session,
            config,
            character_name_buf,
            auto_scroll: true,
            overlay: None,
            region_dialog: None,
            receiver,
            sender,
            paused,
            combat_pipeline,
            mini_panel_pipeline,
            startup_error,
        }
    }

    /// Apply a selected region to config, persist, and restart the relevant pipeline.
    fn apply_region(&mut self, target: RegionTarget, region: CaptureRegion) {
        match target {
            RegionTarget::CombatLog => {
                self.config.combat_log_region = Some(region);
            }
            RegionTarget::MiniPanel => {
                self.config.mini_panel_region = Some(region);
            }
        }
        if let Err(e) = self.config.save() {
            warn!("Failed to save config: {}", e);
        }
        // Restart the relevant pipeline thread with the new region
        match target {
            RegionTarget::CombatLog => self.restart_combat_pipeline(),
            RegionTarget::MiniPanel => self.restart_mini_panel_pipeline(),
        }
    }

    /// Stop the combat log pipeline (if running) and spawn a new one.
    fn restart_combat_pipeline(&mut self) {
        if let Some(ref handle) = self.combat_pipeline {
            handle.signal_stop();
        }
        self.combat_pipeline = None;

        if let Some(region) = self.config.combat_log_region {
            match pipeline::spawn_combat_log_pipeline(
                region,
                self.config.combat_capture_interval_ms,
                self.config.character_name.clone(),
                self.sender.clone(),
                Arc::clone(&self.paused),
            ) {
                Ok(handle) => {
                    self.combat_pipeline = Some(handle);
                }
                Err(e) => {
                    warn!("Failed to start combat log pipeline: {}", e);
                }
            }
        }
    }

    /// Stop the mini panel pipeline (if running) and spawn a new one.
    fn restart_mini_panel_pipeline(&mut self) {
        if let Some(ref handle) = self.mini_panel_pipeline {
            handle.signal_stop();
        }
        self.mini_panel_pipeline = None;

        if let Some(region) = self.config.mini_panel_region {
            match pipeline::spawn_mini_panel_pipeline(
                region,
                self.config.panel_capture_interval_ms,
                self.sender.clone(),
                Arc::clone(&self.paused),
            ) {
                Ok(handle) => {
                    self.mini_panel_pipeline = Some(handle);
                }
                Err(e) => {
                    warn!("Failed to start mini panel pipeline: {}", e);
                }
            }
        }
    }

    /// Drain all pending messages from the pipeline receiver and update session state.
    fn drain_pipeline_messages(&self) {
        while let Ok(msg) = self.receiver.try_recv() {
            let mut session = self.session.lock().unwrap();
            match msg {
                PipelineMessage::DamageEvents(events) => {
                    for event in events {
                        session.add_damage_event(event);
                    }
                }
                PipelineMessage::ManaUpdate(stats) => {
                    session.update_vitals(stats);
                }
                PipelineMessage::ManaTick(tick) => {
                    session.add_mana_tick(tick);
                }
            }
        }
    }

    /// Get the current region for a target, if configured.
    fn get_region(&self, target: RegionTarget) -> Option<CaptureRegion> {
        match target {
            RegionTarget::CombatLog => self.config.combat_log_region,
            RegionTarget::MiniPanel => self.config.mini_panel_region,
        }
    }

    /// Render region selection controls (overlay button + manual entry + status).
    fn region_controls(&mut self, ui: &mut egui::Ui, target: RegionTarget) {
        let label = target.to_string();

        ui.horizontal(|ui| {
            if ui
                .button(format!("Set {} Region", label))
                .on_hover_text("Open drag-select overlay (best-effort)")
                .clicked()
            {
                self.overlay = Some(OverlayState::new(target));
            }
            if ui
                .small_button("Manual Entry")
                .on_hover_text("Enter coordinates manually")
                .clicked()
            {
                let existing = self.get_region(target);
                self.region_dialog = Some(RegionEntryDialog::new(target, existing));
            }

            // Show current region status inline
            if let Some(region) = self.get_region(target) {
                ui.label(format!(
                    "({}, {}) {}x{}",
                    region.x, region.y, region.width, region.height
                ));
            } else {
                ui.weak("Not configured");
            }
        });
    }

    /// Show the manual entry dialog window and handle its actions.
    fn show_region_dialog(&mut self, ctx: &egui::Context) {
        let action = if let Some(ref mut dialog) = self.region_dialog {
            let title = format!("Set {} Region", dialog.target);
            let mut action = None;
            let mut open = true;

            egui::Window::new(title)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Enter screen-absolute coordinates:");
                    ui.add_space(4.0);

                    egui::Grid::new("region_entry_grid")
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("X:");
                            ui.add(
                                egui::TextEdit::singleline(&mut dialog.x).desired_width(100.0),
                            );
                            ui.end_row();

                            ui.label("Y:");
                            ui.add(
                                egui::TextEdit::singleline(&mut dialog.y).desired_width(100.0),
                            );
                            ui.end_row();

                            ui.label("Width:");
                            ui.add(
                                egui::TextEdit::singleline(&mut dialog.width)
                                    .desired_width(100.0),
                            );
                            ui.end_row();

                            ui.label("Height:");
                            ui.add(
                                egui::TextEdit::singleline(&mut dialog.height)
                                    .desired_width(100.0),
                            );
                            ui.end_row();
                        });

                    if let Some(ref err) = dialog.error {
                        ui.add_space(4.0);
                        ui.colored_label(egui::Color32::RED, err);
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            match dialog.validate() {
                                Ok(region) => {
                                    action = Some(DialogAction::Save(dialog.target, region));
                                }
                                Err(msg) => {
                                    dialog.error = Some(msg);
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            action = Some(DialogAction::Close);
                        }
                    });
                });

            if !open {
                Some(DialogAction::Close)
            } else {
                action
            }
        } else {
            None
        };

        match action {
            Some(DialogAction::Save(target, region)) => {
                self.apply_region(target, region);
                self.region_dialog = None;
            }
            Some(DialogAction::Close) => {
                self.region_dialog = None;
            }
            None => {}
        }
    }

    /// Render the control bar at the top of the window.
    fn render_control_bar(&mut self, ui: &mut egui::Ui) {
        let status = {
            let session = self.session.lock().unwrap();
            session.status
        };

        // Row 1: Session control buttons + status
        ui.horizontal(|ui| {
            let is_paused = status == SessionStatus::Paused;

            if ui
                .add_enabled(!is_paused, egui::Button::new("Pause"))
                .clicked()
            {
                self.session.lock().unwrap().pause();
                self.paused.store(true, Ordering::Relaxed);
            }

            if ui
                .add_enabled(is_paused, egui::Button::new("Resume"))
                .clicked()
            {
                self.session.lock().unwrap().resume();
                self.paused.store(false, Ordering::Relaxed);
            }

            if ui.button("Clear").clicked() {
                self.session.lock().unwrap().clear_display();
            }

            if ui.button("Reset").clicked() {
                self.session.lock().unwrap().reset();
            }

            ui.separator();

            // Status indicator
            let status_text = match status {
                SessionStatus::Running => "Running",
                SessionStatus::Paused => "Paused",
            };
            let status_color = match status {
                SessionStatus::Running => egui::Color32::from_rgb(80, 200, 80),
                SessionStatus::Paused => egui::Color32::from_rgb(200, 200, 80),
            };
            ui.colored_label(status_color, status_text);
        });

        // Row 2+3: Region selection controls
        self.region_controls(ui, RegionTarget::CombatLog);
        self.region_controls(ui, RegionTarget::MiniPanel);

        ui.label(
            egui::RichText::new(
                "Regions are screen-absolute coordinates. \
                 If you move your game window, reconfigure your regions.",
            )
            .small()
            .weak(),
        );

        // Character name input
        ui.horizontal(|ui| {
            ui.label("Character Name:");
            let response = ui.text_edit_singleline(&mut self.character_name_buf);
            if response.changed() {
                let name = self.character_name_buf.trim().to_string();
                let name_opt = if name.is_empty() {
                    None
                } else {
                    Some(name.clone())
                };
                self.config.character_name = name_opt.clone();
                self.session.lock().unwrap().character_name = name_opt;
                let _ = self.config.save();
            }
        });
    }

    /// Render the scrolling combat event log panel.
    fn render_event_log(&self, ui: &mut egui::Ui) {
        ui.heading("Combat Log");

        let events = {
            let session = self.session.lock().unwrap();
            session
                .visible_damage_events()
                .iter()
                .map(|e| {
                    let element_suffix = match &e.damage_element {
                        Some(elem) => format!(" ({})", elem),
                        None => String::new(),
                    };
                    format!(
                        "{} | {} | {} | {}{}",
                        e.source_player, e.attack_name, e.target, e.damage_value, element_suffix
                    )
                })
                .collect::<Vec<_>>()
        };

        let text_style = egui::TextStyle::Monospace;
        let row_height = ui.text_style_height(&text_style);
        let total_rows = events.len();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll)
            .show_rows(ui, row_height, total_rows, |ui, row_range| {
                for row in row_range {
                    ui.label(egui::RichText::new(&events[row]).monospace());
                }
            });
    }

    /// Render the statistics summary panel.
    fn render_statistics(&self, ui: &mut egui::Ui) {
        ui.heading("Statistics");

        let session = self.session.lock().unwrap();

        // Total damage
        ui.strong(format!("TOTAL DAMAGE: {}", session.total_damage));
        ui.separator();

        // Damage by player (sorted descending)
        ui.label(egui::RichText::new("Damage by player:").strong());
        let player_sorted = session.damage_by_player_sorted();
        for (player, dmg) in &player_sorted {
            ui.indent(player, |ui| {
                ui.label(format!("{}: {}", player, dmg));
            });
        }

        // Damage by player -> attack (nested, sorted descending)
        if !player_sorted.is_empty() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Damage by player \u{2192} attack:").strong());
            for (player, _) in &player_sorted {
                let attacks = session.attacks_for_player_sorted(player);
                if !attacks.is_empty() {
                    ui.indent(player, |ui| {
                        ui.label(egui::RichText::new(format!("{}:", player)).strong());
                        for (attack, dmg) in &attacks {
                            ui.indent(attack, |ui| {
                                ui.label(format!("{}: {}", attack, dmg));
                            });
                        }
                    });
                }
            }
        }

        ui.separator();

        // Mana stats
        let mana_label = match (session.current_mana, session.max_mana) {
            (Some(cur), Some(max)) => format!("Mana ({}/{})", cur, max),
            (Some(cur), None) => format!("Mana ({}/?)", cur),
            _ => "Mana".to_string(),
        };
        ui.label(egui::RichText::new(mana_label).strong());

        ui.indent("mana_stats", |ui| {
            ui.label(format!("Ticks observed: {}", session.mana_ticks_count));
            ui.label(format!(
                "Avg regen/tick: {:.1}",
                session.mana_avg_per_tick()
            ));
            ui.label(format!("Total regen: {}", session.mana_regen_total));
            ui.label(format!("Total spent: {}", session.mana_spend_total));
        });
    }
}

impl Drop for DamageMeterApp {
    fn drop(&mut self) {
        // Signal all pipeline threads to shut down
        if let Some(ref handle) = self.combat_pipeline {
            handle.signal_stop();
        }
        if let Some(ref handle) = self.mini_panel_pipeline {
            handle.signal_stop();
        }
    }
}

impl eframe::App for DamageMeterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain all pending pipeline messages and update session state
        self.drain_pipeline_messages();

        // Check for overlay completion
        let overlay_result = if let Some(ref overlay) = self.overlay {
            if overlay.done {
                Some((overlay.target, overlay.result))
            } else {
                None
            }
        } else {
            None
        };

        if let Some((target, result)) = overlay_result {
            if let Some(region) = result {
                self.apply_region(target, region);
            }
            self.overlay = None;
        }

        // Show overlay viewport if active
        if let Some(ref mut overlay) = self.overlay {
            overlay.show(ctx);
        }

        // Manual entry dialog (rendered as a floating egui Window)
        self.show_region_dialog(ctx);

        // Title bar via top panel
        egui::TopBottomPanel::top("title_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Damage Meter (OCR) - v0.5");
            });
        });

        // Startup error banner (OCR health check failure, etc.)
        if let Some(ref error) = self.startup_error {
            egui::TopBottomPanel::top("error_banner").show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::RED,
                    format!("OCR unavailable: {}", error),
                );
                ui.label(
                    egui::RichText::new(
                        "Capture pipelines are disabled. Fix the issue and restart the app.",
                    )
                    .small()
                    .weak(),
                );
            });
        }

        // Control bar
        egui::TopBottomPanel::top("control_bar").show(ctx, |ui| {
            self.render_control_bar(ui);
            ui.add_space(2.0);
        });

        // Statistics panel at the bottom
        egui::TopBottomPanel::bottom("statistics_panel")
            .resizable(true)
            .default_height(250.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.render_statistics(ui);
                    });
            });

        // Event log fills the remaining center
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_event_log(ui);
        });

        // Request repaint at ~30fps to keep the UI responsive to incoming data
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::new_shared_session;

    /// Helper: create a DamageMeterApp with dummy pipeline infrastructure for testing.
    fn test_app(session: SharedSession, config: AppConfig) -> DamageMeterApp {
        let (sender, receiver) = mpsc::channel();
        DamageMeterApp::new(
            session,
            config,
            receiver,
            sender,
            Arc::new(AtomicBool::new(false)),
            None,
            None,
            None,
        )
    }

    #[test]
    fn test_region_entry_validation_valid() {
        let dialog = RegionEntryDialog {
            target: RegionTarget::CombatLog,
            x: "100".to_string(),
            y: "200".to_string(),
            width: "800".to_string(),
            height: "300".to_string(),
            error: None,
        };
        let result = dialog.validate().unwrap();
        assert_eq!(result.x, 100);
        assert_eq!(result.y, 200);
        assert_eq!(result.width, 800);
        assert_eq!(result.height, 300);
    }

    #[test]
    fn test_region_entry_validation_with_whitespace() {
        let dialog = RegionEntryDialog {
            target: RegionTarget::MiniPanel,
            x: " 50 ".to_string(),
            y: " 75 ".to_string(),
            width: " 200 ".to_string(),
            height: " 150 ".to_string(),
            error: None,
        };
        let result = dialog.validate().unwrap();
        assert_eq!(result.x, 50);
        assert_eq!(result.y, 75);
        assert_eq!(result.width, 200);
        assert_eq!(result.height, 150);
    }

    #[test]
    fn test_region_entry_validation_zero_width() {
        let dialog = RegionEntryDialog {
            target: RegionTarget::CombatLog,
            x: "0".to_string(),
            y: "0".to_string(),
            width: "0".to_string(),
            height: "100".to_string(),
            error: None,
        };
        let result = dialog.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Width"));
    }

    #[test]
    fn test_region_entry_validation_zero_height() {
        let dialog = RegionEntryDialog {
            target: RegionTarget::CombatLog,
            x: "0".to_string(),
            y: "0".to_string(),
            width: "100".to_string(),
            height: "0".to_string(),
            error: None,
        };
        let result = dialog.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Height"));
    }

    #[test]
    fn test_region_entry_validation_negative_x() {
        let dialog = RegionEntryDialog {
            target: RegionTarget::CombatLog,
            x: "-10".to_string(),
            y: "0".to_string(),
            width: "100".to_string(),
            height: "100".to_string(),
            error: None,
        };
        let result = dialog.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("X"));
    }

    #[test]
    fn test_region_entry_validation_negative_y() {
        let dialog = RegionEntryDialog {
            target: RegionTarget::CombatLog,
            x: "0".to_string(),
            y: "-5".to_string(),
            width: "100".to_string(),
            height: "100".to_string(),
            error: None,
        };
        let result = dialog.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Y"));
    }

    #[test]
    fn test_region_entry_validation_non_numeric() {
        let dialog = RegionEntryDialog {
            target: RegionTarget::CombatLog,
            x: "abc".to_string(),
            y: "0".to_string(),
            width: "100".to_string(),
            height: "100".to_string(),
            error: None,
        };
        let result = dialog.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("X"));
    }

    #[test]
    fn test_region_entry_validation_empty_fields() {
        let dialog = RegionEntryDialog {
            target: RegionTarget::CombatLog,
            x: "".to_string(),
            y: "".to_string(),
            width: "".to_string(),
            height: "".to_string(),
            error: None,
        };
        let result = dialog.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_region_entry_prefill_from_existing() {
        let region = CaptureRegion {
            x: 100,
            y: 200,
            width: 800,
            height: 300,
        };
        let dialog = RegionEntryDialog::new(RegionTarget::CombatLog, Some(region));
        assert_eq!(dialog.x, "100");
        assert_eq!(dialog.y, "200");
        assert_eq!(dialog.width, "800");
        assert_eq!(dialog.height, "300");
        assert_eq!(dialog.target, RegionTarget::CombatLog);
    }

    #[test]
    fn test_region_entry_no_existing() {
        let dialog = RegionEntryDialog::new(RegionTarget::MiniPanel, None);
        assert_eq!(dialog.x, "");
        assert_eq!(dialog.y, "");
        assert_eq!(dialog.width, "");
        assert_eq!(dialog.height, "");
        assert_eq!(dialog.target, RegionTarget::MiniPanel);
    }

    #[test]
    fn test_app_creation() {
        let config = AppConfig::default();
        let session = new_shared_session();
        let app = test_app(session, config);
        assert!(app.overlay.is_none());
        assert!(app.region_dialog.is_none());
        assert!(app.startup_error.is_none());
    }

    #[test]
    fn test_app_creation_with_character_name() {
        let config = AppConfig {
            character_name: Some("Narky".to_string()),
            ..Default::default()
        };
        let session = new_shared_session();
        if let Some(ref name) = config.character_name {
            session.lock().unwrap().character_name = Some(name.clone());
        }
        let app = test_app(session, config);
        let session = app.session.lock().unwrap();
        assert_eq!(session.character_name, Some("Narky".to_string()));
    }

    #[test]
    fn test_get_region_combat_log() {
        let region = CaptureRegion {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        };
        let config = AppConfig {
            combat_log_region: Some(region),
            ..Default::default()
        };
        let session = new_shared_session();
        let app = test_app(session, config);
        assert_eq!(app.get_region(RegionTarget::CombatLog), Some(region));
        assert_eq!(app.get_region(RegionTarget::MiniPanel), None);
    }

    #[test]
    fn test_get_region_mini_panel() {
        let region = CaptureRegion {
            x: 50,
            y: 60,
            width: 150,
            height: 100,
        };
        let config = AppConfig {
            mini_panel_region: Some(region),
            ..Default::default()
        };
        let session = new_shared_session();
        let app = test_app(session, config);
        assert_eq!(app.get_region(RegionTarget::MiniPanel), Some(region));
        assert_eq!(app.get_region(RegionTarget::CombatLog), None);
    }

    #[test]
    fn test_drain_pipeline_messages() {
        let session = new_shared_session();
        let (sender, receiver) = mpsc::channel();
        let app = DamageMeterApp::new(
            Arc::clone(&session),
            AppConfig::default(),
            receiver,
            sender.clone(),
            Arc::new(AtomicBool::new(false)),
            None,
            None,
            None,
        );

        // Send a damage event batch
        use crate::types::DamageEvent;
        use std::time::Instant;
        let event = DamageEvent {
            id: 1,
            timestamp: Instant::now(),
            source_player: "Narky".to_string(),
            attack_name: "pierce".to_string(),
            target: "a cultist".to_string(),
            damage_value: 7,
            damage_element: None,
            raw_line: "Narky pierces a cultist for 7 points of damage.".to_string(),
        };
        sender
            .send(PipelineMessage::DamageEvents(vec![event]))
            .unwrap();

        // Send a vitals update
        use crate::types::VitalStats;
        sender
            .send(PipelineMessage::ManaUpdate(VitalStats {
                hp_current: Some(442),
                hp_max: Some(454),
                mana_current: Some(185),
                mana_max: Some(470),
                endurance_current: None,
                endurance_max: None,
            }))
            .unwrap();

        // Drain and verify
        app.drain_pipeline_messages();

        let session = session.lock().unwrap();
        assert_eq!(session.total_damage, 7);
        assert_eq!(session.damage_events.len(), 1);
        assert_eq!(session.current_mana, Some(185));
        assert_eq!(session.current_hp, Some(442));
    }

    #[test]
    fn test_app_with_startup_error() {
        let (sender, receiver) = mpsc::channel();
        let app = DamageMeterApp::new(
            new_shared_session(),
            AppConfig::default(),
            receiver,
            sender,
            Arc::new(AtomicBool::new(false)),
            None,
            None,
            Some("Tesseract not found".to_string()),
        );
        assert_eq!(
            app.startup_error.as_deref(),
            Some("Tesseract not found")
        );
    }
}
