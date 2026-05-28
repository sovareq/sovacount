// Crate-discipline: `#![deny(unsafe_code)]` op crate-niveau, met twee
// expliciete `#[allow(unsafe_code)]` op de twee functies die unsafe blocks
// nodig hebben:
//   * `ChildGuard::graceful_stop` — `libc::kill(pid, SIGTERM)` syscall.
//   * `main` — `std::env::set_var` (in Rust 2024 unsafe wegens process-wide
//     race-condition met andere threads die `env::var` lezen).
// Alle andere code is safe Rust.
#![deny(unsafe_code)]
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
//!
//! ## Production-lifecycle (T-G-launcher-prod)
//!
//! Drie verdedigingslagen tegen orphan `governor-http` processen:
//!
//! 1. **`ChildGuard` Drop-impl** — wrapt het Child-handle. Bij Drop (normaal
//!    exit, panic, of stack-unwind) stuurt SIGTERM, wacht 500ms, dan
//!    `kill_tree::kill_tree(pid)` voor de hele subtree.
//! 2. **Signal-handler** — registreert SIGTERM + SIGINT via `signal-hook`.
//!    Wanneer de OS de launcher killt (`kill -TERM`, force-quit) wordt de
//!    AtomicBool gezet zodat het tao-event-loop normaal exit + Drop runt.
//! 3. **`tao::Event::LoopDestroyed`** — vangt force-close-paden die
//!    `CloseRequested` niet triggeren.
//!
//! Plus:
//! - `single-instance` flock voorkomt dubbele Finder-launch.
//! - Env-guard `SOVACOUNT_LAUNCHER_GUARD=1` voorkomt recursieve respawn.
//! - Geen `pkill -f governor-http` meer — dat killt processes van andere
//!   users op multi-user machines. PID-tracking only.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use single_instance::SingleInstance;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
use tracing::{error, info, warn};
use wry::WebViewBuilder;

const INDEX_HTML: &str = include_str!("index.html");
const GOVERNOR_PORT: u16 = 8989;
const LAUNCHER_LOCK_KEY: &str = "com.sovareq.sovacount.launcher";
const ENV_GUARD: &str = "SOVACOUNT_LAUNCHER_GUARD";

#[derive(Debug)]
enum UserEvent {
    StatusChanged(bool),
    ResetResult {
        ok: bool,
        count: usize,
        error: Option<String>,
    },
    /// Non-fatale launcher-fout die getoond moet worden in de WebView als
    /// rode toast. Bijvoorbeeld: `governor-http` binary niet gevonden.
    LauncherError(String),
}

