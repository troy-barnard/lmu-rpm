//! Moza RPM telemetry bridge binary.
//!
//! This binary bridges Le Mans Ultimate telemetry (engine RPM and brake pedal data) from LMU's
//! Windows shared memory over serial to a Moza R9 steering wheel base, controlling the RPM
//! display LEDs and button colors.
//!
//! ## How It Works
//!
//! 1. **Initialization**: Load configuration from `config/secrets.json`, set up runtime state,
//!    and open serial connection to the Moza wheel.
//!
//! 2. **Telemetry Reading** (separate blocking tasks):
//!    - Connect to LMU shared memory (Windows memory-mapped file via Wine/Proton)
//!    - Poll for RPM and brake values at known offsets in the shared memory block
//!    - Send updates through async-safe watch channels
//!
//! 3. **Serial Control** (async task):
//!    - Listen on watch channels for RPM/brake updates
//!    - Apply runtime configuration (RPM scale, brightness, fill range)
//!    - Build serial packets according to Moza's protocol
//!    - Send packets to wheel to update LEDs
//!    - Watch for runtime control file changes and reload configuration
//!
//! 4. **Status GUI** (separate process):
//!    - Launch an optional Python/Tk GUI that tails the status log and displays current state
//!    - User can adjust RPM scale, brightness, and fill range on the fly
//!    - Changes are written to `config/moza-rpm-control.env` and loaded by the bridge
//!
//! ## Environment Variables
//!
//! See `docs/README.md` for a complete list. Key ones:
//! - `MOZA_RPM_DEBUG=1`: Enable verbose debug logging to `logs/moza-rpm-debug.log`
//! - `MOZA_RPM_SCALE=1.0`: Multiply RPM % by this factor (0.6 to 1.4)
//! - `MOZA_RPM_BRIGHTNESS=1.0`: LED brightness (0.1 to 1.0)
//! - `MOZA_RPM_FILL_START=65.0`: RPM % at which LEDs start filling (40 to 80)
//! - `MOZA_RPM_FILL_END=93.0`: RPM % at which all LEDs are on (85 to 95)
//!
//! ## Log Files
//!
//! - `logs/moza-rpm-status.log`: Status updates (vehicle connected, RPM samples, etc.)
//! - `logs/moza-rpm-launch.log`: Full stderr output from startup/shutdown
//! - `logs/moza-rpm-debug.log`: Detailed debug trace (when `MOZA_RPM_DEBUG=1`)
//!
//! ## Configuration Files
//!
//! - `config/secrets.json`: Steam library paths, Proton installation, wheel serial device
//! - `config/moza-rpm-control.env`: Runtime state (updated by status GUI)

use moza_rpm::{lmu::*, moza::*, runtime::*};
use std::fs::OpenOptions;
use std::io::Write as _;
use std::panic;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{sleep as tokio_sleep, timeout};

