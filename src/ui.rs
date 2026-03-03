//! Main UI window for the mnm-dps-meter application.
//!
//! Implements the egui/eframe application shell with control bar,
//! scrolling event log, statistics panels, and region selection.
//! Manual coordinate entry is the primary (reliable) region selection method.
//! Drag-select overlay is the secondary (best-effort) method.

use eframe::egui;
use tracing::warn;

use crate::config::AppConfig;
use crate::overlay::{OverlayState, RegionTarget};
use crate::session::SharedSession;
use crate::types::{CaptureRegion, SessionStatus};

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
/// Holds a shared reference to the session state (for pipeline thread
/// communication) and the user's persisted config, plus overlay/dialog state.
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
}

impl DamageMeterApp {
    /// Create a new application instance.
    pub fn new(session: SharedSession, config: AppConfig) -> Self {
        let character_name_buf = config.character_name.clone().unwrap_or_default();
        Self {
            session,
            config,
            character_name_buf,
            auto_scroll: true,
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
            }

            if ui
                .add_enabled(is_paused, egui::Button::new("Resume"))
                .clicked()
            {
                self.session.lock().unwrap().resume();
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

        // Manual entry dialog (rendered as a floating egui Window)
        self.show_region_dialog(ctx);

        // Title bar via top panel
        egui::TopBottomPanel::top("title_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Damage Meter (OCR) - v0.5");
            });
        });

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
        let app = DamageMeterApp::new(session, config);
        assert!(app.overlay.is_none());
        assert!(app.region_dialog.is_none());
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
        let app = DamageMeterApp::new(session, config);
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
        let app = DamageMeterApp::new(session, config);
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
        let app = DamageMeterApp::new(session, config);
        assert_eq!(app.get_region(RegionTarget::MiniPanel), Some(region));
        assert_eq!(app.get_region(RegionTarget::CombatLog), None);
    }
}