/// Wrapper rond `std::process::Child` die garandeert dat het kindproces
/// (en zijn hele subtree) gekilled wordt bij Drop. Voorkomt orphans op
/// elk exit-pad: normaal CloseRequested, panic, of OS-signal.
struct ChildGuard {
    inner: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { inner: Some(child) }
    }

    fn id(&self) -> Option<u32> {
        self.inner.as_ref().map(|c| c.id())
    }

    /// Probeer graceful shutdown via SIGTERM + 500ms wait. Bij timeout valt
    /// Drop terug op SIGKILL via `kill_tree`.
    #[allow(unsafe_code)]
    fn graceful_stop(&mut self) {
        let Some(mut child) = self.inner.take() else {
            return;
        };
        let pid = child.id();
        // SIGTERM first — geeft governor-http kans om netjes te shutdownen.
        // SAFETY: libc::kill is een dunne wrapper rond de POSIX kill(2) syscall.
        // De PID komt van een succesvolle `Command::spawn()` → geldig op moment
        // van aanroep. SIGTERM op een dood PID is geen UB — kernel returnt
        // ESRCH. Geen pointer-arg, geen geheugen-aliasing.
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        // Wacht max 500ms op natural exit.
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    info!(pid, "child exited gracefully after SIGTERM");
                    return;
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    warn!(pid, error = %e, "try_wait failed");
                    break;
                }
            }
        }
        // Timeout — escaleer naar SIGKILL op de hele tree.
        warn!(pid, "graceful stop timed out, escalating to kill_tree");
        let _ = kill_tree::blocking::kill_tree(pid);
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.inner.as_ref().map(|c| c.id()) {
            warn!(pid, "ChildGuard drop — cleaning up subtree");
            // Geen graceful poging hier — Drop moet snel klaar zijn.
            let _ = kill_tree::blocking::kill_tree(pid);
            if let Some(mut child) = self.inner.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[allow(unsafe_code)]
fn main() -> wry::Result<()> {
    // 1. Env-guard: voorkom recursieve respawn (script die zichzelf aanroept,
    //    of een toekomstige helper-binary die de launcher per ongeluk re-exec't).
    if std::env::var(ENV_GUARD).is_ok() {
        eprintln!("[sovacount-launcher] recursive spawn blocked via {ENV_GUARD}");
        std::process::exit(0);
    }
    // SAFETY: `set_var` is in Rust 2024 unsafe wegens process-wide
    // race-condition met andere threads die `env::var` lezen. We zijn hier
    // single-threaded (main, geen threads gespawnd yet) — geen lezer kan
    // race'n.
    unsafe {
        std::env::set_var(ENV_GUARD, "1");
    }

    // 2. Logging — schrijft naar ~/Library/Logs/SovaCount.log zodat we crashes
    //    en spawn-failures kunnen post-mortemen zonder Stdio te lekken.
    init_logging();

    info!("SovaCount launcher starting (env_guard set, logging initialised)");

    // 3. Single-instance lock — voorkomt dat een tweede launcher de poort 8989
    //    binding probeert te stelen of orphan-children spawnt.
    let instance = SingleInstance::new(LAUNCHER_LOCK_KEY).map_err(|e| {
        error!(error = %e, "single-instance check failed");
        wry::Error::Io(std::io::Error::other(e.to_string()))
    })?;
    if !instance.is_single() {
        eprintln!(
            "[sovacount-launcher] another instance is already running — focusing it would be nice, exiting for now"
        );
        info!("rejecting second instance, exiting");
        // Open dashboard in browser zodat de gebruiker iets nuttigs ziet
        // ipv silent exit.
        let _ = Command::new("open")
            .arg(format!("http://127.0.0.1:{GOVERNOR_PORT}/"))
            .status();
        std::process::exit(0);
    }

    // 4. Event-loop bouw.
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

    // Icoon-handling: NSApplication leest Resources/icon.icns automatisch.

    let server_child: Arc<Mutex<Option<ChildGuard>>> = Arc::new(Mutex::new(None));

    // 5. Signal-handlers — SIGTERM + SIGINT zetten een AtomicBool zodat
    //    de poll-thread de tao-loop kan beëindigen via UserEvent. Echte
    //    cleanup gebeurt via ChildGuard's Drop op `server_child`.
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    register_signal_handlers(Arc::clone(&shutdown_flag));

    // 6. Poll-thread — checkt elke 800ms of de server live is + ruimt zombies
    //    op. Stopt zichzelf zodra shutdown_flag = true OF de stop-channel sluit.
    let proxy_for_poll = proxy.clone();
    let server_child_for_poll = Arc::clone(&server_child);
    let shutdown_flag_for_poll = Arc::clone(&shutdown_flag);
    let _poll_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(600));
        let mut last_sent: Option<bool> = None;
        while !shutdown_flag_for_poll.load(Ordering::Relaxed) {
            let is_up = is_server_up();
            {
                let mut guard = server_child_for_poll.lock().unwrap();
                if let Some(child_guard) = guard.as_mut() {
                    if let Some(inner) = child_guard.inner.as_mut() {
                        if let Ok(Some(_)) = inner.try_wait() {
                            info!("child process exited spontaneously, clearing guard");
                            *guard = None;
                        }
                    }
                }
            }
            if last_sent != Some(is_up) {
                if proxy_for_poll
                    .send_event(UserEvent::StatusChanged(is_up))
                    .is_err()
                {
                    // Event-loop is dood — stop de thread.
                    info!("poll-thread: event-loop closed, exiting");
                    return;
                }
                last_sent = Some(is_up);
            }
            // Sleep in kleine increments zodat we shutdown_flag snel zien.
            for _ in 0..8 {
                if shutdown_flag_for_poll.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        info!("poll-thread: shutdown_flag set, exiting cleanly");
    });

    // 7. Signal-monitor-thread — zet shutdown_flag zodra een signal binnenkomt.
    //    Stuurt ook UserEvent zodat de event-loop exit triggert.
    let proxy_for_signal = proxy.clone();
    let shutdown_flag_for_signal = Arc::clone(&shutdown_flag);
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(200));
            if shutdown_flag_for_signal.load(Ordering::Relaxed) {
                warn!("signal received, signalling event-loop to exit");
                // Stuur dummy StatusChanged om de loop wakker te schudden — bij
                // de eerstvolgende iteration zien we shutdown_flag en exit'en.
                let _ = proxy_for_signal.send_event(UserEvent::StatusChanged(false));
                return;
            }
        }
    });

    let proxy_for_ipc = proxy.clone();
    let server_child_for_ipc = Arc::clone(&server_child);
    let webview = WebViewBuilder::new()
        .with_html(INDEX_HTML)
        .with_ipc_handler(move |req| {
            let msg = req.body().as_str();
            handle_ipc(
                msg,
                Arc::clone(&server_child_for_ipc),
                proxy_for_ipc.clone(),
            );
        })
        .build(&window)?;

    let server_child_for_cleanup = Arc::clone(&server_child);
    let shutdown_flag_for_loop = Arc::clone(&shutdown_flag);

    info!("entering event-loop");

    event_loop.run(move |event, _, control_flow| {
        // Default: blokken op events. Polling van het signal-flag gebeurt via
        // de monitor-thread die UserEvents stuurt.
        *control_flow = ControlFlow::Wait;

        // Universele shutdown-check: signal binnenkomst → exit het loop.
        if shutdown_flag_for_loop.load(Ordering::Relaxed) {
            cleanup_child(&server_child_for_cleanup);
            *control_flow = ControlFlow::Exit;
            return;
        }

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                info!("WindowEvent::CloseRequested — cleaning up");
                shutdown_flag_for_loop.store(true, Ordering::Relaxed);
                cleanup_child(&server_child_for_cleanup);
                *control_flow = ControlFlow::Exit;
            }
            Event::LoopDestroyed => {
                // Force-close pad (Cmd+Q via menu zonder window-event, OS-kill,
                // event-loop crash). ChildGuard's Drop runt straks ook nog,
                // maar we doen het hier expliciet voor logging.
                info!("Event::LoopDestroyed — final cleanup");
                shutdown_flag_for_loop.store(true, Ordering::Relaxed);
                cleanup_child(&server_child_for_cleanup);
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
            Event::UserEvent(UserEvent::LauncherError(msg)) => {
                error!(message = %msg, "launcher error surfaced to UI");
                let payload = serde_json::to_string(&msg).unwrap_or_else(|_| "\"\"".into());
                let js = format!("window.sovaError({payload});");
                let _ = webview.evaluate_script(&js);
            }
            _ => (),
        }
    });

    // Onbereikbaar — event_loop.run roept intern std::process::exit() en geeft
    // never (`!`) terug. De `_ = poll_thread` voorkomt een "unused" warning;
    // het type van event_loop.run is `!` dat coerce't naar `wry::Result<()>`.
    #[allow(unreachable_code)]
    {
        Ok(())
    }
}

