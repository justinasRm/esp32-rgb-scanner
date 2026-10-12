//! Serve the preview, RGB-only page, and camera data over Wi-Fi.
use crate::camera::{LatestColor, LatestFrame};
use anyhow::Result;
use esp_idf_svc::{
    http::{server::EspHttpServer, Method},
    io::Write,
};

pub fn start(latest: LatestFrame, latest_color: LatestColor) -> Result<EspHttpServer<'static>> {
    let mut server = EspHttpServer::new(&Default::default())?;

    server.fn_handler("/", Method::Get, |request| -> Result<()> {
        request
            .into_response(200, Some("OK"), &[("Content-Type", "text/html")])?
            .write_all(include_bytes!("preview.html"))?;
        Ok(())
    })?;

    server.fn_handler("/rgb-only", Method::Get, |request| -> Result<()> {
        request
            .into_response(200, Some("OK"), &[("Content-Type", "text/html")])?
            .write_all(include_bytes!("rgb_only.html"))?;
        Ok(())
    })?;

    server.fn_handler("/app.js", Method::Get, |request| -> Result<()> {
        request
            .into_response(200, Some("OK"), &[("Content-Type", "text/javascript")])?
            .write_all(include_bytes!("app.js"))?;
        Ok(())
    })?;

    server.fn_handler("/rgb_only.js", Method::Get, |request| -> Result<()> {
        request
            .into_response(200, Some("OK"), &[("Content-Type", "text/javascript")])?
            .write_all(include_bytes!("rgb_only.js"))?;
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

    server.fn_handler("/rgb", Method::Get, move |request| -> Result<()> {
        let response = match *latest_color.lock().unwrap() {
            Some(color) => format!(
                "{{\"red\":{},\"green\":{},\"blue\":{}}}",
                color.red, color.green, color.blue
            ),
            None => {
                request
                    .into_response(503, Some("Camera starting"), &[])?
                    .write_all(b"Camera is starting; try again shortly")?;
                return Ok(());
            }
        };
        request
            .into_response(
                200,
                Some("OK"),
                &[
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-store"),
                ],
            )?
            .write_all(response.as_bytes())?;
        Ok(())
    })?;

    Ok(server)
}
