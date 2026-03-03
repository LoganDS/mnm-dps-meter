//! mnm-dps-meter — OCR-based damage meter for Monsters & Memories.
//!
//! Entry point: initializes tracing, loads config, and launches the
//! egui/eframe UI with region selection and session display.

mod config;
mod dedup;
mod overlay;
mod panel;
mod parser;
mod session;
mod tick;
mod types;
mod ui;

use config::AppConfig;
use tracing::info;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    info!("mnm-dps-meter starting");

    let config = AppConfig::load();
    info!(
        "Config loaded: character_name={:?}, combat_interval={}ms, panel_interval={}ms",
        config.character_name,
        config.combat_capture_interval_ms,
        config.panel_capture_interval_ms,
    );

    if config.combat_log_region.is_some() {
        info!("Combat log region configured");
    }
    if config.mini_panel_region.is_some() {
        info!("Mini panel region configured");
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Damage Meter (OCR)",
        options,
        Box::new(|_cc| Ok(Box::new(ui::DamageMeterApp::new(config)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
}
