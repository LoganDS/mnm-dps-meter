//! Main UI window for the mnm-dps-meter application.
//!
//! Implements the egui/eframe application shell with control bar,
//! scrolling event log, and statistics panels. Reads from a shared
//! [`SessionState`] and displays real-time damage and mana data.

use eframe::egui;

use crate::config::AppConfig;
use crate::session::SharedSession;
use crate::types::SessionStatus;

/// Main application struct implementing [`eframe::App`].
///
/// Holds a shared reference to the session state (for pipeline thread
/// communication) and the user's persisted config.
pub struct DamageMeterApp {
    /// Thread-safe shared session state.
    session: SharedSession,
    /// Persisted application config (regions, character name, intervals).
    config: AppConfig,
    /// Working copy of the character name text field.
    character_name_buf: String,
    /// Whether the event log should auto-scroll to the bottom.
    auto_scroll: bool,
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
        }
    }

    /// Render the control bar at the top of the window.
    fn render_control_bar(&mut self, ui: &mut egui::Ui) {
        let status = {
            let session = self.session.lock().unwrap();
            session.status
        };

        // Row 1: Session control buttons
        ui.horizontal(|ui| {
            let is_paused = status == SessionStatus::Paused;

            if ui.add_enabled(
                !is_paused,
                egui::Button::new("Pause"),
            ).clicked() {
                self.session.lock().unwrap().pause();
            }

            if ui.add_enabled(
                is_paused,
                egui::Button::new("Resume"),
            ).clicked() {
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

        // Row 2: Region buttons (placeholders for now)
        ui.horizontal(|ui| {
            if ui.button("Set Combat Log Region").clicked() {
                // Placeholder — wired in DPS-07
            }
            if ui.button("Set Mini Panel Region").clicked() {
                // Placeholder — wired in DPS-07
            }
        });

        ui.label(
            egui::RichText::new(
                "Regions are screen-absolute coordinates. If you move your game window, reconfigure your regions."
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
                let name_opt = if name.is_empty() { None } else { Some(name.clone()) };
                self.config.character_name = name_opt.clone();
                self.session.lock().unwrap().character_name = name_opt;
                // Save config on change (ignore errors — best effort)
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
            ui.label(egui::RichText::new("Damage by player → attack:").strong());
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