/// Entrypoint for the telemetry bridge.
///
/// This function sets up the async runtime, initializes logging, spawns telemetry reading tasks,
/// and runs the main event loop that sends LED commands to the Moza wheel.
#[tokio::main]
async fn main() {
    let debug = env_flag("MOZA_RPM_DEBUG");
    let sample_log_interval = Duration::from_millis(read_env_u64_ms("MOZA_RPM_SAMPLE_LOG_INTERVAL_MS", 2000));
    let default_control = RuntimeControl::from_env_defaults().sanitize();
    let control_file_path = std::env::var("MOZA_CONTROL_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config/moza-rpm-control.env"));
    let mut control_file = RuntimeControlFile::new(control_file_path);
    let mut control = default_control;
    if let Some(from_file) = control_file.poll(default_control) {
        control = from_file;
    }
    if debug {
        panic::set_hook(Box::new(|info| {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("logs/moza-rpm-debug.log") {
                let _ = writeln!(file, "panic: {info}");
                let _ = file.flush();
            }
        }));
    }

    // Optional debug log keeps a trace of the bridge lifecycle and telemetry values when
    // you need to diagnose why LMU has not exposed a valid shared-memory sample.
    let mut debug_log = if debug {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open("logs/moza-rpm-debug.log")
            .ok()
    } else {
        None
    };
    let mut log_debug = |line: &str| {
        if let Some(file) = debug_log.as_mut() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    };

    if debug {
        log_debug("debug logging enabled");
    }

    // Telemetry values are passed through a watch channel so the main loop can update the
    // display without blocking the serial writer or the LMU reader.
    let (tx, mut rx) = watch::channel(0u8);
    let (brake_tx, _brake_rx) = watch::channel(None::<u8>);
    let (control_tx, mut control_rx) = watch::channel(control);

    let mut brake_debug_log = if debug {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open("moza-rpm-debug.log")
            .ok()
    } else {
        None
    };
    let mut brake_log_debug = move |line: &str| {
        if let Some(file) = brake_debug_log.as_mut() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    };

    tokio::task::spawn_blocking(move || {
        let mut last_logged_brake_percent: Option<u8> = None;
        let mut last_sample_log = Instant::now() - sample_log_interval;

        println!("Brake telemetry task started (LMU shared memory path)");
        brake_log_debug("Brake telemetry task started (LMU shared memory path)");

        loop {
            match LmuSharedMemory::connect() {
                Ok(shared_mem) => {
                    println!("Brake telemetry: connected to LMU shared memory");
                    brake_log_debug("Brake telemetry: connected to LMU shared memory");
                    loop {
                        if let Some(brake_raw) = shared_mem.read_player_brake_percent() {
                            brake_log_debug(&format!("[RAW] brake value: {}", brake_raw));
                            if let Some(percent) = normalize_simetry_brake_percent(brake_raw) {
                                let _ = brake_tx.send(Some(percent));
                                if should_log_brake_sample(percent, last_logged_brake_percent, last_sample_log, sample_log_interval) {
                                    println!("Brake telemetry sample: brake_percent={percent}");
                                    brake_log_debug(&format!("Brake telemetry sample: brake_percent={percent}"));
                                    last_sample_log = Instant::now();
                                    last_logged_brake_percent = Some(percent);
                                }
                            } else {
                                // Out of range; treat as zero
                                let _ = brake_tx.send(Some(0));
                                if should_log_brake_sample(0, last_logged_brake_percent, last_sample_log, sample_log_interval) {
                                    println!("Brake telemetry sample: brake_percent=0");
                                    brake_log_debug("Brake telemetry sample: brake_percent=0");
                                    last_sample_log = Instant::now();
                                    last_logged_brake_percent = Some(0);
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Err(_) => {
                    brake_log_debug("Brake telemetry: LMU shared memory not found");
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    });

    let mut moza = MozaSerial::create().expect("Failed to open port");
    moza
        .initialize(control.override_rpm_colors, control.force_button_colors, control.brightness)
        .expect("Failed to initialize");

    tokio::spawn(async move {
        // This background task owns the actual wheel updates and keeps button LEDs refreshed
        // while the game is in menus or waiting for telemetry to arrive.
        let mut last_button_refresh = Instant::now() - MENU_BUTTON_REFRESH_INTERVAL;
        let mut telemetry_seen = false;
        let mut active_control = *control_rx.borrow();
        let mut menu_refresh_enabled = !telemetry_seen
            && active_control.enable_button_overrides
            && active_control.menu_force_button_colors;

        loop {
            if let Ok(true) = control_rx.has_changed() {
                active_control = *control_rx.borrow_and_update();
                menu_refresh_enabled = !telemetry_seen
                    && active_control.enable_button_overrides
                    && active_control.menu_force_button_colors;

                if active_control.override_rpm_colors {
                    moza
                        .send_rpm_color_configuration(active_control.brightness)
                        .expect("Failed to apply RPM brightness");
                }

                if active_control.enable_button_overrides && active_control.force_button_colors {
                    moza.refresh_button_colors().expect("Failed to refresh button colors");
                }
            }

            match timeout(Duration::from_millis(100), rx.changed()).await {
                Err(_) => {
                    if active_control.idle_led_writes {
                        moza
                            .send_rpm_telemetry_command(
                                0x03,
                                active_control.fill_start_percent,
                                active_control.fill_end_percent,
                            )
                            .expect("Failed to send telemetry");
                    }
                    if menu_refresh_enabled && last_button_refresh.elapsed() >= MENU_BUTTON_REFRESH_INTERVAL {
                        moza.refresh_button_colors().expect("Failed to refresh button colors");
                        last_button_refresh = Instant::now();
                    }
                    // Poll for button input
                    let _ = moza.try_read_button_input();
                }
                Ok(_) => {
                    telemetry_seen = true;
                    moza
                        .update_rpm_telemetry(
                            *rx.borrow_and_update(),
                            active_control.fill_start_percent,
                            active_control.fill_end_percent,
                        )
                        .expect("Failed to send telemetry");

                    if menu_refresh_enabled {
                        menu_refresh_enabled = false;
                    }
                    // Poll for button input
                    let _ = moza.try_read_button_input();
                }
            }
        }
    });

    if std::env::args().any(|arg| arg == "acevo") {
        let mut saw_rpm_sample = false;
        let mut last_sample_log = Instant::now() - sample_log_interval;
        let mut last_logged_percent: Option<u8> = None;
        loop {
            println!("Starting connection to Assetto Corsa Evo...");
            log_debug("Starting connection to Assetto Corsa Evo...");
            let mut client = match timeout(Duration::from_secs(5), simetry::assetto_corsa_evo::Client::connect()).await {
                Ok(client) => client,
                Err(_) => {
                    log_debug("Assetto Corsa Evo shared memory not found (timeout 5s)");
                    continue;
                }
            };
            println!("Connected!");
            log_debug("Connected!");
            while let Some(state) = client.next_sim_state().await {
                let rpm = state.rpm() as f64;
                let max_rpm = state.max_rpm() as f64;
                if let Some(updated) = control_file.poll(default_control) {
                    control = updated;
                    let _ = control_tx.send(control);
                }
                let percent = if max_rpm > 0.0 {
                    let raw_percent = 100.0 * rpm / max_rpm;
                    clamp_f32((raw_percent as f32) * control.rpm_scale, 0.0, 100.0) as u8
                } else {
                    0
                };
                if !saw_rpm_sample {
                    println!("RPM telemetry active.");
                    log_debug("RPM telemetry active.");
                    saw_rpm_sample = true;
                }
                if should_log_sample(percent, last_logged_percent, last_sample_log, sample_log_interval) {
                    println!("RPM telemetry sample: rpm={rpm:.0} max_rpm={max_rpm:.0} percent={percent}");
                    log_debug(&format!("RPM telemetry sample: rpm={rpm:.0} max_rpm={max_rpm:.0} percent={percent}"));
                    last_sample_log = Instant::now();
                    last_logged_percent = Some(percent);
                }
                if debug {
                    println!("telemetry rpm={rpm:.0} max_rpm={max_rpm:.0} percent={percent}");
                    log_debug(&format!("telemetry rpm={rpm:.0} max_rpm={max_rpm:.0} percent={percent}"));
                }
                tx.send(percent).expect("Failed to send telemetry");
            }
            println!("Connection finished!");
            log_debug("Connection finished!");
        }
    } else {
        let mut saw_rpm_sample = false;
        let mut last_sample_log = Instant::now() - sample_log_interval;
        let mut last_logged_percent: Option<u8> = None;
        loop {
            println!("Starting connection to LMU native shared memory...");
            log_debug("Starting connection to LMU native shared memory...");

            match LmuSharedMemory::connect() {
                Ok(shared_mem) => {
                    println!("Connected!");
                    log_debug("Connected to LMU native shared memory!");
                    loop {
                        if let Some((rpm, max_rpm)) = shared_mem.read_player_rpm() {
                            if let Some(updated) = control_file.poll(default_control) {
                                control = updated;
                                let _ = control_tx.send(control);
                            }
                            let percent = if max_rpm > 0.0 {
                                let raw_percent = 100.0 * rpm / max_rpm;
                                clamp_f32((raw_percent as f32) * control.rpm_scale, 0.0, 100.0) as u8
                            } else {
                                0
                            };
                            if !saw_rpm_sample {
                                println!("RPM telemetry active.");
                                log_debug("RPM telemetry active.");
                                saw_rpm_sample = true;
                            }
                            if should_log_sample(percent, last_logged_percent, last_sample_log, sample_log_interval) {
                                println!("RPM telemetry sample: rpm={rpm:.0} max_rpm={max_rpm:.0} percent={percent}");
                                log_debug(&format!("RPM telemetry sample: rpm={rpm:.0} max_rpm={max_rpm:.0} percent={percent}"));
                                last_sample_log = Instant::now();
                                last_logged_percent = Some(percent);
                            }
                            if debug {
                                log_debug(&format!("telemetry rpm={rpm:.0} max_rpm={max_rpm:.0} percent={percent}"));
                            }
                            tx.send(percent).expect("Failed to send telemetry");
                        }
                        tokio_sleep(Duration::from_millis(10)).await;
                    }
                }
                Err(_) => {
                    log_debug("LMU native shared memory not found.");
                    log_debug("Tip: ensure LMU is running and you are in-session/on-track.");
                    tokio_sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}
