# BLE hello sender (Rust)

## Flash and view serial output

Install the Rust ESP toolchain and ESP-IDF 6.1, then connect the XIAO ESP32-S3
Sense by USB. In this folder, run:

    cargo run --release

Choose the board's /dev/cu.usbmodem... port if prompted. List ports with
ls /dev/cu.\*

The command builds, flashes, and opens the serial monitor. Close the monitor
with Ctrl+].

The sender appears as ESP32-HELLO and repeatedly advertises hello at the
250 ms interval. Every five seconds, serial output reports advertising state,
uptime, and free heap.
