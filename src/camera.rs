//! XIAO ESP32S3 Sense camera setup and one-frame-per-second capture.
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
/// Latest average RGB color from the centered five percent of the frame.
pub type LatestColor = Arc<Mutex<Option<CenterColor>>>;

#[derive(Clone, Copy)]
pub struct CenterColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

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

/// Decode a small JPEG preview and average its centered five-percent region.
fn center_color(frame: &Frame) -> Result<CenterColor> {
    let camera_frame = unsafe { frame.0.as_ref() };
    let width = usize::try_from(camera_frame.width)?;
    let height = usize::try_from(camera_frame.height)?;
    let scaled_width = width.div_ceil(8);
    let scaled_height = height.div_ceil(8);
    let mut pixels = vec![0; scaled_width * scaled_height * 3];
    let mut config: camera::esp_jpeg_image_cfg_t = unsafe { std::mem::zeroed() };
    config.indata = frame.jpeg().as_ptr() as *mut u8;
    config.indata_size = u32::try_from(frame.jpeg().len())?;
    config.outbuf = pixels.as_mut_ptr();
    config.outbuf_size = u32::try_from(pixels.len())?;
    config.out_format = camera::esp_jpeg_image_format_t_JPEG_IMAGE_FORMAT_RGB888;
    config.out_scale = camera::esp_jpeg_image_scale_t_JPEG_IMAGE_SCALE_1_8;

    let mut decoded = camera::esp_jpeg_image_output_t::default();
    let result = unsafe { camera::esp_jpeg_decode(&mut config, &mut decoded) };
    if result != ESP_OK {
        bail!("JPEG color decode failed ({result:#x})");
    }

    let width = usize::from(decoded.width);
    let height = usize::from(decoded.height);
    let region_width = (width as f32 * 0.05_f32.sqrt()).round() as usize;
    let region_height = (height as f32 * 0.05_f32.sqrt()).round() as usize;
    let left = (width - region_width) / 2;
    let top = (height - region_height) / 2;
    let mut red = 0_u64;
    let mut green = 0_u64;
    let mut blue = 0_u64;

    for y in top..top + region_height {
        for x in left..left + region_width {
            let index = (y * width + x) * 3;
            red += u64::from(pixels[index]);
            green += u64::from(pixels[index + 1]);
            blue += u64::from(pixels[index + 2]);
        }
    }

    let count = (region_width * region_height) as u64;
    Ok(CenterColor {
        red: ((red + count / 2) / count) as u8,
        green: ((green + count / 2) / count) as u8,
        blue: ((blue + count / 2) / count) as u8,
    })
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
pub fn run(latest: LatestFrame, latest_color: LatestColor) -> Result<()> {
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
        *latest_color.lock().unwrap() = Some(center_color(&frame)?);
        *latest.lock().unwrap() = Some(frame.jpeg().to_vec());
        drop(frame);
        thread::sleep(Duration::from_secs(1).saturating_sub(started.elapsed()));
    }
}
