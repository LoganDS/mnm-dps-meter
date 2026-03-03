//! Pipeline orchestration for the mnm-dps-meter application.
//!
//! Wires together screen capture, OCR, deduplication, parsing, and session
//! accumulation into two independent pipelines: combat log and mini panel.
//! Each pipeline runs on its own dedicated thread, sending results to the
//! UI via mpsc channels.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::{debug, warn};

use crate::dedup::LineDeduplicator;
use crate::ocr::TesseractCliOcr;
use crate::panel::PanelParser;
use crate::parser::CombatLogParser;
use crate::tick::TickDetector;
use crate::types::{CaptureRegion, OcrEngine, PipelineMessage, ScreenCapture};

/// Handle to a running pipeline thread.
///
/// Holds the shutdown signal and join handle for lifecycle management.
/// Call [`signal_stop`](PipelineHandle::signal_stop) for async shutdown
/// or [`stop`](PipelineHandle::stop) for synchronous shutdown with join.
pub struct PipelineHandle {
    /// Set to `true` to signal the pipeline thread to exit.
    pub shutdown: Arc<AtomicBool>,
    /// Thread join handle. Taken (set to None) when joined.
    handle: Option<JoinHandle<()>>,
}

impl PipelineHandle {
    /// Signal the pipeline thread to stop without waiting for it to exit.
    pub fn signal_stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    /// Signal the pipeline thread to stop and wait for it to exit.
    pub fn stop(&mut self) {
        self.signal_stop();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Combat log OCR pipeline.
///
/// Captures the combat log screen region, runs OCR, deduplicates lines,
/// parses damage events, and sends them to the session accumulator via
/// an mpsc channel.
struct CombatLogPipeline {
    capture: Box<dyn ScreenCapture>,
    ocr: Box<dyn OcrEngine>,
    region: CaptureRegion,
    interval: Duration,
    dedup: LineDeduplicator,
    parser: CombatLogParser,
}

impl CombatLogPipeline {
    /// Create a new combat log pipeline.
    fn new(
        capture: Box<dyn ScreenCapture>,
        ocr: Box<dyn OcrEngine>,
        region: CaptureRegion,
        interval_ms: u32,
        character_name: Option<String>,
    ) -> Self {
        Self {
            capture,
            ocr,
            region,
            interval: Duration::from_millis(interval_ms as u64),
            dedup: LineDeduplicator::new(),
            parser: CombatLogParser::new(character_name),
        }
    }

    /// Run the pipeline loop until shutdown is signaled.
    ///
    /// On each iteration:
    /// 1. Check shutdown and paused flags
    /// 2. Capture the screen region
    /// 3. Run OCR to extract text
    /// 4. Split into lines, deduplicate each
    /// 5. Parse each new line for damage events
    /// 6. Send batch of events via channel
    ///
    /// **Overrun policy:** If OCR takes longer than the capture interval,
    /// the next capture is skipped (no queue buildup). The timer resets
    /// after each OCR completion.
    fn run(
        mut self,
        sender: mpsc::Sender<PipelineMessage>,
        shutdown: Arc<AtomicBool>,
        paused: Arc<AtomicBool>,
    ) {
        debug!("Combat log pipeline started (interval={:?})", self.interval);

        loop {
            if shutdown.load(Ordering::Relaxed) {
                debug!("Combat log pipeline shutting down");
                break;
            }

            if paused.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
                continue;
            }

            let start = Instant::now();

            // Capture screen region
            let image = match self.capture.capture_region(&self.region) {
                Ok(img) => img,
                Err(e) => {
                    warn!("Combat log capture failed: {}", e);
                    Self::sleep_remaining(start, self.interval);
                    continue;
                }
            };

            // OCR: image → text
            let text = match self.ocr.ocr_image(&image) {
                Ok(t) => t,
                Err(e) => {
                    warn!("Combat log OCR failed: {}", e);
                    Self::sleep_remaining(start, self.interval);
                    continue;
                }
            };

            // Split lines → dedup → parse
            let mut events = Vec::new();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if !self.dedup.is_new(line) {
                    continue;
                }
                if let Some(event) = self.parser.parse_line(line) {
                    events.push(event);
                }
            }

            // Send batch of events
            if !events.is_empty() {
                debug!("Combat log pipeline: parsed {} events", events.len());
                if sender.send(PipelineMessage::DamageEvents(events)).is_err() {
                    debug!("Combat log pipeline: receiver dropped, exiting");
                    break;
                }
            }

            // Overrun handling: skip sleep if processing exceeded interval
            Self::sleep_remaining(start, self.interval);
        }
    }

