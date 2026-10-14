//! Start the camera, connect to Wi-Fi, and serve the live preview.
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
    // The camera task updates this JPEG while HTTP clients read it.
    let frame_slot = Mutex::new(None);
    let latest_frame = Arc::new(frame_slot);
    let camera_output = Arc::clone(&latest_frame);

    // Capture independently so each viewer can request the latest frame.
    thread::Builder::new()
        .name("camera".into())
        .stack_size(8192)
        .spawn(move || {
            if let Err(error) = camera::run(camera_output) {
                log::error!("Camera stopped: {error:#}");
            }
        })?;

    let wifi = network::connect()?;
    let _server = web::start(latest_frame)?;
    let address = wifi.wifi().sta_netif().get_ip_info()?.ip;
    log::info!("Open http://{address} to view the camera");

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