/// Eindelijk cleanup van het server-child: graceful SIGTERM + 500ms wait,
/// dan SIGKILL via kill_tree. Idempotent — veilig om meerdere keren te roepen.
fn cleanup_child(child_mutex: &Arc<Mutex<Option<ChildGuard>>>) {
    let Ok(mut guard) = child_mutex.lock() else {
        warn!("cleanup_child: mutex poisoned");
        return;
    };
    if let Some(mut child_guard) = guard.take() {
        info!(pid = ?child_guard.id(), "cleanup_child: stopping");
        child_guard.graceful_stop();
    }
}

fn handle_ipc(
    msg: &str,
    child_mutex: Arc<Mutex<Option<ChildGuard>>>,
    proxy: EventLoopProxy<UserEvent>,
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
                info!("start: server already running, no-op");
                return;
            }
            let Some(path) = locate_governor_http() else {
                error!("start: governor-http binary not found");
                let _ = proxy.send_event(UserEvent::LauncherError(
                    "governor-http binary niet gevonden. Verwacht in ~/.local/bin/ of naast de launcher in de .app-bundle.".to_string(),
                ));
                return;
            };
            info!(binary = %path.display(), "start: spawning governor-http");
            let (provider, api_key) = resolve_provider_and_key();
            let bind = std::env::var("GOVERNOR_HTTP_BIND")
                .unwrap_or_else(|_| format!("127.0.0.1:{GOVERNOR_PORT}"));
            let mut cmd = Command::new(&path);
            cmd.env("GOVERNOR_PROVIDER", &provider)
                .env("GOVERNOR_HTTP_BIND", bind)
                // BELANGRIJK: clear de fork-bomb env-guard zodat governor-http
                // niet denkt dat het zelf een recursieve launcher-respawn is.
                .env_remove(ENV_GUARD)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(k) = api_key {
                cmd.env("GOVERNOR_API_KEY", k);
            }
            match cmd.spawn() {
                Ok(c) => {
                    info!(pid = c.id(), "spawned governor-http");
                    *guard = Some(ChildGuard::new(c));
                }
                Err(e) => {
                    error!(error = %e, "spawn failed");
                    let _ = proxy.send_event(UserEvent::LauncherError(format!(
                        "Kon governor-http niet starten: {e}"
                    )));
                }
            }
        }
        "stop" => {
            let mut guard = child_mutex.lock().unwrap();
            if let Some(mut child_guard) = guard.take() {
                info!(pid = ?child_guard.id(), "stop: graceful shutdown");
                child_guard.graceful_stop();
            } else {
                // Geen child onder onze controle. We doen GEEN pkill meer —
                // dat killt andere users hun processen op een multi-user
                // machine. User moet handmatig opruimen via Activity Monitor.
                warn!("stop: no child under our control, refusing pkill");
                let _ = proxy.send_event(UserEvent::LauncherError(
                    "Geen door deze launcher gestarte server gevonden. Eventuele losse governor-http processen moeten handmatig gestopt worden.".to_string(),
                ));
            }
        }
        "open_dashboard" => {
            let url = format!("http://127.0.0.1:{GOVERNOR_PORT}/");
            info!(url = %url, "open_dashboard");
            let _ = Command::new("open").arg(url).status();
        }
        "reset" => {
            thread::spawn(move || {
                let result = post_reset();
                let _ = proxy.send_event(UserEvent::ResetResult {
                    ok: result.is_ok(),
                    count: result.as_ref().map(|n| *n).unwrap_or(0),
                    error: result.err(),
                });
            });
        }
        _ => {
            warn!(action = %action, "unknown IPC action");
        }
    }
}