    /// Sleep for the remaining interval time, or skip if already overrun.
    fn sleep_remaining(start: Instant, interval: Duration) {
        let elapsed = start.elapsed();
        if elapsed < interval {
            thread::sleep(interval - elapsed);
        } else {
            debug!(
                "Combat log pipeline overrun: took {:?} (interval={:?})",
                elapsed, interval
            );
        }
    }
}

/// Mini character panel OCR pipeline.
///
/// Captures the mini panel screen region, runs OCR, parses vital stats
/// (HP, Mana, Endurance), detects mana ticks, and sends results to
/// the session accumulator via an mpsc channel.
struct MiniPanelPipeline {
    capture: Box<dyn ScreenCapture>,
    ocr: Box<dyn OcrEngine>,
    region: CaptureRegion,
    interval: Duration,
    panel_parser: PanelParser,
    tick_detector: TickDetector,
}

impl MiniPanelPipeline {
    /// Create a new mini panel pipeline.
    fn new(
        capture: Box<dyn ScreenCapture>,
        ocr: Box<dyn OcrEngine>,
        region: CaptureRegion,
        interval_ms: u32,
    ) -> Self {
        Self {
            capture,
            ocr,
            region,
            interval: Duration::from_millis(interval_ms as u64),
            panel_parser: PanelParser::new(),
            tick_detector: TickDetector::new(),
        }
    }

    /// Run the pipeline loop until shutdown is signaled.
    ///
    /// On each iteration:
    /// 1. Check shutdown and paused flags
    /// 2. Capture the screen region
    /// 3. Run OCR to extract text
    /// 4. Parse panel text for vital stats
    /// 5. Send vitals update via channel
    /// 6. Detect mana tick and send if changed
    fn run(
        mut self,
        sender: mpsc::Sender<PipelineMessage>,
        shutdown: Arc<AtomicBool>,
        paused: Arc<AtomicBool>,
    ) {
        debug!("Mini panel pipeline started (interval={:?})", self.interval);

        loop {
            if shutdown.load(Ordering::Relaxed) {
                debug!("Mini panel pipeline shutting down");
                break;
            }

            if paused.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
                continue;
            }

            let start = Instant::now();

            // Capture screen region
            let image = match self.capture.capture_region(&self.region) {
                Ok(img) => img,
                Err(e) => {
                    warn!("Mini panel capture failed: {}", e);
                    Self::sleep_remaining(start, self.interval);
                    continue;
                }
            };

            // OCR: image → text
            let text = match self.ocr.ocr_image(&image) {
                Ok(t) => t,
                Err(e) => {
                    warn!("Mini panel OCR failed: {}", e);
                    Self::sleep_remaining(start, self.interval);
                    continue;
                }
            };

            // Parse panel stats
            let stats = self.panel_parser.parse_panel_text(&text);

            // Extract mana values before sending (stats is moved by send)
            let mana_current = stats.mana_current;
            let mana_max = stats.mana_max;

            // Send vitals update
            if sender.send(PipelineMessage::ManaUpdate(stats)).is_err() {
                debug!("Mini panel pipeline: receiver dropped, exiting");
                break;
            }

            // Detect mana tick
            if let (Some(current), Some(max)) = (mana_current, mana_max) {
                if let Some(tick_event) = self.tick_detector.process_reading(current, max) {
                    debug!("Mana tick detected: delta={}", tick_event.delta);
                    if sender.send(PipelineMessage::ManaTick(tick_event)).is_err() {
                        debug!("Mini panel pipeline: receiver dropped, exiting");
                        break;
                    }
                }
            }

            // Overrun handling
            Self::sleep_remaining(start, self.interval);
        }
    }

    /// Sleep for the remaining interval time, or skip if already overrun.
    fn sleep_remaining(start: Instant, interval: Duration) {
        let elapsed = start.elapsed();
        if elapsed < interval {
            thread::sleep(interval - elapsed);
        } else {
            debug!(
                "Mini panel pipeline overrun: took {:?} (interval={:?})",
                elapsed, interval
            );
        }
    }
}

