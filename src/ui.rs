//! Main UI module for the mnm-dps-meter application.
//!
//! Implements the egui/eframe application with region selection controls.
//! Manual coordinate entry is the primary (reliable) region selection method.
//! Drag-select overlay is the secondary (best-effort) method.
//!
//! Region selection buttons attempt the overlay first; a "Manual Entry"
//! button next to each provides the reliable fallback.

use crate::config::AppConfig;
use crate::overlay::{OverlayState, RegionTarget};
use crate::session::{new_shared_session, SharedSession};
use crate::types::{CaptureRegion, SessionStatus};
use eframe::egui;
use tracing::warn;

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

/// Main application state for the damage meter UI.
pub struct DamageMeterApp {
    /// Thread-safe session state shared with pipeline threads.
    session: SharedSession,
    /// Persistent application configuration.
    config: AppConfig,
    /// Active overlay state (if drag-select is in progress).
    overlay: Option<OverlayState>,
    /// Active manual entry dialog (if open).
    region_dialog: Option<RegionEntryDialog>,
}

impl DamageMeterApp {
    /// Create a new app instance with the given config.
    pub fn new(config: AppConfig) -> Self {
        let session = new_shared_session();
        if let Some(ref name) = config.character_name {
            if let Ok(mut s) = session.lock() {
                s.character_name = Some(name.clone());
            }
        }
        Self {
            session,
            config,
            overlay: None,
            region_dialog: None,
        }
    }

    /// Apply a selected region to config and persist.
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
                                egui::TextEdit::singleline(&mut dialog.width).desired_width(100.0),
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
}

impl eframe::App for DamageMeterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        // Snapshot session status for display (avoid holding lock during UI)
        let status = self
            .session
            .lock()
            .map(|s| s.status)
            .unwrap_or(SessionStatus::Running);

        // Top panel: control bar with region selection
        egui::TopBottomPanel::top("control_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Damage Meter (OCR) - v0.5");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match status {
                        SessionStatus::Running => {
                            ui.colored_label(egui::Color32::GREEN, "Running");
                        }
                        SessionStatus::Paused => {
                            ui.colored_label(egui::Color32::YELLOW, "Paused");
                        }
                    }
                });
            });
            ui.separator();

            // Session controls
            ui.horizontal(|ui| {
                if ui.button("Pause").clicked() {
                    if let Ok(mut s) = self.session.lock() {
                        s.pause();
                    }
                }
                if ui.button("Resume").clicked() {
                    if let Ok(mut s) = self.session.lock() {
                        s.resume();
                    }
                }
                if ui.button("Clear").clicked() {
                    if let Ok(mut s) = self.session.lock() {
                        s.clear_display();
                    }
                }
                if ui.button("Reset").clicked() {
                    if let Ok(mut s) = self.session.lock() {
                        s.reset();
                    }
                }
            });
            ui.separator();

            // Region selection controls
            self.region_controls(ui, RegionTarget::CombatLog);
            self.region_controls(ui, RegionTarget::MiniPanel);
            ui.add_space(4.0);

            // Character name
            ui.horizontal(|ui| {
                ui.label("Character Name:");
                let mut name = self.config.character_name.clone().unwrap_or_default();
                if ui.text_edit_singleline(&mut name).changed() {
                    let trimmed = name.trim().to_string();
                    if trimmed.is_empty() {
                        self.config.character_name = None;
                        if let Ok(mut s) = self.session.lock() {
                            s.character_name = None;
                        }
                    } else {
                        self.config.character_name = Some(trimmed.clone());
                        if let Ok(mut s) = self.session.lock() {
                            s.character_name = Some(trimmed);
                        }
                    }
                    if let Err(e) = self.config.save() {
                        warn!("Failed to save config: {}", e);
                    }
                }
            });

            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "Note: Regions are screen-absolute coordinates. \
                     If you move your game window, reconfigure your regions.",
                )
                .small()
                .weak(),
            );
        });

        // Manual entry dialog (rendered as a floating egui Window)
        self.show_region_dialog(ctx);

        // Central panel: event log + statistics (placeholder for DPS-06)
        egui::CentralPanel::default().show(ctx, |ui| {
            let session = self.session.lock().unwrap();

            ui.heading("Combat Log");
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() * 0.5)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for event in session.visible_damage_events() {
                        let element_str = event
                            .damage_element
                            .as_deref()
                            .map(|e| format!(" ({})", e))
                            .unwrap_or_default();
                        ui.label(format!(
                            "{} | {} | {} | {}{}",
                            event.source_player,
                            event.attack_name,
                            event.target,
                            event.damage_value,
                            element_str,
                        ));
                    }
                    if session.visible_damage_events().is_empty() {
                        ui.weak("No events yet.");
                    }
                });

            ui.separator();

            ui.heading("Statistics");
            ui.label(format!("TOTAL DAMAGE: {}", session.total_damage));

            if !session.damage_by_player.is_empty() {
                ui.add_space(4.0);
                for (player, dmg) in session.damage_by_player_sorted() {
                    ui.label(format!("  {} \u{2014} {}", player, dmg));
                    for (attack, attack_dmg) in session.attacks_for_player_sorted(&player) {
                        ui.label(format!("    {} \u{2014} {}", attack, attack_dmg));
                    }
                }
            }

            ui.separator();

            let mana_str = match (session.current_mana, session.max_mana) {
                (Some(cur), Some(max)) => format!("{}/{}", cur, max),
                (Some(cur), None) => format!("{}", cur),
                _ => "N/A".to_string(),
            };
            ui.label(format!("Mana ({})", mana_str));
            ui.label(format!("  Ticks: {}", session.mana_ticks_count));
            ui.label(format!(
                "  Avg regen/tick: {:.1}",
                session.mana_avg_per_tick()
            ));
            ui.label(format!("  Total regen: {}", session.mana_regen_total));
            ui.label(format!("  Total spent: {}", session.mana_spend_total));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let app = DamageMeterApp::new(config);
        assert!(app.overlay.is_none());
        assert!(app.region_dialog.is_none());
    }

    #[test]
    fn test_app_creation_with_character_name() {
        let config = AppConfig {
            character_name: Some("Narky".to_string()),
            ..Default::default()
        };
        let app = DamageMeterApp::new(config);
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
        let app = DamageMeterApp::new(config);
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
        let app = DamageMeterApp::new(config);
        assert_eq!(app.get_region(RegionTarget::MiniPanel), Some(region));
        assert_eq!(app.get_region(RegionTarget::CombatLog), None);
    }
}
