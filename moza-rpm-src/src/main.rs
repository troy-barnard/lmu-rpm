/// Main entrypoint for the Moza RPM telemetry bridge.
mod lmu;
mod lmu_config;
mod moza;
mod runtime;

use std::fs::OpenOptions;
use std::io::Write as _;
use std::panic;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{sleep as tokio_sleep, timeout};

use crate::lmu::LmuSharedMemory;
use crate::moza::MozaSerial;
use crate::runtime::{
    clamp_f32, env_flag, read_env_u64_ms, should_log_sample, RuntimeControl, RuntimeControlFile,
    MENU_BUTTON_REFRESH_INTERVAL,
};

/// Entrypoint for the telemetry bridge.
#[tokio::main]
async fn main() {
    let debug = env_flag("MOZA_RPM_DEBUG");
    let sample_log_interval = Duration::from_millis(read_env_u64_ms("MOZA_RPM_SAMPLE_LOG_INTERVAL_MS", 2000));
    let default_control = RuntimeControl::from_env_defaults().sanitize();
    let control_file_path = std::env::var("MOZA_CONTROL_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("moza-rpm-control.env"));
    let mut control_file = RuntimeControlFile::new(control_file_path);
    let mut control = default_control;
    if let Some(from_file) = control_file.poll(default_control) {
        control = from_file;
    }
    if debug {
        panic::set_hook(Box::new(|info| {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("moza-rpm-debug.log") {
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
            .open("moza-rpm-debug.log")
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
    let (control_tx, mut control_rx) = watch::channel(control);

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
                            if let Some(brake_percent) = shared_mem.read_player_brake_percent() {
                                println!("Brake telemetry sample: brake_percent={brake_percent:.1}");
                                log_debug(&format!("Brake telemetry sample: brake_percent={brake_percent:.1}"));
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
