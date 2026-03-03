//! Region selector overlay for drag-to-select screen regions.
//!
//! Provides a semi-transparent fullscreen overlay where users can drag
//! to select a rectangular region. The overlay is best-effort — transparent
//! fullscreen windows may not work on all platforms. Manual coordinate
//! entry in [`crate::ui`] is the reliable fallback.

use crate::types::CaptureRegion;
use eframe::egui;

/// Which capture region is being selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionTarget {
    /// The combat log capture region.
    CombatLog,
    /// The mini character panel capture region.
    MiniPanel,
}

impl std::fmt::Display for RegionTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegionTarget::CombatLog => write!(f, "Combat Log"),
            RegionTarget::MiniPanel => write!(f, "Mini Panel"),
        }
    }
}

/// State for an active drag-select region overlay.
///
/// When the user clicks a "Set Region" button, an `OverlayState` is created
/// and a new transparent fullscreen viewport opens for drag selection.
/// The user drags to draw a rectangle, and on release the coordinates
/// are captured as a [`CaptureRegion`].
pub struct OverlayState {
    /// Whether the overlay viewport is currently showing.
    pub active: bool,
    /// Which region is being selected.
    pub target: RegionTarget,
    /// Start position of the drag (viewport-local coordinates).
    drag_start: Option<egui::Pos2>,
    /// Current/end position of the drag.
    drag_end: Option<egui::Pos2>,
    /// The completed selection result (set on successful drag).
    pub result: Option<CaptureRegion>,
    /// Whether selection is complete (selected or cancelled).
    pub done: bool,
}

impl OverlayState {
    /// Create a new overlay for selecting the given target region.
    pub fn new(target: RegionTarget) -> Self {
        Self {
            active: true,
            target,
            drag_start: None,
            drag_end: None,
            result: None,
            done: false,
        }
    }

    /// Render the overlay viewport. Call from the main app's `update()`.
    ///
    /// Opens a transparent, fullscreen, always-on-top viewport. The user
    /// drags to select a rectangle. Press Escape to cancel.
    ///
    /// Note: Transparent fullscreen viewports are platform-dependent.
    /// If they don't work on the current platform, the manual coordinate
    /// entry fallback should be used instead.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.active {
            return;
        }

        let target = self.target;
        let OverlayState {
            active,
            drag_start,
            drag_end,
            result,
            done,
            ..
        } = self;

        let viewport_id = egui::ViewportId::from_hash_of("region_selector_overlay");
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("Select {} Region", target))
            .with_decorations(false)
            .with_transparent(true)
            .with_fullscreen(true)
            .with_always_on_top();

        ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            // Escape cancels the selection
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                *done = true;
                *active = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }

            let overlay_frame = egui::Frame::default()
                .fill(egui::Color32::from_black_alpha(100))
                .inner_margin(0.0)
                .outer_margin(0.0);

            egui::CentralPanel::default()
                .frame(overlay_frame)
                .show(ctx, |ui| {
                    let rect = ui.available_rect_before_wrap();
                    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());

                    if response.drag_started() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            *drag_start = Some(pos);
                            *drag_end = Some(pos);
                        }
                    }

                    if response.dragged() {
                        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                            *drag_end = Some(pos);
                        }
                    }

                    if response.drag_stopped() {
                        if let (Some(start), Some(end)) = (*drag_start, *drag_end) {
                            let x = start.x.min(end.x) as i32;
                            let y = start.y.min(end.y) as i32;
                            let w = (start.x - end.x).abs() as u32;
                            let h = (start.y - end.y).abs() as u32;
                            if w > 5 && h > 5 {
                                *result = Some(CaptureRegion {
                                    x,
                                    y,
                                    width: w,
                                    height: h,
                                });
                            }
                        }
                        *done = true;
                        *active = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }

                    // Draw the selection rectangle during drag
                    if let (Some(start), Some(end)) = (*drag_start, *drag_end) {
                        let sel_rect = egui::Rect::from_two_pos(start, end);
                        ui.painter().rect_filled(
                            sel_rect,
                            egui::CornerRadius::ZERO,
                            egui::Color32::from_rgba_premultiplied(0, 200, 255, 40),
                        );
                        ui.painter().rect_stroke(
                            sel_rect,
                            egui::CornerRadius::ZERO,
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 200, 255)),
                            egui::StrokeKind::Outside,
                        );

                        let w = (start.x - end.x).abs() as u32;
                        let h = (start.y - end.y).abs() as u32;
                        ui.painter().text(
                            sel_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("{}x{}", w, h),
                            egui::FontId::proportional(16.0),
                            egui::Color32::WHITE,
                        );
                    }

                    // Instruction text at top
                    ui.painter().text(
                        rect.center_top() + egui::vec2(0.0, 30.0),
                        egui::Align2::CENTER_TOP,
                        format!(
                            "Select {} Region \u{2014} Drag to select, Escape to cancel",
                            target
                        ),
                        egui::FontId::proportional(20.0),
                        egui::Color32::WHITE,
                    );
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_state_creation() {
        let state = OverlayState::new(RegionTarget::CombatLog);
        assert!(state.active);
        assert_eq!(state.target, RegionTarget::CombatLog);
        assert!(state.result.is_none());
        assert!(!state.done);
        assert!(state.drag_start.is_none());
        assert!(state.drag_end.is_none());
    }

    #[test]
    fn test_overlay_state_mini_panel() {
        let state = OverlayState::new(RegionTarget::MiniPanel);
        assert!(state.active);
        assert_eq!(state.target, RegionTarget::MiniPanel);
    }

    #[test]
    fn test_region_target_display() {
        assert_eq!(RegionTarget::CombatLog.to_string(), "Combat Log");
        assert_eq!(RegionTarget::MiniPanel.to_string(), "Mini Panel");
    }

    #[test]
    fn test_region_target_equality() {
        assert_eq!(RegionTarget::CombatLog, RegionTarget::CombatLog);
        assert_ne!(RegionTarget::CombatLog, RegionTarget::MiniPanel);
    }
}
