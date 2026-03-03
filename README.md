# mnm-dps-meter

OCR-based damage meter for [Monsters & Memories](https://www.monstersandmemories.com/).

Reads on-screen combat text and mana state from the M&M game client using OCR, parses prose-format combat log lines into structured damage events, tracks mana regeneration via the mini character panel, and presents real-time cumulative statistics.

## Prerequisites

### Windows

- [Rust toolchain](https://rustup.rs/) (stable)
- No additional dependencies — uses the built-in WinRT OCR API

### Linux

- [Rust toolchain](https://rustup.rs/) (stable)
- Tesseract OCR and Leptonica development libraries:

```bash
# Debian/Ubuntu
sudo apt install tesseract-ocr libleptonica-dev libtesseract-dev

# Fedora
sudo dnf install tesseract leptonica-devel tesseract-devel

# Arch
sudo pacman -S tesseract leptonica
```

### macOS

- [Rust toolchain](https://rustup.rs/) (stable)
- Tesseract OCR via Homebrew:

```bash
brew install tesseract leptonica
```

## Build

```bash
# Debug build (without OCR — for development/testing)
cargo build

# With Tesseract OCR enabled (Linux/macOS, requires system libraries above)
cargo build --features tesseract

# With screen capture + OCR enabled (full functionality)
cargo build --features "xcap-capture,tesseract"

# Optimized release build (recommended for distribution)
cargo build --release --features "xcap-capture,tesseract"
```

The release build uses LTO and symbol stripping for a smaller, faster binary.

The `tesseract` and `xcap-capture` features are optional — the project compiles without them for development and testing. Enable them when building for actual use.

## Run

```bash
# Debug
cargo run

# Release
cargo run --release

# Or run the binary directly
./target/release/mnm-dps-meter
```

## Usage

1. Launch the application
2. Enter your character name (used to translate "You"/"Your" in combat text)
3. Configure the **Combat Log Region** — set the screen coordinates covering your M&M combat log window
4. Configure the **Mini Panel Region** — set the screen coordinates covering your character's HP/Mana/Endurance panel
5. Start playing — damage events and mana ticks will appear in real-time

**Note:** Regions use screen-absolute coordinates. If you move your game window, you will need to reconfigure your regions.

## Testing

```bash
cargo test
```
