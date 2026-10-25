use std::fs::{metadata, OpenOptions};
use std::io::Write as _;
use std::panic;
use std::cell::Cell;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::watch;
use tokio::time::{sleep as tokio_sleep, timeout};
use winapi::shared::ntdef::HANDLE;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{FILE_MAP_READ, MapViewOfFile, UnmapViewOfFile};
use winapi::um::winbase::OpenFileMappingA;

/// Candidate shared-memory mapping names exposed by LMU under Wine/Proton.
///
/// Some environments expose the object in the default namespace (`LMU_Data`)
/// while others expose it in the global namespace (`Global\\LMU_Data`).
const LMU_MAPPING_NAMES: [&[u8]; 2] = [b"LMU_Data\0", b"Global\\LMU_Data\0"];

/// Byte offset from the start of the LMU shared-memory block to telemetry data.
const LMU_TELEMETRY_OFFSET: usize = 128_464;

/// Offset within the telemetry block where per-vehicle telemetry records begin.
const LMU_TELEM_INFO_OFFSET: usize = 4;

/// Size in bytes of a single per-vehicle telemetry record.
const LMU_TELEM_INFO_SIZE: usize = 1_888;

/// Offset within a vehicle telemetry record for current engine RPM (`f64`).
const LMU_ENGINE_RPM_OFFSET: usize = 356;

/// Offset within a vehicle telemetry record for engine max RPM (`f64`).
const LMU_ENGINE_MAX_RPM_OFFSET: usize = 532;

/// Upper bound for valid LMU vehicle indices in shared telemetry.
const LMU_MAX_VEHICLES: u8 = 104;

/// Base packet template for Moza RPM LED bitmask updates.
///
/// Bytes at indices 6 and 7 are populated with LED on/off bits before sending.
/// The final byte (index 10) is replaced with the computed packet checksum.
const RPM_MASK_TEMPLATE: [u8; 11] = [0x7e, 0x06, 0x3f, 0x17, 0x1a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// First color-configuration packet for RPM LEDs.
const RPM_COLOR_PAYLOAD_1: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x00, 0x00, 0x00, 0xff, 0x00, 0x01, 0x80, 0xff, 0x00, 0x02, 0xff, 0xff, 0x00, 0x03, 0xff, 0x00, 0x00, 0x04, 0xff, 0x00, 0x00, 0x00];

/// Second color-configuration packet for RPM LEDs.
const RPM_COLOR_PAYLOAD_2: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x00, 0x05, 0xff, 0x00, 0x00, 0x06, 0xff, 0x00, 0x00, 0x07, 0xff, 0x00, 0x00, 0x08, 0xff, 0x00, 0x00, 0x09, 0xff, 0x00, 0x00, 0x00];

/// Third color-configuration packet for RPM LEDs.
const RPM_COLOR_PAYLOAD_3: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x00, 0x0a, 0xff, 0x7f, 0x00, 0x0b, 0xff, 0x7f, 0x00, 0x0c, 0xff, 0x7f, 0x00, 0x0d, 0x00, 0x00, 0xff, 0x0e, 0x00, 0x00, 0xff, 0x00];

/// First color-configuration packet for button LEDs.
const BTN_COLOR_PAYLOAD_1: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x01, 0x00, 0xff, 0x00, 0x00, 0x01, 0xff, 0x00, 0x00, 0x02, 0xff, 0x00, 0x00, 0x03, 0xff, 0x00, 0x00, 0x04, 0xff, 0x00, 0x00, 0x00];

/// Second color-configuration packet for button LEDs.
const BTN_COLOR_PAYLOAD_2: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x01, 0x05, 0xff, 0x00, 0x00, 0x06, 0xff, 0x00, 0x00, 0x07, 0xff, 0x00, 0x00, 0x08, 0xff, 0x00, 0x00, 0x09, 0xff, 0x00, 0x00, 0x00];

/// Third color-configuration packet for button LEDs.
const BTN_COLOR_PAYLOAD_3: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x01, 0x0a, 0xff, 0x00, 0x00, 0x0b, 0xff, 0x00, 0x00, 0x0c, 0xff, 0x00, 0x00, 0x0d, 0xff, 0x00, 0x00, 0x0e, 0xff, 0x00, 0x00, 0x00];

/// RPM percentage above which the shift lights enter flashing mode.
const LED_FLASH_THRESHOLD: u8 = 95;
const MENU_BUTTON_REFRESH_INTERVAL: Duration = Duration::from_millis(1500);
const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_RPM_SCALE: f32 = 1.0;
const DEFAULT_BRIGHTNESS: f32 = 1.0;
const MIN_RPM_SCALE: f32 = 0.6;
const MAX_RPM_SCALE: f32 = 1.4;
const MIN_BRIGHTNESS: f32 = 0.1;
const MAX_BRIGHTNESS: f32 = 1.0;
const DEFAULT_FILL_START_PERCENT: f32 = 65.0;
const DEFAULT_FILL_END_PERCENT: f32 = 93.0;
const MIN_FILL_START_PERCENT: f32 = 40.0;
const MAX_FILL_START_PERCENT: f32 = 80.0;
const MIN_FILL_END_PERCENT: f32 = 85.0;
const MAX_FILL_END_PERCENT: f32 = 95.0;
const LED_COUNT: usize = 15;

