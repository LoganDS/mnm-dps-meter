//! Screen capture implementations.
//!
//! Provides cross-platform screen region capture behind the [`ScreenCapture`] trait.
//! The primary implementation uses the `xcap` crate (feature-gated behind
//! `xcap-capture`) to capture a monitor and crop to the requested region.
//! A mock implementation is provided for testing.

use crate::types::{CaptureRegion, ScreenCapture};
use anyhow::Result;
use image::DynamicImage;

/// Screen capture backend using the `xcap` crate.
///
/// Captures the full monitor containing the target region, then crops to
/// the requested rectangle. Region coordinates are screen-absolute.
///
/// Requires the `xcap-capture` feature and `libxcb` system library on Linux.
#[cfg(feature = "xcap-capture")]
pub struct XCapScreenCapture;

#[cfg(feature = "xcap-capture")]
impl XCapScreenCapture {
    /// Create a new xcap-based screen capture backend.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "xcap-capture")]
impl ScreenCapture for XCapScreenCapture {
    fn capture_region(&self, region: &CaptureRegion) -> Result<DynamicImage> {
        use anyhow::Context;
        use tracing::debug;
        use xcap::Monitor;

        // Find the monitor that contains the top-left corner of the region
        let monitor = Monitor::from_point(region.x, region.y)
            .context("Region is offscreen — no monitor found at the specified coordinates")?;

        let mon_x = monitor.x().context("Failed to get monitor x")?;
        let mon_y = monitor.y().context("Failed to get monitor y")?;
        let mon_w = monitor.width().context("Failed to get monitor width")?;
        let mon_h = monitor.height().context("Failed to get monitor height")?;

        // Calculate crop coordinates relative to the monitor
        let crop_x = (region.x - mon_x) as u32;
        let crop_y = (region.y - mon_y) as u32;

        // Validate the region fits within the monitor
        if crop_x + region.width > mon_w || crop_y + region.height > mon_h {
            anyhow::bail!(
                "Region extends beyond monitor bounds: region ({}, {}, {}x{}) vs monitor ({}x{} at {},{})",
                region.x, region.y, region.width, region.height,
                mon_w, mon_h, mon_x, mon_y
            );
        }

        debug!(
            "Capturing monitor '{}' at ({},{}) {}x{}, cropping to ({},{}) {}x{}",
            monitor.name().unwrap_or_default(),
            mon_x, mon_y, mon_w, mon_h,
            crop_x, crop_y, region.width, region.height
        );

        let screenshot = monitor
            .capture_image()
            .context("Failed to capture monitor image")?;

        let full_image = DynamicImage::ImageRgba8(screenshot);
        let cropped = full_image.crop_imm(crop_x, crop_y, region.width, region.height);

        Ok(cropped)
    }
}

/// Mock screen capture for testing.
///
/// Returns a pre-configured image on every capture call, ignoring the
/// requested region. Useful for deterministic pipeline tests.
pub struct MockScreenCapture {
    image: DynamicImage,
}

impl MockScreenCapture {
    /// Create a mock capture backend that always returns the given image.
    pub fn new(image: DynamicImage) -> Self {
        Self { image }
    }

    /// Create a mock capture backend with a small solid-color test image.
    pub fn with_test_image(width: u32, height: u32) -> Self {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));
        Self {
            image: DynamicImage::ImageRgba8(img),
        }
    }
}

impl ScreenCapture for MockScreenCapture {
    fn capture_region(&self, _region: &CaptureRegion) -> Result<DynamicImage> {
        Ok(self.image.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CaptureRegion;

    #[test]
    fn mock_capture_returns_configured_image() {
        let mock = MockScreenCapture::with_test_image(100, 50);
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        };
        let result = mock.capture_region(&region);
        assert!(result.is_ok());
        let img = result.unwrap();
        assert_eq!(img.width(), 100);
        assert_eq!(img.height(), 50);
    }

    #[test]
    fn mock_capture_ignores_region_coordinates() {
        let mock = MockScreenCapture::with_test_image(200, 100);
        let region = CaptureRegion {
            x: 999,
            y: 999,
            width: 50,
            height: 50,
        };
        let result = mock.capture_region(&region);
        assert!(result.is_ok());
        // Mock returns the full configured image regardless of region
        let img = result.unwrap();
        assert_eq!(img.width(), 200);
        assert_eq!(img.height(), 100);
    }

    #[test]
    fn mock_capture_with_custom_image() {
        let custom = image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255]));
        let mock = MockScreenCapture::new(DynamicImage::ImageRgba8(custom));
        let region = CaptureRegion {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        let img = mock.capture_region(&region).unwrap();
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 64);
    }

    #[test]
    fn mock_capture_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<MockScreenCapture>();
    }
}