/// POST http://127.0.0.1:8989/reset via stdlib TCP. Geen extra crate nodig.
fn post_reset() -> Result<usize, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let addr = format!("127.0.0.1:{GOVERNOR_PORT}");
    let mut stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|e: std::net::AddrParseError| e.to_string())?,
        Duration::from_millis(800),
    )
    .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(2000)))
        .ok();
    let req = b"POST /reset HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    stream.write_all(req).map_err(|e| format!("write: {e}"))?;
    let mut buf = Vec::with_capacity(2048);
    stream
        .read_to_end(&mut buf)
        .map_err(|e| format!("read: {e}"))?;
    let resp = String::from_utf8_lossy(&buf);
    if !resp.starts_with("HTTP/1.0 200") && !resp.starts_with("HTTP/1.1 200") {
        let status = resp.lines().next().unwrap_or("").to_string();
        return Err(format!("server: {status}"));
    }
    let body_idx = resp.find("\r\n\r\n").map(|i| i + 4).unwrap_or(resp.len());
    let body = &resp[body_idx..];
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("parse: {e}"))?;
    let count = parsed
        .get("deleted_files")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    Ok(count)
}

fn is_server_up() -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let mut stream = match TcpStream::connect_timeout(
        &format!("127.0.0.1:{GOVERNOR_PORT}").parse().unwrap(),
        Duration::from_millis(300),
    ) {
        Ok(s) => s,
        Err(_) => return false,
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(400)))
        .ok();
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
fn resolve_provider_and_key() -> (String, Option<String>) {
    if let Ok(p) = std::env::var("GOVERNOR_PROVIDER") {
        let key = std::env::var("GOVERNOR_API_KEY").ok();
        return (p, key);
    }
    if let Ok(home) = std::env::var("HOME") {
        let key_path = PathBuf::from(home).join(".config/sovacount/anthropic-key");
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

fn provider_label() -> &'static str {
    if let Ok(home) = std::env::var("HOME") {
        let key_path = PathBuf::from(home).join(".config/sovacount/anthropic-key");
        if key_path.exists() {
            return "anthropic provider";
        }
    }
    "mock provider"
}

/// Zoek `governor-http` op disk. Volgorde:
/// 1. Env override `GOVERNOR_HTTP_BIN` (absolute path)
/// 2. Naast onze eigen launcher binary (Contents/MacOS/ in bundled .app)
/// 3. Sibling in Contents/Resources/ (aanbevolen production-locatie)
/// 4. `~/.local/bin/governor-http` (typisch dev-install via `cp`)
///
/// **Verwijderd in T-G-launcher-prod**: de "5 niveaus omhoog naar
/// target/release/" zoek-loop. Die was nuttig tijdens dev maar gevaarlijk
/// in productie omdat een verkeerd geplaatste .app dan een ander random
/// binary zou kunnen vinden. Voor dev-iteraties gebruik je nu de env-var.
fn locate_governor_http() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GOVERNOR_HTTP_BIN") {
        let path = PathBuf::from(&p);
        if path.exists() {
            return Some(path);
        }
        warn!(path = %p, "GOVERNOR_HTTP_BIN set but file missing");
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("governor-http");
            if p.exists() {
                return Some(p);
            }
            // Production .app layout: launcher in Contents/MacOS/,
            // server in Contents/Resources/governor-http.
            let resources = parent.parent().map(|p| p.join("Resources/governor-http"));
            if let Some(r) = resources {
                if r.exists() {
                    return Some(r);
                }
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".local/bin/governor-http");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Init `tracing` met file-output. Bestand: `~/Library/Logs/SovaCount.log`
/// op macOS (Apple-canonical), of `$XDG_STATE_HOME/sovacount/launcher.log`
/// elders. Faalt silent (eprintln) als logging niet opgezet kan worden —
/// de launcher moet kunnen draaien zonder log-bestand.
fn init_logging() {
    let log_path = if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(&home).join("Library/Logs");
        if std::fs::create_dir_all(&p).is_ok() {
            Some(p.join("SovaCount.log"))
        } else {
            None
        }
    } else {
        None
    };

    let Some(log_path) = log_path else {
        eprintln!("[sovacount-launcher] could not resolve log dir, logging to stderr");
        return;
    };

    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "[sovacount-launcher] could not open log file {}: {e}",
                log_path.display()
            );
            return;
        }
    };

    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_env("SOVACOUNT_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .finish();
    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("[sovacount-launcher] could not install tracing subscriber: {e}");
    }
}

/// Registreer SIGTERM + SIGINT handlers. Beide zetten de shutdown_flag
/// zodat het event-loop een schone exit kan doen.
fn register_signal_handlers(flag: Arc<AtomicBool>) {
    use signal_hook::consts::{SIGINT, SIGTERM};
    for &sig in &[SIGTERM, SIGINT] {
        let flag = Arc::clone(&flag);
        if let Err(e) = signal_hook::flag::register(sig, flag) {
            warn!(signal = sig, error = %e, "could not register signal handler");
        }
    }
}
