//! Serve a simple camera preview and the latest JPEG over Wi-Fi.
use crate::camera::LatestFrame;
use anyhow::Result;
use esp_idf_svc::{
    http::{server::EspHttpServer, Method},
    io::Write,
};

pub fn start(latest: LatestFrame) -> Result<EspHttpServer<'static>> {
    let mut server = EspHttpServer::new(&Default::default())?;

    server.fn_handler("/", Method::Get, |request| -> Result<()> {
        request
            .into_response(200, Some("OK"), &[("Content-Type", "text/html")])?
            .write_all(include_bytes!("preview.html"))?;
        Ok(())
    })?;

    server.fn_handler("/capture", Method::Get, move |request| -> Result<()> {
        match latest.lock().unwrap().clone() {
            Some(jpeg) => {
                request
                    .into_response(
                        200,
                        Some("OK"),
                        &[
                            ("Content-Type", "image/jpeg"),
                            ("Cache-Control", "no-store"),
                        ],
                    )?
                    .write_all(&jpeg)?;
            }
            None => {
                request
                    .into_response(503, Some("Camera starting"), &[])?
                    .write_all(b"Camera is starting; try again shortly")?;
            }
        }
        Ok(())
    })?;

    Ok(server)
}
