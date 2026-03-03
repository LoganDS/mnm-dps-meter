//! mnm-dps-meter — OCR-based damage meter for Monsters & Memories.
//!
//! Entry point: initializes tracing, loads config, creates shared session
//! state, and launches the egui/eframe UI window.

mod config;
mod dedup;
mod panel;
mod parser;
mod session;
mod tick;
mod types;
mod ui;

use config::AppConfig;
use session::new_shared_session;
use tracing::info;
use ui::DamageMeterApp;

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

    // Create shared session state
    let session = new_shared_session();

    // Set character name from config into session
    if let Some(ref name) = config.character_name {
        session.lock().unwrap().character_name = Some(name.clone());
    }

    // Launch the egui/eframe UI
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Damage Meter (OCR) - v0.5",
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(DamageMeterApp::new(session, config)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    info!("mnm-dps-meter shutting down");
    Ok(())
}
