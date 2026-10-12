//! Connect the ESP32 to the phone hotspot in wifi-config.rs.
use anyhow::Result;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::peripherals::Peripherals,
    nvs::EspDefaultNvsPartition,
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};

mod credentials {
    include!("../wifi-config.rs");
}

pub type Connection = BlockingWifi<EspWifi<'static>>;

pub fn connect() -> Result<Connection> {
    let peripherals = Peripherals::take()?;
    let events = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let wifi = EspWifi::new(peripherals.modem, events.clone(), Some(nvs))?;
    let mut wifi = BlockingWifi::wrap(wifi, events)?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: credentials::SSID
            .try_into()
            .expect("Hotspot name is too long"),
        password: credentials::PASSWORD
            .try_into()
            .expect("Hotspot password is too long"),
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    }))?;
    wifi.start()?;

    loop {
        log::info!("Connecting to hotspot {}", credentials::SSID);
        match wifi.connect().and_then(|_| wifi.wait_netif_up()) {
            Ok(()) => return Ok(wifi),
            Err(error) => {
                log::warn!("Wi-Fi connection failed: {error}; retrying in 5 seconds");
                let _ = wifi.disconnect();
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}