// --- Factory functions ---

/// Run the OCR engine health check on startup.
///
/// Creates a temporary OCR engine and verifies it can process images.
/// Returns an error message with platform-specific guidance if the
/// engine is unavailable or misconfigured.
pub fn run_ocr_health_check() -> Result<(), String> {
    #[cfg(feature = "leptess-ocr")]
    {
        let ocr = crate::ocr::TesseractOcr::new();
        return ocr.health_check().map_err(|e| e.to_string());
    }
    #[cfg(not(feature = "leptess-ocr"))]
    {
        let ocr = TesseractCliOcr::new();
        ocr.health_check().map_err(|e| e.to_string())
    }
}

/// Create a new OCR engine instance using the best available backend.
///
/// Prefers `leptess` (Tesseract library binding) when the feature is enabled,
/// falling back to the Tesseract CLI wrapper.
pub fn create_ocr_engine() -> Box<dyn OcrEngine> {
    #[cfg(feature = "leptess-ocr")]
    {
        return Box::new(crate::ocr::TesseractOcr::new());
    }
    #[cfg(not(feature = "leptess-ocr"))]
    {
        Box::new(TesseractCliOcr::new())
    }
}

/// Create a new screen capture backend.
///
/// Returns an error if no screen capture backend is available. Build with
/// `--features xcap-capture` to enable native screen capture.
pub fn create_screen_capture() -> Result<Box<dyn ScreenCapture>, String> {
    #[cfg(feature = "xcap-capture")]
    {
        return Ok(Box::new(crate::capture::XCapScreenCapture::new()));
    }
    #[cfg(not(feature = "xcap-capture"))]
    {
        Err(
            "Screen capture not available. Build with --features xcap-capture \
             (requires libxcb on Linux)."
                .to_string(),
        )
    }
}

/// Spawn a combat log pipeline thread.
///
/// Creates the screen capture and OCR backends, builds the pipeline, and
/// spawns it on a dedicated thread. Returns a [`PipelineHandle`] for
/// lifecycle management.
pub fn spawn_combat_log_pipeline(
    region: CaptureRegion,
    interval_ms: u32,
    character_name: Option<String>,
    sender: mpsc::Sender<PipelineMessage>,
    paused: Arc<AtomicBool>,
) -> Result<PipelineHandle, String> {
    let capture = create_screen_capture()?;
    let ocr = create_ocr_engine();

    let pipeline = CombatLogPipeline::new(capture, ocr, region, interval_ms, character_name);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    let handle = thread::Builder::new()
        .name("combat-log-pipeline".to_string())
        .spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        })
        .map_err(|e| format!("Failed to spawn combat log thread: {}", e))?;

    Ok(PipelineHandle {
        shutdown,
        handle: Some(handle),
    })
}

