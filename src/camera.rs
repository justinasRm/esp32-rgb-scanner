//! Capture one maximum-resolution JPEG per second for the web server.
use anyhow::{bail, Context, Result};
use esp_idf_sys::{camera, ESP_OK};
use std::{
    ptr::NonNull,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

/// Latest JPEG shared with the web server.
pub type LatestFrame = Arc<Mutex<Option<Vec<u8>>>>;

/// Keeps the camera driver's frame buffer valid until it is returned.
struct Frame(NonNull<camera::camera_fb_t>);

impl Frame {
    fn capture() -> Result<Self> {
        NonNull::new(unsafe { camera::esp_camera_fb_get() })
            .map(Self)
            .context("Camera did not return a frame")
    }

    fn jpeg(&self) -> &[u8] {
        let frame = unsafe { self.0.as_ref() };
        unsafe { std::slice::from_raw_parts(frame.buf, frame.len) }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        unsafe { camera::esp_camera_fb_return(self.0.as_ptr()) };
    }
}

fn initialize() -> Result<()> {
    // These pins are wired to the camera connector on the XIAO Sense board.
    let mut config: camera::camera_config_t = unsafe { std::mem::zeroed() };
    config.pin_pwdn = -1;
    config.pin_reset = -1;
    config.pin_xclk = 10;
    config.__bindgen_anon_1.pin_sccb_sda = 40;
    config.__bindgen_anon_2.pin_sccb_scl = 39;
    config.pin_d0 = 15;
    config.pin_d1 = 17;
    config.pin_d2 = 18;
    config.pin_d3 = 16;
    config.pin_d4 = 14;
    config.pin_d5 = 12;
    config.pin_d6 = 11;
    config.pin_d7 = 48;
    config.pin_vsync = 38;
    config.pin_href = 47;
    config.pin_pclk = 13;

    // Ask the sensor for its largest JPEG size; the driver limits this to the
    // maximum supported by the installed sensor.
    config.xclk_freq_hz = 20_000_000;
    config.ledc_timer = camera::ledc_timer_t_LEDC_TIMER_0;
    config.ledc_channel = camera::ledc_channel_t_LEDC_CHANNEL_0;
    config.pixel_format = camera::pixformat_t_PIXFORMAT_JPEG;
    config.frame_size = camera::framesize_t_FRAMESIZE_5MP;
    config.jpeg_quality = 12;
    config.fb_count = 1;
    config.fb_location = camera::camera_fb_location_t_CAMERA_FB_IN_PSRAM;
    config.grab_mode = camera::camera_grab_mode_t_CAMERA_GRAB_LATEST;

    let result = unsafe { camera::esp_camera_init(&config) };
    if result != ESP_OK {
        bail!("Camera initialization failed ({result:#x}). Check the camera ribbon.");
    }

    let sensor = unsafe { camera::esp_camera_sensor_get().as_ref() }
        .context("Camera sensor not detected")?;
    log::info!("Camera sensor PID: 0x{:04x}", sensor.id.PID);
    Ok(())
}

/// Start capturing one maximum-resolution JPEG every second.
pub fn run(latest: LatestFrame) -> Result<()> {
    initialize()?;
    loop {
        let started = Instant::now();
        let frame = Frame::capture()?;
        let driver_frame = unsafe { frame.0.as_ref() };
        log::info!(
            "Captured {}x{} frame",
            driver_frame.width,
            driver_frame.height
        );
        *latest.lock().unwrap() = Some(frame.jpeg().to_vec());
        drop(frame);
        thread::sleep(Duration::from_secs(1).saturating_sub(started.elapsed()));
    }
}
