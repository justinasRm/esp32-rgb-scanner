# XIAO ESP32S3 Sense color meter

Minimal Rust camera color meter: detects the camera sensor, requests its maximum
resolution (up to 5 MP), and captures one JPEG per second. The ESP32 decodes a
1/8-size copy of each JPEG, averages the centered square covering 5% of the image
area, and serves the RGB values as small JSON at `/rgb`. Open `/rgb-only` for a
separate auto-updating color page that downloads no camera images, or open `/`
for the camera preview and reading.

The code is split into small parts:

- `src/main.rs` starts the camera, Wi-Fi, and web server.
- `src/camera.rs` configures the camera, captures one JPEG per second, and
  calculates the center RGB average on the ESP32.
- `src/network.rs` joins the phone hotspot.
- `src/web.rs` serves the preview, RGB-only page, latest JPEG, and RGB JSON.
- `src/app.js` displays the picture and ESP32-calculated RGB reading.
- `src/rgb_only.html` and `src/rgb_only.js` show the auto-updating color without
  loading images.
- `src/preview.html` is the page layout.

Hotspot settings live in the gitignored `wifi-config.rs`; copy
`wifi-config.example.rs` for a fresh checkout. Changing credentials requires
rebuilding and uploading.

Build and flash with `cargo run --release`. The browser displays the frame size,
average RGB values, and hex code. The ESP32 prints the sensor ID and frame size
to the serial log.