/// Serial connection state for a Moza wheel/base.
///
/// Holds the open serial port and optional flash timing state used to toggle
/// LEDs when RPM enters the flashing threshold region.
struct MozaSerial {
    port: Box<dyn serialport::SerialPort>,
    flash_start: Option<Instant>,
}

#[derive(Clone, Copy, Debug)]
struct RuntimeControl {
    rpm_scale: f32,
    brightness: f32,
    fill_start_percent: f32,
    fill_end_percent: f32,
    override_rpm_colors: bool,
    enable_button_overrides: bool,
    force_button_colors: bool,
    menu_force_button_colors: bool,
    idle_led_writes: bool,
}

impl RuntimeControl {
    fn from_env_defaults() -> Self {
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

    fn sanitize(mut self) -> Self {
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

struct RuntimeControlFile {
    path: PathBuf,
    last_poll: Instant,
    last_modified: Option<SystemTime>,
}

/// Wrapper around LMU shared-memory mapping resources.
///
/// Stores both the mapping handle and the mapped view pointer so they can be
/// safely released when dropped.
struct LmuSharedMemory {
    mapping_handle: HANDLE,
    mapped_view: *const u8,
    preferred_vehicle_idx: Cell<Option<u8>>,
}

impl LmuSharedMemory {
    /// Opens and maps LMU shared memory in read-only mode.
    ///
    /// Tries each candidate mapping name in `LMU_MAPPING_NAMES` until one
    /// succeeds. Returns an error if neither mapping can be opened or mapped.
    fn connect() -> std::io::Result<Self> {
        unsafe {
            let mut mapping_handle: HANDLE = std::ptr::null_mut();
            for name in LMU_MAPPING_NAMES {
                mapping_handle = OpenFileMappingA(FILE_MAP_READ, 0, name.as_ptr() as *const i8);
                if !mapping_handle.is_null() {
                    break;
                }
            }

            if mapping_handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }

            let mapped_view = MapViewOfFile(mapping_handle, FILE_MAP_READ, 0, 0, 0) as *const u8;
            if mapped_view.is_null() {
                let _ = CloseHandle(mapping_handle);
                return Err(std::io::Error::last_os_error());
            }

            Ok(Self {
                mapping_handle,
                mapped_view,
                preferred_vehicle_idx: Cell::new(None),
            })
        }
    }

    /// Reads the local player's current and max RPM from mapped LMU telemetry.
    ///
    /// Returns `None` if no player vehicle is active, indices are invalid, or
    /// parsed RPM values are non-finite/invalid.
    fn read_player_rpm(&self) -> Option<(f64, f64)> {
        unsafe {
            let telemetry = self.mapped_view.add(LMU_TELEMETRY_OFFSET);
            let active_vehicles = *telemetry;
            let player_idx = *telemetry.add(1);

            if active_vehicles == 0 {
                return None;
            }

            let read_slot = |idx: u8| -> Option<(f64, f64)> {
                let info_base = telemetry.add(LMU_TELEM_INFO_OFFSET + (idx as usize * LMU_TELEM_INFO_SIZE));
                let rpm = *(info_base.add(LMU_ENGINE_RPM_OFFSET) as *const f64);
                let max_rpm = *(info_base.add(LMU_ENGINE_MAX_RPM_OFFSET) as *const f64);

                // Ignore clearly invalid/empty slots.
                if !rpm.is_finite() || !max_rpm.is_finite() {
                    return None;
                }
                if max_rpm < 500.0 || rpm < 0.0 {
                    return None;
                }

                Some((rpm, max_rpm))
            };

            if player_idx < active_vehicles && player_idx < LMU_MAX_VEHICLES {
                if let Some(values) = read_slot(player_idx) {
                    self.preferred_vehicle_idx.set(Some(player_idx));
                    return Some(values);
                }
            }

            if let Some(preferred_idx) = self.preferred_vehicle_idx.get() {
                if preferred_idx < active_vehicles && preferred_idx < LMU_MAX_VEHICLES {
                    if let Some(values) = read_slot(preferred_idx) {
                        return Some(values);
                    }
                }
            }

            for idx in 0..active_vehicles {
                if idx >= LMU_MAX_VEHICLES {
                    break;
                }
                if let Some(values) = read_slot(idx) {
                    self.preferred_vehicle_idx.set(Some(idx));
                    return Some(values);
                }
            }

            self.preferred_vehicle_idx.set(None);

            None
        }
    }
}

fn read_env_u64_ms(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn should_log_sample(
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

        // Emit on large step changes so decel/accel transitions are visible without spam.
        if delta >= 14 {
            return true;
        }

        // Emit quickly when RPM drops near idle to avoid stale-looking high samples.
        if previous > 8 && percent <= 8 {
            return true;
        }
    }

    false
}

impl Drop for LmuSharedMemory {
    /// Releases LMU shared-memory resources when the wrapper goes out of scope.
    fn drop(&mut self) {
        unsafe {
            if !self.mapped_view.is_null() {
                let _ = UnmapViewOfFile(self.mapped_view as *const _);
            }
            if !self.mapping_handle.is_null() {
                let _ = CloseHandle(self.mapping_handle);
            }
        }
    }
}

fn clamp_f32(value: f32, min_value: f32, max_value: f32) -> f32 {
    value.max(min_value).min(max_value)
}

fn read_env_f32(name: &str, default: f32, min_value: f32, max_value: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<f32>().ok())
        .map(|value| clamp_f32(value, min_value, max_value))
        .unwrap_or(default)
}

fn scaled_rgb(value: u8, brightness: f32) -> u8 {
    let scaled = (value as f32 * clamp_f32(brightness, MIN_BRIGHTNESS, MAX_BRIGHTNESS)).round();
    scaled.max(0.0).min(255.0) as u8
}

fn apply_brightness_to_color_payload(payload: &mut [u8], brightness: f32) {
    if payload.len() < 9 {
        return;
    }

    let mut i = 7;
    while i + 2 < payload.len() - 1 {
        payload[i] = scaled_rgb(payload[i], brightness);
        payload[i + 1] = scaled_rgb(payload[i + 1], brightness);
        payload[i + 2] = scaled_rgb(payload[i + 2], brightness);
        i += 4;
    }
}

fn parse_runtime_control(path: &PathBuf, defaults: RuntimeControl) -> RuntimeControl {
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

impl RuntimeControlFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            last_poll: Instant::now() - CONTROL_POLL_INTERVAL,
            last_modified: None,
        }
    }

    fn poll(&mut self, defaults: RuntimeControl) -> Option<RuntimeControl> {
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

impl MozaSerial {
    /// Computes Moza packet checksum using the protocol's additive scheme.
    ///
    /// Starts from seed `0x0d` and wraps on overflow while summing all bytes.
    fn checksum(buf: &[u8]) -> u8 {
        let mut ret: u8 = 0x0d;
        for b in buf {
            ret = ret.wrapping_add(*b);
        }
        ret
    }

    /// Writes a packet payload to the serial port.
    fn send_packet(&mut self, payload: &[u8]) -> std::io::Result<()> {
        self.port.write(payload)?;
        Ok(())
    }

    /// Builds and sends an RPM LED bitmask update for the given percent.
    ///
    /// The mask progressively enables LEDs from approximately 65% to 93% RPM.
    fn send_rpm_telemetry_command(&mut self, percent: u8, fill_start_percent: f32, fill_end_percent: f32) -> std::io::Result<()> {
        let mut packet = RPM_MASK_TEMPLATE.to_vec();

        let start = clamp_f32(fill_start_percent, MIN_FILL_START_PERCENT, MAX_FILL_START_PERCENT);
        let end = clamp_f32(fill_end_percent.max(start + 1.0), MIN_FILL_END_PERCENT, MAX_FILL_END_PERCENT);
        let step = (end - start) / ((LED_COUNT - 1) as f32);
        let percent_f = percent as f32;

        for led_idx in 0..LED_COUNT {
            let threshold = start + step * (led_idx as f32);
            if percent_f < threshold {
                continue;
            }

            if led_idx < 8 {
                packet[6] |= 1u8 << led_idx;
            } else {
                packet[7] |= 1u8 << (led_idx - 8);
            }
        }

        packet[10] = Self::checksum(&packet[..10]);
        self.send_packet(&packet)
    }

    fn send_rpm_color_configuration(&mut self, brightness: f32) -> std::io::Result<()> {
        let mut p1 = RPM_COLOR_PAYLOAD_1.to_vec();
        let mut p2 = RPM_COLOR_PAYLOAD_2.to_vec();
        let mut p3 = RPM_COLOR_PAYLOAD_3.to_vec();

        apply_brightness_to_color_payload(&mut p1, brightness);
        apply_brightness_to_color_payload(&mut p2, brightness);
        apply_brightness_to_color_payload(&mut p3, brightness);

        let end1 = p1.len() - 1;
        p1[end1] = Self::checksum(&p1[..end1]);
        let end2 = p2.len() - 1;
        p2[end2] = Self::checksum(&p2[..end2]);
        let end3 = p3.len() - 1;
        p3[end3] = Self::checksum(&p3[..end3]);

        self.send_packet(&p1)?;
        self.send_packet(&p2)?;
        self.send_packet(&p3)?;
        Ok(())
    }

    /// Opens the Moza serial device on `COM1` at 115200 baud.
    pub fn create() -> std::io::Result<Self> {
        let port = serialport::new("COM1", 115200)
            .timeout(Duration::from_millis(100))
            .open()?;
        Ok(MozaSerial { port, flash_start: None })
    }

    /// Sends startup color configuration to RPM/button LEDs when requested.
    ///
    /// Color payloads are checksummed and transmitted only for the groups whose
    /// `force_*_colors` flag is enabled.
    pub fn initialize(&mut self, force_rpm_colors: bool, force_button_colors: bool, brightness: f32) -> std::io::Result<()> {
        sleep(Duration::from_millis(250));

        let mut p4 = BTN_COLOR_PAYLOAD_1.to_vec();
        let mut p5 = BTN_COLOR_PAYLOAD_2.to_vec();
        let mut p6 = BTN_COLOR_PAYLOAD_3.to_vec();

        if force_rpm_colors {
            self.send_rpm_color_configuration(brightness)?;
        }

        if force_button_colors {
            let end4 = p4.len() - 1;
            p4[end4] = Self::checksum(&p4[..end4]);
            let end5 = p5.len() - 1;
            p5[end5] = Self::checksum(&p5[..end5]);
            let end6 = p6.len() - 1;
            p6[end6] = Self::checksum(&p6[..end6]);
            self.send_packet(&p4)?;
            self.send_packet(&p5)?;
            self.send_packet(&p6)?;
        }

        sleep(Duration::from_millis(250));
        Ok(())
    }

    /// Re-applies bridge button color payloads.
    ///
    /// Intended for menu/idle refresh when users opt in to force fully lit
    /// button colors while telemetry is not yet flowing.
    fn refresh_button_colors(&mut self) -> std::io::Result<()> {
        let mut p4 = BTN_COLOR_PAYLOAD_1.to_vec();
        let mut p5 = BTN_COLOR_PAYLOAD_2.to_vec();
        let mut p6 = BTN_COLOR_PAYLOAD_3.to_vec();

        let end4 = p4.len() - 1;
        p4[end4] = Self::checksum(&p4[..end4]);
        let end5 = p5.len() - 1;
        p5[end5] = Self::checksum(&p5[..end5]);
        let end6 = p6.len() - 1;
        p6[end6] = Self::checksum(&p6[..end6]);

        self.send_packet(&p4)?;
        self.send_packet(&p5)?;
        self.send_packet(&p6)?;
        Ok(())
    }

    /// Updates RPM LEDs, applying a blink effect above `LED_FLASH_THRESHOLD`.
    ///
    /// When in the flashing zone, output alternates between the real value and
    /// zero on a timed cadence to create a shift-light blink.
    pub fn update_rpm_telemetry(&mut self, actual_percent: u8, fill_start_percent: f32, fill_end_percent: f32) -> std::io::Result<()> {
        let mut percent = actual_percent;
        if percent > LED_FLASH_THRESHOLD {
            match self.flash_start {
                Some(s) => {
                    if (s.elapsed().as_millis() >> 7) & 1 == 1 {
                        percent = 0;
                    }
                }
                None => {
                    percent = 0;
                    self.flash_start = Some(Instant::now());
                }
            }
        } else if self.flash_start.is_some() {
            self.flash_start = None;
        }

        self.send_rpm_telemetry_command(percent, fill_start_percent, fill_end_percent)
    }
}

fn env_flag(name: &str) -> bool {
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

/// Entrypoint for the telemetry bridge.
///
/// Initializes serial output, starts a background LED update task, then
/// continuously reads RPM telemetry from either Assetto Corsa Evo (when `acevo`
/// CLI arg is present) or LMU native shared memory and forwards scaled RPM
/// percentages to the Moza device.
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

    let (tx, mut rx) = watch::channel(0u8);
    let (control_tx, mut control_rx) = watch::channel(control);

    let mut moza = MozaSerial::create().expect("Failed to open port");
    moza
        .initialize(control.override_rpm_colors, control.force_button_colors, control.brightness)
        .expect("Failed to initialize");

    tokio::spawn(async move {
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
                },
                Ok(_) => {
                    telemetry_seen = true;
                    moza
                        .update_rpm_telemetry(
                            *rx.borrow_and_update(),
                            active_control.fill_start_percent,
                            active_control.fill_end_percent,
                        )
                        .expect("Failed to send telemetry");

                    // Once live telemetry is flowing, stop menu refresh writes so wheel profile
                    // button colors are no longer overridden while on track.
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
