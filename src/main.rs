//! mnm-dps-meter — OCR-based damage meter for Monsters & Memories.
//!
//! Entry point: initializes tracing, loads config, runs OCR health check,
//! creates shared session state, spawns pipeline threads, and launches
//! the egui/eframe UI window.

mod capture;
mod config;
mod dedup;
mod ocr;
mod overlay;
mod panel;
mod parser;
mod pipeline;
mod session;
mod tick;
mod types;
mod ui;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use config::AppConfig;
use session::new_shared_session;
use tracing::{info, warn};
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

    // Create pipeline communication channel
    let (sender, receiver) = std::sync::mpsc::channel();
    let paused = Arc::new(AtomicBool::new(false));

    // OCR health check on startup (invariant 15)
    let ocr_error = match pipeline::run_ocr_health_check() {
        Ok(()) => {
            info!("OCR health check passed");
            None
        }
        Err(e) => {
            warn!("OCR health check failed: {}", e);
            Some(e)
        }
    };

    // Spawn pipeline threads if OCR is available and regions are configured
    let mut combat_pipeline = None;
    let mut mini_panel_pipeline = None;

    if ocr_error.is_none() {
        if let Some(region) = config.combat_log_region {
            match pipeline::spawn_combat_log_pipeline(
                region,
                config.combat_capture_interval_ms,
                config.character_name.clone(),
                sender.clone(),
                Arc::clone(&paused),
            ) {
                Ok(handle) => {
                    combat_pipeline = Some(handle);
                    info!("Combat log pipeline started");
                }
                Err(e) => warn!("Failed to start combat log pipeline: {}", e),
            }
        }

        if let Some(region) = config.mini_panel_region {
            match pipeline::spawn_mini_panel_pipeline(
                region,
                config.panel_capture_interval_ms,
                sender.clone(),
                Arc::clone(&paused),
            ) {
                Ok(handle) => {
                    mini_panel_pipeline = Some(handle);
                    info!("Mini panel pipeline started");
                }
                Err(e) => warn!("Failed to start mini panel pipeline: {}", e),
            }
        }
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
            Ok(Box::new(DamageMeterApp::new(
                session,
                config,
                receiver,
                sender,
                paused,
                combat_pipeline,
                mini_panel_pipeline,
                ocr_error,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    info!("mnm-dps-meter shutting down");
    Ok(())
}
