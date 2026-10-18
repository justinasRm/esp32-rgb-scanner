use std::sync::{
    atomic::{AtomicU32, Ordering},
    mpsc, Arc,
};

use anyhow::Result;
use esp_idf_svc::{
    bt::{
        ble::{
            gap::{BleGapEvent, EspBleGap, GapSearchEvent, ScanDuplicate, ScanParams, ScanType},
            gatt::{
                client::{
                    CharacteristicElement, EspGattc, GattAuthReq, GattWriteType, GattcEvent,
                    ServiceElement,
                },
                GattStatus,
            },
        },
        Ble, BtDriver, BtStatus, BtUuid,
    },
    hal::peripherals::Peripherals,
    log::EspLogger,
    nvs::EspDefaultNvsPartition,
};
use log::error;
use std::time::{Duration, Instant};

const SERVICE_UUID: BtUuid = BtUuid::uuid16(0x00A0);
const PING_PONG_UUID: BtUuid = BtUuid::uuid16(0x00A1);

enum Event {
    // Startup: GAP accepted the scan settings. This can arrive before or after ClientRegistered.
    ScanParamsReady,
    ScanStarted(BtStatus),
    ScanReports(u32),
    // 1. Scanning found the sender's advertised service UUID.
    Found(
        esp_idf_svc::bt::BdAddr,
        esp_idf_svc::bt::ble::gap::BleAddrType,
    ),
    // Startup: GATT client is ready. Scanning starts after this and ScanParamsReady have both arrived.
    ClientRegistered(u8, GattStatus),
    // 2. The receiver's GATT connection attempt completed.
    Opened(u8, GattStatus, u16),
    // 3. Service discovery completed; the receiver then looks up the hello characteristic.
    SearchComplete(u8, GattStatus, u16),
    // The receiver read the characteristic after writing a ping.
    Read(GattStatus, Vec<u8>),
    WriteComplete(GattStatus, u8, u16, u16),
    // Later: the link dropped; the receiver starts scanning again.
    Disconnected,
}

