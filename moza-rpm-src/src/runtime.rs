/// Runtime configuration and control for the Moza RPM display.
use std::fs::metadata;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

pub const MENU_BUTTON_REFRESH_INTERVAL: Duration = Duration::from_millis(1500);
pub const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(250);
pub const DEFAULT_RPM_SCALE: f32 = 1.0;
pub const DEFAULT_BRIGHTNESS: f32 = 1.0;
pub const MIN_RPM_SCALE: f32 = 0.6;
pub const MAX_RPM_SCALE: f32 = 1.4;
pub const MIN_BRIGHTNESS: f32 = 0.1;
pub const MAX_BRIGHTNESS: f32 = 1.0;
pub const DEFAULT_FILL_START_PERCENT: f32 = 65.0;
pub const DEFAULT_FILL_END_PERCENT: f32 = 93.0;
pub const MIN_FILL_START_PERCENT: f32 = 40.0;
pub const MAX_FILL_START_PERCENT: f32 = 80.0;
pub const MIN_FILL_END_PERCENT: f32 = 85.0;
pub const MAX_FILL_END_PERCENT: f32 = 95.0;
pub const LED_COUNT: usize = 15;

#[derive(Clone, Copy, Debug)]
pub struct RuntimeControl {
    pub rpm_scale: f32,
    pub brightness: f32,
    pub fill_start_percent: f32,
    pub fill_end_percent: f32,
    pub override_rpm_colors: bool,
    pub enable_button_overrides: bool,
    pub force_button_colors: bool,
    pub menu_force_button_colors: bool,
    pub idle_led_writes: bool,
}

impl RuntimeControl {
    pub fn from_env_defaults() -> Self {
        // The launcher injects environment variables for the bridge startup path, but the
        // runtime still needs sensible defaults for direct/manual execution when those values
        // are absent.
        Self {
            rpm_scale: read_env_f32("MOZA_RPM_SCALE", DEFAULT_RPM_SCALE, MIN_RPM_SCALE, MAX_RPM_SCALE),
            brightness: read_env_f32("MOZA_RPM_BRIGHTNESS", DEFAULT_BRIGHTNESS, MIN_BRIGHTNESS, MAX_BRIGHTNESS),
            fill_start_percent: read_env_f32(
                "MOZA_RPM_FILL_START",
                DEFAULT_FILL_START_PERCENT,
                MIN_FILL_START_PERCENT,
                MAX_FILL_START_PERCENT,
            ),
            fill_end_percent: read_env_f32(
                "MOZA_RPM_FILL_END",
                DEFAULT_FILL_END_PERCENT,
                MIN_FILL_END_PERCENT,
                MAX_FILL_END_PERCENT,
            ),
            override_rpm_colors: env_flag("MOZA_FORCE_RPM_COLORS"),
            enable_button_overrides: env_flag("MOZA_ENABLE_BUTTON_COLOR_OVERRIDES"),
            force_button_colors: env_flag("MOZA_FORCE_BUTTON_COLORS"),
            menu_force_button_colors: env_flag("MOZA_MENU_FORCE_BUTTON_COLORS"),
            idle_led_writes: env_flag("MOZA_IDLE_LED_WRITES"),
        }
    }

    pub fn sanitize(mut self) -> Self {
        // Keep the LED thresholds within a safe window so the bridge never configures an
        // impossible or inverted fill region while the user is adjusting values live.
        self.fill_start_percent = clamp_f32(self.fill_start_percent, MIN_FILL_START_PERCENT, MAX_FILL_START_PERCENT);
        self.fill_end_percent = clamp_f32(self.fill_end_percent, MIN_FILL_END_PERCENT, MAX_FILL_END_PERCENT);
        if self.fill_end_percent <= self.fill_start_percent + 1.0 {
            self.fill_end_percent = (self.fill_start_percent + 1.0).min(MAX_FILL_END_PERCENT);
        }
        if !self.enable_button_overrides {
            self.force_button_colors = false;
            self.menu_force_button_colors = false;
        }
        self
    }
}

