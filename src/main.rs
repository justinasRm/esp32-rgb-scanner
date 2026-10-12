//! Start the camera, connect to the hotspot, and serve the color meter.
mod camera;
mod network;
mod web;

use anyhow::Result;
use esp_idf_svc::log::EspLogger;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

fn run() -> Result<()> {
    // Both handles point to the same mutex-protected latest JPEG.
    let frame_slot = Mutex::new(None);
    let latest_frame = Arc::new(frame_slot);
    let camera_output = Arc::clone(&latest_frame);
    let color_slot = Mutex::new(None);
    let latest_color = Arc::new(color_slot);
    let camera_color = Arc::clone(&latest_color);

    // Keep one-second camera capture independent from web page requests.
    thread::Builder::new()
        .name("camera".into())
        .stack_size(8192)
        .spawn(move || {
            if let Err(error) = camera::run(camera_output, camera_color) {
                log::error!("Camera stopped: {error:#}");
            }
        })?;

    let wifi = network::connect()?;
    let _server = web::start(latest_frame, latest_color)?;
    let address = wifi.wifi().sta_netif().get_ip_info()?.ip;
    log::info!("Open http://{address} to view the camera color meter");

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    if let Err(error) = run() {
        log::error!("Application stopped: {error:#}");
    }
}
