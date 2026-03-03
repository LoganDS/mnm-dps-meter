//! OCR engine implementations.
//!
//! Provides image-to-text OCR behind the [`OcrEngine`] trait. Two Tesseract
//! backends are available on Linux/macOS:
//!
//! - [`TesseractOcr`] — Uses the `leptess` crate (requires leptonica/tesseract
//!   system libraries at link time).
//! - [`TesseractCliOcr`] — Falls back to the `tesseract` CLI command via
//!   `std::process::Command`. Simpler dependency story but slightly slower.
//!
//! A mock implementation is provided for testing, and a cfg-gated placeholder
//! exists for a future Windows WinRT OCR backend.

use crate::types::OcrEngine;
use anyhow::{Context, Result};
use image::DynamicImage;
use std::io::Write;
use tracing::debug;

/// Tesseract-based OCR engine using the `leptess` crate.
///
/// Requires Tesseract and Leptonica system libraries to be installed.
/// On Linux: `apt install tesseract-ocr libleptonica-dev`
/// On macOS: `brew install tesseract`
#[cfg(feature = "leptess-ocr")]
pub struct TesseractOcr {
    /// Optional path to tessdata directory. None uses system default.
    data_path: Option<String>,
    /// Language code for Tesseract (e.g., "eng").
    lang: String,
}

#[cfg(feature = "leptess-ocr")]
impl TesseractOcr {
    /// Create a new Tesseract OCR engine with default settings.
    ///
    /// Uses the system default tessdata path and English language.
    pub fn new() -> Self {
        Self {
            data_path: None,
            lang: "eng".to_string(),
        }
    }

    /// Create a Tesseract OCR engine with a custom tessdata path and language.
    pub fn with_config(data_path: Option<String>, lang: String) -> Self {
        Self { data_path, lang }
    }

    /// Run a health check to verify the OCR engine is available and functional.
    ///
    /// Creates a small test image with known text characteristics and verifies
    /// Tesseract can initialize and process it. Returns an error with
    /// platform-specific installation guidance if the engine is unavailable.
    pub fn health_check(&self) -> Result<()> {
        // Try to initialize Tesseract — this is the most common failure point
        let data_path = self.data_path.as_deref();
        let mut lt = leptess::LepTess::new(data_path, &self.lang).map_err(|e| {
            let guidance = if cfg!(target_os = "linux") {
                "Tesseract not found. Install via: sudo apt install tesseract-ocr libleptonica-dev tesseract-ocr-eng"
            } else if cfg!(target_os = "macos") {
                "Tesseract not found. Install via: brew install tesseract"
            } else {
                "Tesseract not found. Please install Tesseract OCR for your platform."
            };
            anyhow::anyhow!("OCR engine initialization failed: {e}. {guidance}")
        })?;

        // Create a simple test image (white background, small size)
        // We just verify Tesseract can process an image without crashing.
        // A blank white image should produce empty or whitespace text.
        let test_img =
            image::RgbaImage::from_pixel(100, 30, image::Rgba([255, 255, 255, 255]));
        let test_image = DynamicImage::ImageRgba8(test_img);

        let temp_path = write_image_to_temp(&test_image)
            .context("Failed to create temp file for OCR health check")?;

        let loaded = lt.set_image(temp_path.to_str().unwrap_or(""));
        if !loaded {
            // Clean up temp file before returning error
            let _ = std::fs::remove_file(&temp_path);
            anyhow::bail!(
                "OCR engine failed to load test image. Tesseract may be misconfigured."
            );
        }

        lt.recognize();
        // We don't check the text content — a blank image may produce empty string
        // or whitespace. The point is that the pipeline didn't crash.
        let _ = lt.get_utf8_text();

        let _ = std::fs::remove_file(&temp_path);
        debug!("OCR health check passed");
        Ok(())
    }
}

#[cfg(feature = "leptess-ocr")]
impl OcrEngine for TesseractOcr {
    fn ocr_image(&self, image: &DynamicImage) -> Result<String> {
        let data_path = self.data_path.as_deref();
        let mut lt = leptess::LepTess::new(data_path, &self.lang)
            .map_err(|e| anyhow::anyhow!("Failed to initialize Tesseract: {e}"))?;

        let temp_path =
            write_image_to_temp(image).context("Failed to write image to temp file for OCR")?;

        let loaded = lt.set_image(temp_path.to_str().unwrap_or(""));
        if !loaded {
            let _ = std::fs::remove_file(&temp_path);
            anyhow::bail!("Tesseract failed to load image from temp file");
        }

        lt.recognize();
        let text = lt
            .get_utf8_text()
            .map_err(|e| anyhow::anyhow!("Failed to get UTF-8 text from Tesseract: {e}"))?;

        let _ = std::fs::remove_file(&temp_path);
        debug!("OCR extracted {} chars", text.len());
        Ok(text)
    }
}