pub struct RuntimeControlFile {
    pub path: PathBuf,
    pub last_poll: Instant,
    pub last_modified: Option<SystemTime>,
}

impl RuntimeControlFile {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            last_poll: Instant::now() - CONTROL_POLL_INTERVAL,
            last_modified: None,
        }
    }

    pub fn poll(&mut self, defaults: RuntimeControl) -> Option<RuntimeControl> {
        if self.last_poll.elapsed() < CONTROL_POLL_INTERVAL {
            return None;
        }
        self.last_poll = Instant::now();

        let metadata = metadata(&self.path).ok()?;
        let modified = metadata.modified().ok()?;

        if self.last_modified.as_ref() == Some(&modified) {
            return None;
        }
        self.last_modified = Some(modified);

        Some(parse_runtime_control(&self.path, defaults))
    }
}

pub fn read_env_u64_ms(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

pub fn should_log_sample(
    percent: u8,
    last_logged_percent: Option<u8>,
    last_sample_log: Instant,
    sample_log_interval: Duration,
) -> bool {
    if last_sample_log.elapsed() >= sample_log_interval {
        return true;
    }

    if let Some(previous) = last_logged_percent {
        let delta = previous.abs_diff(percent);

        if delta >= 14 {
            return true;
        }

        if previous > 8 && percent <= 8 {
            return true;
        }
    }

    false
}

pub fn clamp_f32(value: f32, min_value: f32, max_value: f32) -> f32 {
    value.max(min_value).min(max_value)
}

pub fn read_env_f32(name: &str, default: f32, min_value: f32, max_value: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<f32>().ok())
        .map(|value| clamp_f32(value, min_value, max_value))
        .unwrap_or(default)
}

pub fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(raw) => {
            let value = raw.trim().to_ascii_lowercase();
            match value.as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                "" => true,
                _ => true,
            }
        }
        Err(_) => false,
    }
}

pub fn parse_runtime_control(path: &PathBuf, defaults: RuntimeControl) -> RuntimeControl {
    let mut control = defaults;

    if let Ok(raw) = std::fs::read_to_string(path) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };

            let key = key.trim().to_ascii_uppercase();
            let value = value.trim();

            if key == "RPM_SCALE" {
                if let Ok(parsed) = value.parse::<f32>() {
                    control.rpm_scale = clamp_f32(parsed, MIN_RPM_SCALE, MAX_RPM_SCALE);
                }
            } else if key == "BRIGHTNESS" {
                if let Ok(parsed) = value.parse::<f32>() {
                    control.brightness = clamp_f32(parsed, MIN_BRIGHTNESS, MAX_BRIGHTNESS);
                }
            } else if key == "FILL_START_PERCENT" {
                if let Ok(parsed) = value.parse::<f32>() {
                    control.fill_start_percent = clamp_f32(parsed, MIN_FILL_START_PERCENT, MAX_FILL_START_PERCENT);
                }
            } else if key == "FILL_END_PERCENT" {
                if let Ok(parsed) = value.parse::<f32>() {
                    control.fill_end_percent = clamp_f32(parsed, MIN_FILL_END_PERCENT, MAX_FILL_END_PERCENT);
                }
            } else if key == "OVERRIDE_RPM_COLORS" {
                control.override_rpm_colors = parse_bool_like(value, control.override_rpm_colors);
            } else if key == "ENABLE_BUTTON_OVERRIDES" {
                control.enable_button_overrides = parse_bool_like(value, control.enable_button_overrides);
            } else if key == "FORCE_BUTTON_COLORS" {
                control.force_button_colors = parse_bool_like(value, control.force_button_colors);
            } else if key == "MENU_FORCE_BUTTON_COLORS" {
                control.menu_force_button_colors = parse_bool_like(value, control.menu_force_button_colors);
            } else if key == "IDLE_LED_WRITES" {
                control.idle_led_writes = parse_bool_like(value, control.idle_led_writes);
            }
        }
    }

    control.sanitize()
}

fn parse_bool_like(value: &str, fallback: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => fallback,
    }
}
