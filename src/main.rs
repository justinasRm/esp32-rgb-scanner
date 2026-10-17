use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use anyhow::{bail, Result};
use esp_idf_svc::{
    bt::{
        ble::gap::{BleGapEvent, EspBleGap},
        Ble, BtDriver, BtStatus,
    },
    hal::{delay::FreeRtos, peripherals::Peripherals},
    log::EspLogger,
    nvs::EspDefaultNvsPartition,
    sys::{
        esp_ble_adv_params_t, esp_ble_gap_start_advertising, esp_ble_tx_power_set,
        esp_get_free_heap_size, esp_timer_get_time, ESP_OK,
    },
};
use log::{error, info};

// BLE intervals use 0.625 ms units: 400 means 250 ms.
const ADVERTISING_INTERVAL_MS: u16 = 150;
const ADVERTISING_INTERVAL_UNITS: u16 = (ADVERTISING_INTERVAL_MS * 16) / 10;
const STATS_PERIOD_MS: u32 = 5_000;

// Advertising fields: flags, manufacturer data (0xFFFF + hello), complete name.
const ADVERTISEMENT: [u8; 24] = [
    2, 0x01, 0x06, // General discoverable; BR/EDR not supported
    9, 0xFF, 6, 7, 1, 2, 3, 4, b'w', b'W', // me
    10, 0x09, // Complete local name
    b'C', b'O', b'o', b'L', b' ', b'S', b'h', b'i', b'T',
];

fn start_nonconnectable_advertising() -> Result<()> {
    println!("NC start");
    // Set a 250 ms interval, non-connectable mode, and all three primary channels.
    let mut params = esp_ble_adv_params_t {
        adv_int_min: ADVERTISING_INTERVAL_UNITS,
        adv_int_max: ADVERTISING_INTERVAL_UNITS,
        adv_type: 3,      // ADV_TYPE_NONCONN_IND
        own_addr_type: 0, // BLE_ADDR_TYPE_PUBLIC
        peer_addr: [0; 6],
        peer_addr_type: 0,    // BLE_ADDR_TYPE_PUBLIC
        channel_map: 0x07,    // ADV_CHNL_ALL: channels 37, 38, 39
        adv_filter_policy: 0, // Allow scan and connection requests
    };

    // ESP-IDF's C API takes a mutable pointer to the parameter structure.
    let result = unsafe { esp_ble_gap_start_advertising(&mut params) };
    if result != ESP_OK {
        bail!("ESP-IDF could not start BLE advertising (error {result})");
    }
    println!("NC end");
    Ok(())
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let bluetooth: BtDriver<'_, Ble> = BtDriver::new(peripherals.modem, Some(nvs))?;
    let gap = EspBleGap::new(bluetooth)?;

    // The GAP callback runs separately from main, so share status with atomics.
    let advertising = Arc::new(AtomicBool::new(false));
    let advertising_in_callback = Arc::clone(&advertising);
    let advertising_started_at_ms = Arc::new(AtomicU32::new(0));
    let start_time_in_callback = Arc::clone(&advertising_started_at_ms);

    gap.subscribe(move |event| match event {
        BleGapEvent::RawAdvertisingConfigured(status) => {
            println!("RAC start");
            if status != BtStatus::Success {
                error!("Advertising data setup failed: {status:?}");
            } else if let Err(err) = start_nonconnectable_advertising() {
                error!("{err:#}");
            }
            println!("RAC end");
        }
        BleGapEvent::AdvertisingStarted(status) => {
            println!("AS start");
            if status == BtStatus::Success {
                let now_ms = (unsafe { esp_timer_get_time() } / 1_000) as u32;
                start_time_in_callback.store(now_ms, Ordering::Relaxed);
                advertising_in_callback.store(true, Ordering::Relaxed);
            } else {
                error!("Advertising failed to start: {status:?}");
            }
            println!("AS end");
        }
        _ => {}
    })?;

    // Advertising power type 9 and level 4 (ESP_PWR_LVL_N0) select nominal 0 dBm.
    let power_result = unsafe { esp_ble_tx_power_set(9, 4) };
    if power_result != ESP_OK {
        bail!("ESP-IDF could not set advertising TX power (error {power_result})");
    }
    println!("conf set start");
    gap.set_raw_adv_conf(&ADVERTISEMENT)?;
    println!("conf set end");

    loop {
        FreeRtos::delay_ms(STATS_PERIOD_MS);
        if !advertising.load(Ordering::Relaxed) {
            continue;
        }

        let now_ms = (unsafe { esp_timer_get_time() } / 1_000) as u32;
        let started_at_ms = advertising_started_at_ms.load(Ordering::Relaxed);
        let uptime_seconds = now_ms.wrapping_sub(started_at_ms) / 1_000;
        info!(
            "stats: advertising=active, uptime={} s, free_heap={} bytes",
            uptime_seconds,
            unsafe { esp_get_free_heap_size() }
        );
    }
}