/// Write a DynamicImage to a temporary PNG file and return the path.
///
/// The caller is responsible for cleaning up the temp file.
fn write_image_to_temp(image: &DynamicImage) -> Result<std::path::PathBuf> {
    let mut temp = tempfile::Builder::new()
        .prefix("mnm-ocr-")
        .suffix(".png")
        .tempfile()
        .context("Failed to create temp file")?;

    let rgba = image.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let encoder = image::codecs::png::PngEncoder::new(&mut temp);
    image::ImageEncoder::write_image(encoder, &rgba, w, h, image::ExtendedColorType::Rgba8)
        .context("Failed to encode image as PNG")?;

    temp.flush().context("Failed to flush temp file")?;

    // Persist the temp file so it isn't deleted when the NamedTempFile drops
    let (_, path) = temp
        .keep()
        .map_err(|e| anyhow::anyhow!("Failed to persist temp file: {e}"))?;

    Ok(path)
}

// --- Tesseract CLI fallback ---

/// Tesseract OCR engine using the `tesseract` command-line tool.
///
/// This is a simpler alternative to [`TesseractOcr`] that avoids the need for
/// leptonica/tesseract development libraries at link time. It shells out to the
/// `tesseract` binary, which must be installed and on PATH.
///
/// On Linux: `apt install tesseract-ocr tesseract-ocr-eng`
/// On macOS: `brew install tesseract`
pub struct TesseractCliOcr {
    /// Language code for Tesseract (e.g., "eng").
    lang: String,
}

impl TesseractCliOcr {
    /// Create a new CLI-based Tesseract OCR engine with English language.
    pub fn new() -> Self {
        Self {
            lang: "eng".to_string(),
        }
    }

    /// Create a CLI-based Tesseract OCR engine with a custom language.
    pub fn with_lang(lang: String) -> Self {
        Self { lang }
    }

    /// Run a health check to verify the `tesseract` CLI is available.
    ///
    /// Runs `tesseract --version` and verifies it exits successfully.
    /// Returns an error with platform-specific installation guidance if
    /// the command is not found.
    pub fn health_check(&self) -> Result<()> {
        let output = std::process::Command::new("tesseract")
            .arg("--version")
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let version = String::from_utf8_lossy(&out.stdout);
                debug!("Tesseract CLI health check passed: {}", version.trim());
                Ok(())
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                anyhow::bail!(
                    "Tesseract CLI returned error: {}. {}",
                    stderr.trim(),
                    platform_install_guidance()
                )
            }
            Err(e) => {
                anyhow::bail!(
                    "Tesseract CLI not found: {e}. {}", platform_install_guidance()
                )
            }
        }
    }
}

impl OcrEngine for TesseractCliOcr {
    fn ocr_image(&self, image: &DynamicImage) -> Result<String> {
        let temp_path =
            write_image_to_temp(image).context("Failed to write image to temp file for OCR")?;

        let temp_path_str = temp_path
            .to_str()
            .context("Temp file path is not valid UTF-8")?;

        // tesseract <input> stdout -l <lang> outputs OCR text to stdout
        let output = std::process::Command::new("tesseract")
            .arg(temp_path_str)
            .arg("stdout")
            .arg("-l")
            .arg(&self.lang)
            .output()
            .context("Failed to run tesseract CLI")?;

        let _ = std::fs::remove_file(&temp_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Tesseract CLI failed: {}", stderr.trim());
        }

        let text = String::from_utf8(output.stdout)
            .context("Tesseract output is not valid UTF-8")?;

        debug!("OCR (CLI) extracted {} chars", text.len());
        Ok(text)
    }
}

/// Return platform-specific installation guidance for Tesseract.
fn platform_install_guidance() -> &'static str {
    if cfg!(target_os = "linux") {
        "Install via: sudo apt install tesseract-ocr tesseract-ocr-eng"
    } else if cfg!(target_os = "macos") {
        "Install via: brew install tesseract"
    } else if cfg!(target_os = "windows") {
        "Install Tesseract from https://github.com/UB-Mannheim/tesseract/wiki and add to PATH"
    } else {
        "Please install Tesseract OCR for your platform"
    }
}

// --- Windows WinRT OCR placeholder ---

