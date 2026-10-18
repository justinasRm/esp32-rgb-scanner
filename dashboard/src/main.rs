use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[derive(Default)]
struct Stats {
    sender_advertising: bool,
    sender_connected: bool,
    receiver_connected: bool,
    receiver_status: String,
    pong_received: u32,
    last_round_trip_ms: u128,
    scan_reports: u32,
}

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>BLE packet dashboard</title>
<style>
body{font:16px system-ui,sans-serif;max-width:700px;margin:3rem auto;padding:0 1rem;color:#222}
nav{display:flex;gap:.5rem;border-bottom:1px solid #ccc}
button{font:inherit;padding:.7rem 1rem;border:0;background:#eee;cursor:pointer}
button.selected{background:#1769aa;color:white}.panel{padding:1.5rem 0}
.number{font-size:3rem;font-weight:650;margin:.4rem 0}.muted{color:#666}
</style></head><body>
<h1>BLE packet dashboard</h1>
<nav><button id="sender-tab" class="selected" onclick="show('sender')">Sender</button>
<button id="receiver-tab" onclick="show('receiver')">Receiver</button></nav>
<section id="sender" class="panel"><h2>Sender</h2><div id="sender-status" class="number">Starting</div>
<div>GATT server: ping / pong</div></section>
<section id="receiver" class="panel" hidden><h2>Receiver</h2><div id="receiver-status" class="number">Starting</div>
<p>BLE scan reports: <span id="scan-reports">0</span></p>
<p>Pong replies: <span id="pong-count">0</span></p>
<p>Latest round trip: <span id="round-trip">—</span></p></section>
<script>
function show(tab){for(const name of ['sender','receiver']){
 document.getElementById(name).hidden=name!==tab;
 document.getElementById(name+'-tab').classList.toggle('selected',name===tab)}}
async function update(){try{const s=await(await fetch('/api')).json();
 document.getElementById('sender-status').textContent=s.sender_connected?'Connected':(s.sender_advertising?'Advertising':'Starting');
 document.getElementById('receiver-status').textContent=s.receiver_status||'Starting';
 document.getElementById('scan-reports').textContent=s.scan_reports;
 document.getElementById('pong-count').textContent=s.pong_received;
 document.getElementById('round-trip').textContent=s.pong_received?s.last_round_trip_ms+' ms':'—';
 }catch(e){}} setInterval(update,500); update();
</script></body></html>"#;

fn update_from_line(line: &str, stats: &mut Stats) {
    let Some(start) = line.find("DASH,") else {
        return;
    };
    let fields: Vec<&str> = line[start..].trim().split(',').collect();

    if fields.len() == 3 && fields[1] == "SENDER" && fields[2] == "ADVERTISING" {
        stats.sender_advertising = true;
    }

    if fields.len() == 3 && fields[1] == "SENDER" && fields[2] == "CONNECTED" {
        stats.sender_connected = true;
    }

    if fields.len() == 3 && fields[1] == "SENDER" && fields[2] == "DISCONNECTED" {
        stats.sender_connected = false;
    }

    if fields.len() == 3 && fields[1] == "RECEIVER" && fields[2] == "CONNECTED" {
        stats.receiver_connected = true;
        stats.receiver_status = "Connected".to_owned();
    }

    if fields.len() == 3 && fields[1] == "RECEIVER" && fields[2] == "DISCONNECTED" {
        stats.receiver_connected = false;
        stats.receiver_status = "Disconnected".to_owned();
    }

    if fields.len() == 3 && fields[1] == "RECEIVER" {
        let status = match fields[2] {
            "READY" => Some("Starting"),
            "SCANNING" => Some("Scanning"),
            "FOUND_SERVICE" => Some("Found sender service"),
            "CONNECTING" => Some("Connecting"),
            "CONNECTED" => Some("Connected"),
            "DISCONNECTED" => Some("Disconnected"),
            "DISCOVERING" => Some("Finding hello service"),
            "READING" => Some("Reading hello"),
            "SERVICE_SEARCH_FAILED" => Some("Service discovery failed"),
            "SERVICE_NOT_FOUND" => Some("Service not found"),
            "CHARACTERISTIC_NOT_FOUND" => Some("Hello characteristic not found"),
            "READ_FAILED" => Some("Hello read failed"),
            "SCAN_FAILED" => Some("Scan failed"),
            _ => None,
        };
        if let Some(status) = status {
            stats.receiver_status = status.to_owned();
        }
    }

    if fields.len() == 5 && fields[1] == "RECEIVER" && fields[2] == "PONG" {
        if let (Ok(count), Ok(round_trip_ms)) = (fields[3].parse(), fields[4].parse()) {
            stats.pong_received = count;
            stats.last_round_trip_ms = round_trip_ms;
        }
    }

    if fields.len() == 4 && fields[1] == "RECEIVER" && fields[2] == "REPORTS" {
        if let Ok(count) = fields[3].parse::<u32>() {
            stats.scan_reports = count;
        }
    }
}

fn read_board(role: &'static str, port_name: String, stats: Arc<Mutex<Stats>>) {
    loop {
        let port = serialport::new(&port_name, 115_200)
            .timeout(Duration::from_millis(250))
            .open();

        match port {
            Ok(mut port) => {
                println!("Reading {role} from {port_name}");
                let mut line = Vec::new();
                let mut byte = [0_u8; 1];

                loop {
                    match port.read(&mut byte) {
                        Ok(1) if byte[0] == b'\n' => {
                            let text = String::from_utf8_lossy(&line);
                            let text = text.trim();
                            if text.starts_with("DASH,")
                                || text.starts_with("E (")
                                || text.starts_with("W (")
                            {
                                println!("[{role}] {text}");
                            }
                            update_from_line(&text, &mut stats.lock().unwrap());
                            line.clear();
                        }
                        Ok(1) if line.len() < 256 => line.push(byte[0]),
                        Ok(_) => line.clear(),
                        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(error) => {
                            eprintln!("{role} serial disconnected: {error}");
                            break;
                        }
                    }
                }
            }
            Err(error) => eprintln!("Cannot open {role} port {port_name}: {error}"),
        }

        thread::sleep(Duration::from_secs(2));
    }
}

fn json(stats: &Stats) -> String {
    format!(
        "{{\"sender_advertising\":{},\"sender_connected\":{},\"receiver_connected\":{},\"receiver_status\":{:?},\"pong_received\":{},\"last_round_trip_ms\":{},\"scan_reports\":{}}}",
        stats.sender_advertising,
        stats.sender_connected,
        stats.receiver_connected,
        stats.receiver_status,
        stats.pong_received,
        stats.last_round_trip_ms,
        stats.scan_reports,
    )
}

fn reply(mut stream: TcpStream, status: &str, content_type: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn handle_request(stream: TcpStream, stats: &Arc<Mutex<Stats>>) {
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }

    match request.split_whitespace().nth(1) {
        Some("/") => reply(stream, "200 OK", "text/html; charset=utf-8", PAGE),
        Some("/api") => {
            let body = json(&stats.lock().unwrap());
            reply(stream, "200 OK", "application/json", &body);
        }
        _ => reply(stream, "404 Not Found", "text/plain", "Not found"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: ble-dashboard <sender-port> <receiver-port>");
        eprintln!("Example: ble-dashboard /dev/cu.usbmodem1101 /dev/cu.usbmodem2101");
        std::process::exit(2);
    }

    let stats = Arc::new(Mutex::new(Stats::default()));
    thread::spawn({
        let stats = Arc::clone(&stats);
        let port = args[1].clone();
        move || read_board("sender", port, stats)
    });
    thread::spawn({
        let stats = Arc::clone(&stats);
        let port = args[2].clone();
        move || read_board("receiver", port, stats)
    });

    let listener = TcpListener::bind("127.0.0.1:8000")?;
    println!("Dashboard: http://localhost:8000 (Ctrl+C to stop)");
    for stream in listener.incoming().flatten() {
        handle_request(stream, &stats);
    }

    Ok(())
}