fn advertises_our_service(data: &[u8]) -> bool {
    let mut offset = 0;

    while offset < data.len() {
        let Some(&length) = data.get(offset) else {
            return false;
        };
        let length = length as usize;
        if length == 0 || offset + length + 1 > data.len() {
            return false;
        }

        let field_type = data[offset + 1];
        if field_type == 0x02 || field_type == 0x03 {
            let service_uuids = &data[offset + 2..offset + length + 1];
            if service_uuids
                .chunks_exact(2)
                .any(|uuid| uuid == [0xA0, 0x00])
            {
                return true;
            }
        }
        offset += length + 1;
    }

    false
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let bluetooth: BtDriver<'_, Ble> = BtDriver::new(peripherals.modem, Some(nvs))?;
    let gap = EspBleGap::new(&bluetooth)?;
    let gattc = EspGattc::new(&bluetooth)?;
    let (sender, events) = mpsc::channel();
    let reports_seen = Arc::new(AtomicU32::new(0));

    let event_sender = sender.clone();
    let report_counter = Arc::clone(&reports_seen);
    gap.subscribe(move |event| {
        let event = match event {
            BleGapEvent::ScanParameterConfigured(BtStatus::Success) => Some(Event::ScanParamsReady),
            BleGapEvent::ScanStarted(status) => Some(Event::ScanStarted(status)),
            BleGapEvent::ScanResult(result) => {
                let report = match result {
                    GapSearchEvent::InquiryResult(report)
                    | GapSearchEvent::DiscoveryResult(report)
                    | GapSearchEvent::DiscoveryBleResult(report) => report,
                    _ => return,
                };
                let report_count = report_counter.fetch_add(1, Ordering::Relaxed) + 1;
                if report_count % 100 == 0 {
                    let _ = event_sender.send(Event::ScanReports(report_count));
                }
                if report.ble_adv.is_some_and(advertises_our_service) {
                    Some(Event::Found(report.bda, report.ble_addr_type))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(event) = event {
            let _ = event_sender.send(event);
        }
    })?;

    let event_sender = sender.clone();
    gattc.subscribe(move |(interface, event)| {
        let event = match event {
            GattcEvent::ClientRegistered { status, .. } => {
                Some(Event::ClientRegistered(interface, status))
            }
            GattcEvent::Open {
                status, conn_id, ..
            } => Some(Event::Opened(interface, status, conn_id)),
            GattcEvent::SearchComplete {
                status, conn_id, ..
            } => Some(Event::SearchComplete(interface, status, conn_id)),
            GattcEvent::ReadCharacteristic { status, value, .. } => {
                Some(Event::Read(status, value.unwrap_or_default().to_vec()))
            }
            GattcEvent::WriteCharacteristic { status, conn_id, handle } => {
                Some(Event::WriteComplete(status, interface, conn_id, handle))
            }
            GattcEvent::Disconnected { .. } => Some(Event::Disconnected),
            _ => None,
        };
        if let Some(event) = event {
            let _ = event_sender.send(event);
        }
    })?;

    let params = ScanParams {
        scan_type: ScanType::Passive,
        scan_interval: 0x50,
        scan_window: 0x50,
        scan_duplicate: ScanDuplicate::Disable,
        ..Default::default()
    };
    gap.set_scan_params(&params)?;
    gattc.register_app(0)?;
    println!("DASH,RECEIVER,READY");

    let mut scan_params_ready = false;
    let mut client_interface = None;
    let mut scanning = false;
    let mut connecting_or_connected = false;
    let mut connection = None;
    let mut ping_characteristic = None;
    let mut next_ping = None;
    let mut ping_started = None;
    let mut pong_count = 0_u32;

    loop {
        let timeout = next_ping
            .map(|deadline: Instant| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(1));
        let event = match events.recv_timeout(timeout) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let (Some((interface, conn_id)), Some(handle)) = (connection, ping_characteristic) {
                    if let Err(err) = gattc.write_characteristic(
                        interface,
                        conn_id,
                        handle,
                        b"ping",
                        GattWriteType::RequireResponse,
                        GattAuthReq::None,
                    ) {
                        error!("Could not write ping: {err}");
                        next_ping = Some(Instant::now() + Duration::from_secs(1));
                    } else {
                        ping_started = Some(Instant::now());
                        next_ping = None;
                        println!("DASH,RECEIVER,PING");
                    }
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match event {
            Event::ScanParamsReady => {
                scan_params_ready = true;
                println!("DASH,RECEIVER,SCAN_PARAMS_READY");
            }
            Event::ScanStarted(BtStatus::Success) => println!("DASH,RECEIVER,SCANNING"),
            Event::ScanStarted(status) => {
                error!("Scanning failed to start: {status:?}");
                println!("DASH,RECEIVER,SCAN_FAILED");
            }
            Event::ScanReports(count) => println!("DASH,RECEIVER,REPORTS,{count}"),
            Event::ClientRegistered(interface, GattStatus::Ok) => {
                client_interface = Some(interface);
                println!("DASH,RECEIVER,GATT_CLIENT_READY");
            }
            Event::ClientRegistered(_, status) => error!("GATT client setup failed: {status:?}"),
            Event::Found(address, address_type) if !connecting_or_connected => {
                connecting_or_connected = true;
                scanning = false;
                println!("DASH,RECEIVER,FOUND_SERVICE");
                gap.stop_scanning()?;
                let Some(interface) = client_interface else {
                    error!("GATT client is not ready yet");
                    connecting_or_connected = false;
                    continue;
                };
                println!("DASH,RECEIVER,CONNECTING");
                if let Err(err) = gattc.open(interface, &address, address_type, true) {
                    error!("Could not connect to sender: {err}");
                    connecting_or_connected = false;
                    gap.start_scanning(0)?;
                    scanning = true;
                }
            }
            Event::Opened(interface, GattStatus::Ok, conn_id) => {
                connection = Some((interface, conn_id));
                println!("DASH,RECEIVER,CONNECTED");
                println!("DASH,RECEIVER,DISCOVERING");
                gattc.search_service(interface, conn_id, Some(&SERVICE_UUID))?;
            }
            Event::Opened(_, status, _) => {
                error!("Connection failed: {status:?}");
                connecting_or_connected = false;
                gap.start_scanning(0)?;
                scanning = true;
            }
            Event::SearchComplete(interface, GattStatus::Ok, conn_id) => {
                let mut services = [ServiceElement::new()];
                let service_count =
                    gattc.get_service(interface, conn_id, Some(&SERVICE_UUID), 0, &mut services)?;
                if service_count == 0 {
                    error!("Sender does not have the hello service");
                    println!("DASH,RECEIVER,SERVICE_NOT_FOUND");
                    continue;
                }

                let mut characteristics = [CharacteristicElement::new()];
                let characteristic_count = gattc
                    .get_characteristic_by_uuid(
                        interface,
                        conn_id,
                        services[0].start_handle(),
                        services[0].end_handle(),
                        &PING_PONG_UUID,
                        &mut characteristics,
                    )
                    .map_err(|status| {
                        anyhow::anyhow!("Could not find GATT characteristic: {status:?}")
                    })?;
                if characteristic_count == 0 {
                    error!("Hello characteristic not found");
                    println!("DASH,RECEIVER,CHARACTERISTIC_NOT_FOUND");
                    continue;
                }

                ping_characteristic = Some(characteristics[0].handle());
                next_ping = Some(Instant::now());
            }
            Event::SearchComplete(_, status, _) => {
                error!("Service search failed: {status:?}");
                println!("DASH,RECEIVER,SERVICE_SEARCH_FAILED");
            }
            Event::WriteComplete(GattStatus::Ok, interface, conn_id, handle)
                if Some(handle) == ping_characteristic =>
            {
                gattc.read_characteristic(interface, conn_id, handle, GattAuthReq::None)?;
            }
            Event::WriteComplete(status, _, _, _) => {
                error!("Ping write failed: {status:?}");
                ping_started = None;
                next_ping = Some(Instant::now() + Duration::from_secs(1));
            }
            Event::Read(GattStatus::Ok, value) if value == b"pong" => {
                pong_count += 1;
                let round_trip_ms = ping_started
                    .take()
                    .map(|started| started.elapsed().as_millis())
                    .unwrap_or_default();
                println!("DASH,RECEIVER,PONG,{pong_count},{round_trip_ms}");
                next_ping = Some(Instant::now() + Duration::from_secs(1));
            }
            Event::Read(status, _) => {
                error!("Could not read pong: {status:?}");
                next_ping = Some(Instant::now() + Duration::from_secs(1));
            }
            Event::Disconnected => {
                println!("DASH,RECEIVER,DISCONNECTED");
                connecting_or_connected = false;
                connection = None;
                ping_characteristic = None;
                next_ping = None;
                gap.start_scanning(0)?;
                scanning = true;
            }
            Event::Found(_, _) => {}
        }

        if scan_params_ready && client_interface.is_some() && !scanning && !connecting_or_connected
        {
            gap.start_scanning(0)?;
            scanning = true;
        }
    }

    Ok(())
}
