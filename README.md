# XIAO ESP32S3 Sense camera server

Minimal Rust camera server: connects to the configured Wi-Fi network, captures
one maximum-resolution JPEG per second, and serves an auto-updating preview at
`/`. The camera sensor sets the actual maximum resolution.

The code is split into small parts:

- `src/main.rs` starts the camera, Wi-Fi, and web server.
- `src/camera.rs` configures the camera and captures one JPEG per second.
- `src/network.rs` joins the phone hotspot.
- `src/web.rs` serves the preview page and latest JPEG.
- `src/preview.html` refreshes the image once per second.

Hotspot settings live in the gitignored `wifi-config.rs`; copy
`wifi-config.example.rs` for a fresh checkout. Changing credentials requires
rebuilding and uploading.

Build and flash with `cargo run --release`, then open the URL printed in the
serial log. The ESP32 prints the detected sensor and captured frame size.