/// Spawn a mini panel pipeline thread.
///
/// Creates the screen capture and OCR backends, builds the pipeline, and
/// spawns it on a dedicated thread. Returns a [`PipelineHandle`] for
/// lifecycle management.
pub fn spawn_mini_panel_pipeline(
    region: CaptureRegion,
    interval_ms: u32,
    sender: mpsc::Sender<PipelineMessage>,
    paused: Arc<AtomicBool>,
) -> Result<PipelineHandle, String> {
    let capture = create_screen_capture()?;
    let ocr = create_ocr_engine();

    let pipeline = MiniPanelPipeline::new(capture, ocr, region, interval_ms);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    let handle = thread::Builder::new()
        .name("mini-panel-pipeline".to_string())
        .spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        })
        .map_err(|e| format!("Failed to spawn mini panel thread: {}", e))?;

    Ok(PipelineHandle {
        shutdown,
        handle: Some(handle),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::MockScreenCapture;
    use crate::ocr::MockOcrEngine;

    #[test]
    fn test_combat_log_pipeline_parses_events() {
        let capture = MockScreenCapture::with_test_image(100, 50);
        let ocr = MockOcrEngine::with_text(
            "Narky pierces a cultist for 7 points of damage.\n\
             Creature slashes a cultist for 12 points of damage.",
        );

        let pipeline = CombatLogPipeline::new(
            Box::new(capture),
            Box::new(ocr),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 100,
                height: 50,
            },
            1000,
            None,
        );

        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        let shutdown_clone = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        });

        // Wait for first batch of events
        let msg = receiver.recv_timeout(Duration::from_secs(5)).unwrap();

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        match msg {
            PipelineMessage::DamageEvents(events) => {
                assert_eq!(events.len(), 2);
                assert_eq!(events[0].source_player, "Narky");
                assert_eq!(events[0].attack_name, "pierce");
                assert_eq!(events[0].damage_value, 7);
                assert_eq!(events[1].source_player, "Creature");
                assert_eq!(events[1].attack_name, "slash");
                assert_eq!(events[1].damage_value, 12);
            }
            other => panic!("Expected DamageEvents, got {:?}", other),
        }
    }

    #[test]
    fn test_combat_log_pipeline_with_character_name() {
        let capture = MockScreenCapture::with_test_image(100, 50);
        let ocr = MockOcrEngine::with_text(
            "You pierce a cultist for 7 points of damage.",
        );

        let pipeline = CombatLogPipeline::new(
            Box::new(capture),
            Box::new(ocr),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 100,
                height: 50,
            },
            1000,
            Some("Narky".to_string()),
        );

        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        let shutdown_clone = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        });

        let msg = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        match msg {
            PipelineMessage::DamageEvents(events) => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].source_player, "Narky");
            }
            other => panic!("Expected DamageEvents, got {:?}", other),
        }
    }

    #[test]
    fn test_mini_panel_pipeline_sends_vitals() {
        let capture = MockScreenCapture::with_test_image(100, 50);
        let ocr = MockOcrEngine::with_text(
            "Health 442/454\nMana 185/470\nEndurance 80/100",
        );

        let pipeline = MiniPanelPipeline::new(
            Box::new(capture),
            Box::new(ocr),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 100,
                height: 50,
            },
            1000,
        );

        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        let shutdown_clone = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        });

        // First message should be ManaUpdate
        let msg = receiver.recv_timeout(Duration::from_secs(5)).unwrap();

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        match msg {
            PipelineMessage::ManaUpdate(stats) => {
                assert_eq!(stats.hp_current, Some(442));
                assert_eq!(stats.hp_max, Some(454));
                assert_eq!(stats.mana_current, Some(185));
                assert_eq!(stats.mana_max, Some(470));
                assert_eq!(stats.endurance_current, Some(80));
                assert_eq!(stats.endurance_max, Some(100));
            }
            other => panic!("Expected ManaUpdate, got {:?}", other),
        }
    }

    #[test]
    fn test_mini_panel_pipeline_detects_mana_tick() {
        let capture = MockScreenCapture::with_test_image(100, 50);
        // First reading: Mana 185/470, second reading: Mana 200/470
        let ocr = MockOcrEngine::with_sequence(vec![
            "Health 442/454\nMana 185/470\nEndurance 80/100".to_string(),
            "Health 442/454\nMana 200/470\nEndurance 80/100".to_string(),
        ]);

        let pipeline = MiniPanelPipeline::new(
            Box::new(capture),
            Box::new(ocr),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 100,
                height: 50,
            },
            100, // Short interval for test speed
        );

        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        let shutdown_clone = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        });

        // Collect messages until we find a ManaTick
        let mut found_tick = false;
        for _ in 0..20 {
            match receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(PipelineMessage::ManaTick(tick)) => {
                    assert_eq!(tick.previous_mana, 185);
                    assert_eq!(tick.current_mana, 200);
                    assert_eq!(tick.delta, 15);
                    assert_eq!(tick.event_type, crate::types::ManaEventType::Regen);
                    found_tick = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        assert!(found_tick, "Should have detected a mana regen tick");
    }

    #[test]
    fn test_pipeline_respects_shutdown() {
        let capture = MockScreenCapture::with_test_image(10, 10);
        let ocr = MockOcrEngine::with_text("");

        let pipeline = CombatLogPipeline::new(
            Box::new(capture),
            Box::new(ocr),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            100,
            None,
        );

        let (sender, _receiver) = mpsc::channel();
        // Pre-set shutdown before starting
        let shutdown = Arc::new(AtomicBool::new(true));
        let paused = Arc::new(AtomicBool::new(false));

        let shutdown_clone = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        });

        // Should exit immediately
        handle.join().unwrap();
    }

    #[test]
    fn test_pipeline_respects_pause() {
        let capture = MockScreenCapture::with_test_image(10, 10);
        let ocr = MockOcrEngine::with_text(
            "Narky pierces a cultist for 7 points of damage.",
        );

        let pipeline = CombatLogPipeline::new(
            Box::new(capture),
            Box::new(ocr),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            100,
            None,
        );

        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(true)); // Start paused

        let shutdown_clone = Arc::clone(&shutdown);
        let paused_clone = Arc::clone(&paused);
        let handle = thread::spawn(move || {
            pipeline.run(sender, shutdown_clone, paused_clone);
        });

        // Should not receive messages while paused
        assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());

        // Unpause
        paused.store(false, Ordering::Relaxed);

        // Should now receive events
        let msg = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(msg, PipelineMessage::DamageEvents(_)));

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();
    }

    #[test]
    fn test_combat_log_pipeline_deduplicates() {
        let capture = MockScreenCapture::with_test_image(10, 10);
        // Same text repeated — dedup should prevent duplicate events
        let ocr = MockOcrEngine::with_text(
            "Narky pierces a cultist for 7 points of damage.",
        );

        let pipeline = CombatLogPipeline::new(
            Box::new(capture),
            Box::new(ocr),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            100,
            None,
        );

        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        let shutdown_clone = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        });

        // First message should have the event
        let msg = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        match msg {
            PipelineMessage::DamageEvents(events) => {
                assert_eq!(events.len(), 1);
            }
            other => panic!("Expected DamageEvents, got {:?}", other),
        }

        // Wait a bit — next iterations should produce no events (deduped)
        // because the same text is returned and it's within the 2-second window
        thread::sleep(Duration::from_millis(300));
        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        // Drain remaining: should be no more DamageEvents messages
        let mut extra_events = 0;
        while let Ok(msg) = receiver.try_recv() {
            if let PipelineMessage::DamageEvents(events) = msg {
                extra_events += events.len();
            }
        }
        assert_eq!(extra_events, 0, "Deduplication should prevent repeat events");
    }

    #[test]
    fn test_pipeline_handle_stop() {
        let capture = MockScreenCapture::with_test_image(10, 10);
        let ocr = MockOcrEngine::with_text("");

        let pipeline = CombatLogPipeline::new(
            Box::new(capture),
            Box::new(ocr),
            CaptureRegion {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            100,
            None,
        );

        let (sender, _receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        let shutdown_clone = Arc::clone(&shutdown);
        let join_handle = thread::spawn(move || {
            pipeline.run(sender, shutdown_clone, paused);
        });

        let mut handle = PipelineHandle {
            shutdown,
            handle: Some(join_handle),
        };

        // stop() should signal and join cleanly
        handle.stop();
        assert!(handle.handle.is_none());
    }
}
