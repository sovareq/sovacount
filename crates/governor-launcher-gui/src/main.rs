// Clippy: collapsible_if is bewust uit — geneste `if let`s zijn leesbaarder
// dan let-chains in dit launcher-bestand met I/O + filesystem-probes.
#![allow(clippy::collapsible_if)]

//! SovaCount native launcher — wry + tao.
//!
//! Toont een venster met:
//! - status-indicator (groen=draait, rood=uit)
//! - één AAN/UIT-knop die `governor-http` als child process spawnt of kill't
//! - "Dashboard"-knop die de externe browser opent op http://127.0.0.1:8989/
//!
//! Sovareq design-tokens hard ingebed in HTML (identiek aan dashboard).

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{WindowBuilder, Icon},
    dpi::LogicalSize,
};
use wry::WebViewBuilder;

const INDEX_HTML: &str = include_str!("index.html");
const GOVERNOR_PORT: u16 = 8989;

#[derive(Debug)]
enum UserEvent {
    StatusChanged(bool),
    ResetResult { ok: bool, count: usize, error: Option<String> },
}

fn main() -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("SovaCount")
        .with_inner_size(LogicalSize::new(440.0, 360.0))
        .with_resizable(false)
        .with_min_inner_size(LogicalSize::new(440.0, 360.0))
        .with_max_inner_size(LogicalSize::new(440.0, 360.0))
        .build(&event_loop)
        .expect("window build");

    // Probeer icoon te laden uit Resources (.app/Contents/Resources/icon.png).
    // .icns wordt door tao niet rechtstreeks ondersteund — Info.plist regelt
    // het dock-icoon. Hier laden we een optionele 256×256 PNG fallback.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent().and_then(|p| p.parent()) {
            let png_path = parent.join("Resources").join("icon.png");
            if png_path.exists() {
                if let Ok(bytes) = std::fs::read(&png_path) {
                    if let Some(icon) = parse_png_to_icon(&bytes) {
                        window.set_window_icon(Some(icon));
                    }
                }
            }
        }
    }

    let server_child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let server_child_for_ipc = Arc::clone(&server_child);
    let server_child_for_drop = Arc::clone(&server_child);

    let proxy_for_poll = proxy.clone();
    let server_child_for_poll = Arc::clone(&server_child);
    thread::spawn(move || {
        // Eerste poll na 600ms zodat de webview klaar is om window.sovaSetStatus
        // te ontvangen. Daarna stuur ALTIJD een StatusChanged event (niet alleen
        // bij wijziging) zodat de UI consistent reageert.
        thread::sleep(Duration::from_millis(600));
        let mut last_sent: Option<bool> = None;
        loop {
            let is_up = is_server_up();
            {
                let mut guard = server_child_for_poll.lock().unwrap();
                if let Some(child) = guard.as_mut() {
                    if let Ok(Some(_)) = child.try_wait() {
                        *guard = None;
                    }
                }
            }
            // Verstuur bij elke wijziging EN bij eerste tick (last_sent=None).
            if last_sent != Some(is_up) {
                let _ = proxy_for_poll.send_event(UserEvent::StatusChanged(is_up));
                last_sent = Some(is_up);
            }
            thread::sleep(Duration::from_millis(800));
        }
    });

    let proxy_for_ipc = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_html(INDEX_HTML)
        .with_ipc_handler(move |req| {
            let msg = req.body().as_str();
            handle_ipc(msg, Arc::clone(&server_child_for_ipc), proxy_for_ipc.clone());
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                // Schoon child proces op vóór quit.
                if let Ok(mut guard) = server_child_for_drop.lock() {
                    if let Some(mut child) = guard.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::StatusChanged(up)) => {
                let js = format!(
                    "window.sovaSetStatus({}); window.sovaSetProvider({});",
                    up,
                    serde_json::to_string(provider_label()).unwrap_or_else(|_| "\"\"".into())
                );
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::ResetResult { ok, count, error }) => {
                let payload = serde_json::json!({
                    "ok": ok,
                    "count": count,
                    "error": error,
                });
                let js = format!(
                    "window.sovaResetResult({});",
                    serde_json::to_string(&payload).unwrap_or_else(|_| "null".into())
                );
                let _ = webview.evaluate_script(&js);
            }
            _ => (),
        }
    });
}