/// Placeholder for Windows WinRT OCR engine.
///
/// TODO: Implement using `windows-rs` crate for hardware-accelerated OCR
/// via the Windows.Media.Ocr namespace. This is expected to be significantly
/// faster than Tesseract on Windows.
#[cfg(target_os = "windows")]
pub struct WinRtOcr;

#[cfg(target_os = "windows")]
impl WinRtOcr {
    /// Create a new WinRT OCR engine.
    pub fn new() -> Self {
        Self
    }

    /// Run a health check for the WinRT OCR engine.
    pub fn health_check(&self) -> Result<()> {
        tracing::warn!("WinRT OCR backend is not yet implemented");
        anyhow::bail!("WinRT OCR backend is not yet implemented. Use Tesseract as a fallback.")
    }
}

#[cfg(target_os = "windows")]
impl OcrEngine for WinRtOcr {
    fn ocr_image(&self, _image: &DynamicImage) -> Result<String> {
        anyhow::bail!("WinRT OCR backend is not yet implemented")
    }
}

// --- Mock OCR Engine ---

/// Mock OCR engine for testing.
///
/// Returns pre-configured text strings in sequence, or a single repeated
/// string. Useful for deterministic pipeline and parser tests.
pub struct MockOcrEngine {
    /// Text responses to return, in order. If exhausted, returns empty string.
    responses: std::sync::Mutex<Vec<String>>,
}

impl MockOcrEngine {
    /// Create a mock OCR engine that returns the given text on every call.
    pub fn with_text(text: &str) -> Self {
        Self {
            responses: std::sync::Mutex::new(vec![text.to_string()]),
        }
    }

    /// Create a mock OCR engine that returns texts from the sequence in order.
    ///
    /// Once the sequence is exhausted, returns an empty string.
    pub fn with_sequence(texts: Vec<String>) -> Self {
        let mut reversed = texts;
        reversed.reverse(); // Reverse so we can pop from the end efficiently
        Self {
            responses: std::sync::Mutex::new(reversed),
        }
    }
}

impl OcrEngine for MockOcrEngine {
    fn ocr_image(&self, _image: &DynamicImage) -> Result<String> {
        let mut responses = self.responses.lock().map_err(|e| {
            anyhow::anyhow!("MockOcrEngine lock poisoned: {e}")
        })?;
        if responses.len() <= 1 {
            // Return the last remaining response (or empty if none)
            Ok(responses.first().cloned().unwrap_or_default())
        } else {
            // Pop from the back (which was the front before reversal)
            Ok(responses.pop().unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_ocr_returns_configured_text() {
        let mock = MockOcrEngine::with_text("Narky pierces a cultist for 7 points of damage.");
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            10,
            10,
            image::Rgba([0, 0, 0, 255]),
        ));

        let result = mock.ocr_image(&img).unwrap();
        assert_eq!(result, "Narky pierces a cultist for 7 points of damage.");

        // Calling again returns the same text
        let result2 = mock.ocr_image(&img).unwrap();
        assert_eq!(result2, "Narky pierces a cultist for 7 points of damage.");
    }

    #[test]
    fn mock_ocr_returns_sequence_in_order() {
        let mock = MockOcrEngine::with_sequence(vec![
            "first line".to_string(),
            "second line".to_string(),
            "third line".to_string(),
        ]);
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            10,
            10,
            image::Rgba([0, 0, 0, 255]),
        ));

        assert_eq!(mock.ocr_image(&img).unwrap(), "first line");
        assert_eq!(mock.ocr_image(&img).unwrap(), "second line");
        assert_eq!(mock.ocr_image(&img).unwrap(), "third line");
        // After exhaustion, returns last element
        assert_eq!(mock.ocr_image(&img).unwrap(), "third line");
    }

    #[test]
    fn mock_ocr_empty_sequence_returns_empty() {
        let mock = MockOcrEngine::with_sequence(vec![]);
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            10,
            10,
            image::Rgba([0, 0, 0, 255]),
        ));

        assert_eq!(mock.ocr_image(&img).unwrap(), "");
    }

    #[test]
    fn mock_ocr_is_send() {
        // Verify MockOcrEngine satisfies Send bound from OcrEngine trait
        fn assert_send<T: Send>() {}
        assert_send::<MockOcrEngine>();
    }

    #[test]
    fn tesseract_cli_ocr_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<TesseractCliOcr>();
    }

    #[test]
    fn write_image_to_temp_creates_file() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            20,
            20,
            image::Rgba([128, 128, 128, 255]),
        ));
        let path = write_image_to_temp(&img).unwrap();
        assert!(path.exists());
        assert!(path.extension().map_or(false, |ext| ext == "png"));

        // Clean up
        let _ = std::fs::remove_file(&path);
    }
}
