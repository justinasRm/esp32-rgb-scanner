use std::sync::mpsc;

use anyhow::{bail, Result};
use esp_idf_svc::{
    bt::{
        ble::{
            gap::{BleGapEvent, EspBleGap},
            gatt::{
                server::{EspGatts, GattsEvent},
                AutoResponse, GattCharacteristic, GattId, GattServiceId, Permission, Property,
            },
        },
        Ble, BtDriver, BtStatus, BtUuid,
    },
    hal::peripherals::Peripherals,
    log::EspLogger,
    nvs::EspDefaultNvsPartition,
    sys::{esp_ble_adv_params_t, esp_ble_gap_start_advertising, esp_ble_tx_power_set, ESP_OK},
};
use log::error;

const SERVICE_UUID: BtUuid = BtUuid::uuid16(0x00A0);
const PING_PONG_UUID: BtUuid = BtUuid::uuid16(0x00A1);

// Flags, the 0x00A0 service UUID, and the name make the receiver able to find us.
const ADVERTISEMENT: [u8; 20] = [
    2, 0x01, 0x06, // Flags
    3, 0x03, 0xA0, 0x00, // Complete list of 16-bit service UUIDs
    12, 0x09, // Complete local name: type byte plus 11 name bytes
    b'E', b'S', b'P', b'3', b'2', b'-', b'H', b'E', b'L', b'L', b'O',
];

enum Event {
    /// 1. ESP-IDF registered this GATT server application.
    Registered(u8, esp_idf_svc::bt::ble::gatt::GattStatus),
    /// 2. The sender's GATT service was created.
    ServiceCreated(esp_idf_svc::bt::ble::gatt::GattStatus, u16),
    /// 3. The readable "hello" characteristic was added.
    CharacteristicAdded(esp_idf_svc::bt::ble::gatt::GattStatus, u16),
    /// 4. The service is ready; the sender configures its advertisement next.
    ServiceStarted(esp_idf_svc::bt::ble::gatt::GattStatus),
    /// 5. GAP finished configuring the advertisement.
    AdvertisingConfigured(BtStatus),
    /// 6. GAP started advertising; now the receiver can find and connect to the sender.
    AdvertisingStarted(BtStatus),
    /// 7. A receiver connected to the sender.
    Connected,
    /// The receiver wrote a value to the ping/pong characteristic.
    Write(u16, Vec<u8>),
    /// Later: the receiver disconnected.
    Disconnected,
}

fn start_advertising() -> Result<()> {
    let mut params = esp_ble_adv_params_t {
        adv_int_min: 240,
        adv_int_max: 240,
        adv_type: 0, // Connectable, undirected advertising
        own_addr_type: 0,
        peer_addr: [0; 6],
        peer_addr_type: 0,
        channel_map: 0x07,
        adv_filter_policy: 0,
    };

    let result = unsafe { esp_ble_gap_start_advertising(&mut params) };
    if result != ESP_OK {
        bail!("Could not start advertising (error {result})");
    }
    Ok(())
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let bluetooth: BtDriver<'_, Ble> = BtDriver::new(peripherals.modem, Some(nvs))?;
    let gap = EspBleGap::new(&bluetooth)?;
    let gatts = EspGatts::new(&bluetooth)?;
    let (sender, events) = mpsc::channel();

    let event_sender = sender.clone();
    gatts.subscribe(move |(interface, event)| {
        let event = match event {
            GattsEvent::ServiceRegistered { status, .. } => {
                Some(Event::Registered(interface, status))
            }
            GattsEvent::ServiceCreated {
                status,
                service_handle,
                ..
            } => Some(Event::ServiceCreated(status, service_handle)),
            GattsEvent::CharacteristicAdded {
                status,
                service_handle,
                ..
            } => Some(Event::CharacteristicAdded(status, service_handle)),
            GattsEvent::ServiceStarted { status, .. } => Some(Event::ServiceStarted(status)),
            GattsEvent::PeerConnected { .. } => Some(Event::Connected),
            GattsEvent::PeerDisconnected { .. } => Some(Event::Disconnected),
            GattsEvent::Write { handle, value, .. } => Some(Event::Write(handle, value.to_vec())),
            _ => None,
        };
        if let Some(event) = event {
            let _ = event_sender.send(event);
        }
    })?;

    let event_sender = sender.clone();
    gap.subscribe(move |event| {
        let event = match event {
            BleGapEvent::RawAdvertisingConfigured(status) => {
                Some(Event::AdvertisingConfigured(status))
            }
            BleGapEvent::AdvertisingStarted(status) => Some(Event::AdvertisingStarted(status)),
            _ => None,
        };
        if let Some(event) = event {
            let _ = event_sender.send(event);
        }
    })?;

    let power_result = unsafe { esp_ble_tx_power_set(9, 4) }; // Advertising, nominal 0 dBm
    if power_result != ESP_OK {
        bail!("Could not set advertising power (error {power_result})");
    }

    gatts.register_app(0)?;
    println!("DASH,SENDER,READY");

    while let Ok(event) = events.recv() {
        use esp_idf_svc::bt::ble::gatt::GattStatus;
        match event {
            Event::Registered(interface, GattStatus::Ok) => {
                println!("DASH,SENDER,GATT_REGISTERED");
                let service = GattServiceId {
                    id: GattId {
                        uuid: SERVICE_UUID,
                        inst_id: 0,
                    },
                    is_primary: true,
                };
                gatts.create_service(interface, &service, 4)?;
            }
            Event::Registered(_, status) => error!("GATT service registration failed: {status:?}"),
            Event::ServiceCreated(GattStatus::Ok, handle) => {
                println!("DASH,SENDER,SERVICE_CREATED");
                let ping_pong = GattCharacteristic::new(
                    PING_PONG_UUID,
                    (Permission::Read | Permission::Write).into(),
                    (Property::Read | Property::Write).into(),
                    5,
                    AutoResponse::ByGatt,
                );
                gatts.add_characteristic(handle, &ping_pong, b"pong")?;
            }
            Event::ServiceCreated(status, _) => error!("GATT service creation failed: {status:?}"),
            Event::CharacteristicAdded(GattStatus::Ok, service_handle) => {
                println!("DASH,SENDER,CHARACTERISTIC_ADDED");
                gatts.start_service(service_handle)?;
            }
            Event::CharacteristicAdded(status, _) => {
                error!("GATT characteristic setup failed: {status:?}")
            }
            Event::ServiceStarted(GattStatus::Ok) => {
                println!("DASH,SENDER,SERVICE_STARTED");
                gap.set_raw_adv_conf(&ADVERTISEMENT)?;
            }
            Event::ServiceStarted(status) => error!("GATT service start failed: {status:?}"),
            Event::AdvertisingConfigured(BtStatus::Success) => {
                if let Err(err) = start_advertising() {
                    error!("{err:#}");
                }
            }
            Event::AdvertisingConfigured(status) => error!("Advertising setup failed: {status:?}"),
            Event::AdvertisingStarted(BtStatus::Success) => println!("DASH,SENDER,ADVERTISING"),
            Event::AdvertisingStarted(status) => error!("Advertising failed to start: {status:?}"),
            Event::Connected => println!("DASH,SENDER,CONNECTED"),
            Event::Write(handle, value) if value == b"ping" => {
                gatts.set_attr(handle, b"pong")?;
                println!("DASH,SENDER,PING");
            }
            Event::Write(_, _) => {}
            Event::Disconnected => {
                println!("DASH,SENDER,DISCONNECTED");
                if let Err(err) = start_advertising() {
                    error!("Could not resume advertising: {err:#}");
                }
            }
        }
    }

    Ok(())
}