fn handle_ipc(
    msg: &str,
    child_mutex: Arc<Mutex<Option<Child>>>,
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
) {
    let parsed: serde_json::Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => return,
    };
    let action = parsed.get("action").and_then(|v| v.as_str()).unwrap_or("");

    match action {
        "start" => {
            let mut guard = child_mutex.lock().unwrap();
            if guard.is_some() && is_server_up() {
                return; // al actief
            }
            let bin = locate_governor_http();
            if let Some(path) = bin {
                let (provider, api_key) = resolve_provider_and_key();
                let bind = std::env::var("GOVERNOR_HTTP_BIND").unwrap_or_else(|_| format!("127.0.0.1:{}", GOVERNOR_PORT));
                let mut cmd = Command::new(path);
                cmd.env("GOVERNOR_PROVIDER", &provider)
                    .env("GOVERNOR_HTTP_BIND", bind)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                if let Some(k) = api_key {
                    cmd.env("GOVERNOR_API_KEY", k);
                }
                if let Ok(c) = cmd.spawn() {
                    *guard = Some(c);
                }
            }
        }
        "stop" => {
            let mut guard = child_mutex.lock().unwrap();
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            } else {
                // Geen child onder onze controle — probeer een bestaande server te killen
                let _ = Command::new("pkill")
                    .args(["-f", "governor-http"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        "open_dashboard" => {
            let url = format!("http://127.0.0.1:{}/", GOVERNOR_PORT);
            let _ = Command::new("open").arg(url).status();
        }
        "reset" => {
            // POST /reset op de lokale server. Niet-blokkerend: in een thread.
            thread::spawn(move || {
                let result = post_reset();
                let _ = proxy.send_event(UserEvent::ResetResult {
                    ok: result.is_ok(),
                    count: result.as_ref().map(|n| *n).unwrap_or(0),
                    error: result.err(),
                });
            });
        }
        _ => {}
    }
}

/// POST http://127.0.0.1:8989/reset via stdlib TCP. Geen extra crate nodig.
/// Returnt het aantal verwijderde bestanden bij succes, of een korte foutboodschap.
fn post_reset() -> Result<usize, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let addr = format!("127.0.0.1:{}", GOVERNOR_PORT);
    let mut stream = TcpStream::connect_timeout(
        &addr.parse().map_err(|e: std::net::AddrParseError| e.to_string())?,
        Duration::from_millis(800),
    ).map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_millis(2000))).ok();
    let req = b"POST /reset HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    stream.write_all(req).map_err(|e| format!("write: {e}"))?;
    let mut buf = Vec::with_capacity(2048);
    stream.read_to_end(&mut buf).map_err(|e| format!("read: {e}"))?;
    let resp = String::from_utf8_lossy(&buf);
    if !resp.starts_with("HTTP/1.0 200") && !resp.starts_with("HTTP/1.1 200") {
        let status = resp.lines().next().unwrap_or("").to_string();
        return Err(format!("server: {status}"));
    }
    // Find body — eerste blank-line scheidt headers van body
    let body_idx = resp.find("\r\n\r\n").map(|i| i + 4).unwrap_or(resp.len());
    let body = &resp[body_idx..];
    let parsed: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("parse: {e}"))?;
    let count = parsed.get("deleted_files").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    Ok(count)
}

fn is_server_up() -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let mut stream = match TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", GOVERNOR_PORT).parse().unwrap(),
        Duration::from_millis(300),
    ) {
        Ok(s) => s,
        Err(_) => return false,
    };
    stream.set_read_timeout(Some(Duration::from_millis(400))).ok();
    let req = b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n";
    if stream.write_all(req).is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    match stream.read(&mut buf) {
        Ok(n) if n > 0 => {
            let s = String::from_utf8_lossy(&buf[..n]);
            s.starts_with("HTTP/1.0 200") || s.starts_with("HTTP/1.1 200")
        }
        _ => false,
    }
}

/// Lees de provider-config bij elke spawn opnieuw zodat een nieuw geplaatste
/// key onmiddellijk effect heeft zonder launcher-restart.
///
/// Resolutie-volgorde:
///   1. Env GOVERNOR_PROVIDER + GOVERNOR_API_KEY (handmatige override)
///   2. ~/.config/sovacount/anthropic-key (file) → provider=anthropic
///   3. Geen key → provider=mock, geen key
fn resolve_provider_and_key() -> (String, Option<String>) {
    if let Ok(p) = std::env::var("GOVERNOR_PROVIDER") {
        let key = std::env::var("GOVERNOR_API_KEY").ok();
        return (p, key);
    }
    if let Ok(home) = std::env::var("HOME") {
        let key_path = std::path::PathBuf::from(home).join(".config/sovacount/anthropic-key");
        if key_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&key_path) {
                let key = content.trim().to_string();
                if !key.is_empty() {
                    return ("anthropic".to_string(), Some(key));
                }
            }
        }
    }
    ("mock".to_string(), None)
}

/// Welk label tonen in de UI subtitle. Geen key gelezen ≠ geen werking.
fn provider_label() -> &'static str {
    if let Ok(home) = std::env::var("HOME") {
        let key_path = std::path::PathBuf::from(home).join(".config/sovacount/anthropic-key");
        if key_path.exists() {
            return "anthropic provider";
        }
    }
    "mock provider"
}

fn locate_governor_http() -> Option<std::path::PathBuf> {
    // 1. ~/.local/bin/governor-http
    if let Ok(home) = std::env::var("HOME") {
        let p = std::path::PathBuf::from(home).join(".local/bin/governor-http");
        if p.exists() {
            return Some(p);
        }
    }
    // 2. Naast onze launcher binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("governor-http");
            if p.exists() {
                return Some(p);
            }
        }
    }
    // 3. In de workspace cargo target/release als we vanuit de repo draaien
    if let Ok(exe) = std::env::current_exe() {
        let mut p = exe.clone();
        for _ in 0..5 {
            if let Some(parent) = p.parent() {
                let candidate = parent.join("target/release/governor-http");
                if candidate.exists() {
                    return Some(candidate);
                }
                p = parent.to_path_buf();
            } else {
                break;
            }
        }
    }
    None
}

/// Minimalistische PNG-decoder zonder externe crate: we lezen de width/height
/// uit de IHDR-chunk en gebruiken de raw bytes als-is. Voor het tao-icoon
/// hebben we echter RGBA-bytes nodig — daarom: alleen RGBA-PNGs werken hier.
/// Als parsing faalt geven we None terug en heeft de launcher gewoon geen
/// custom icoon (de .app krijgt het via Info.plist alsnog).
fn parse_png_to_icon(_bytes: &[u8]) -> Option<Icon> {
    // Voor de eerste versie laten we icoon-decoding over aan macOS via
    // Info.plist + icon.icns. Custom tao-Icon vereist een raw RGBA buffer;
    // dat heroptueren we later met een echte PNG-crate als nodig.
    None
}
