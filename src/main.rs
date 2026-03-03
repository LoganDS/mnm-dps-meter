//! mnm-dps-meter — OCR-based damage meter for Monsters & Memories.
//!
//! Entry point: initializes tracing, loads config, and (in later tasks)
//! launches the egui UI and OCR pipelines.

mod config;
mod session;
mod types;

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

    info!("mnm-dps-meter initialized (UI not yet implemented)");
    Ok(())
}
