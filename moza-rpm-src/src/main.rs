use std::fs::OpenOptions;
use std::io::Write as _;
use std::panic;
use std::thread::sleep;
use std::time::{Duration, Instant};
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

/// RPM percentage above which the shift lights enter flashing mode.
const LED_FLASH_THRESHOLD: u8 = 95;

/// Serial connection state for a Moza wheel/base.
///
/// Holds the open serial port and optional flash timing state used to toggle
/// LEDs when RPM enters the flashing threshold region.
struct MozaSerial {
    port: Box<dyn serialport::SerialPort>,
    flash_start: Option<Instant>,
}

/// Wrapper around LMU shared-memory mapping resources.
///
/// Stores both the mapping handle and the mapped view pointer so they can be
/// safely released when dropped.
struct LmuSharedMemory {
    mapping_handle: HANDLE,
    mapped_view: *const u8,
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
            let player_has_vehicle = *telemetry.add(2) != 0;

            if !player_has_vehicle || active_vehicles == 0 {
                return None;
            }

            if player_idx >= active_vehicles || player_idx >= LMU_MAX_VEHICLES {
                return None;
            }

            let info_base = telemetry.add(LMU_TELEM_INFO_OFFSET + (player_idx as usize * LMU_TELEM_INFO_SIZE));
            let rpm = *(info_base.add(LMU_ENGINE_RPM_OFFSET) as *const f64);
            let max_rpm = *(info_base.add(LMU_ENGINE_MAX_RPM_OFFSET) as *const f64);

            if rpm.is_finite() && max_rpm.is_finite() && max_rpm >= 0.0 {
                Some((rpm, max_rpm))
            } else {
                None
            }
        }
    }
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
    fn send_rpm_telemetry_command(&mut self, percent: u8) -> std::io::Result<()> {
        let mut packet = RPM_MASK_TEMPLATE.to_vec();
        if percent >= 65 {
            packet[6] |= 1u8 << 0;
        }
        if percent >= 67 {
            packet[6] |= 1u8 << 1;
        }
        if percent >= 69 {
            packet[6] |= 1u8 << 2;
        }
        if percent >= 71 {
            packet[6] |= 1u8 << 3;
        }
        if percent >= 73 {
            packet[6] |= 1u8 << 4;
        }
        if percent >= 75 {
            packet[6] |= 1u8 << 5;
        }
        if percent >= 77 {
            packet[6] |= 1u8 << 6;
        }
        if percent >= 79 {
            packet[6] |= 1u8 << 7;
        }
        if percent >= 81 {
            packet[7] |= 1u8 << 0;
        }
        if percent >= 83 {
            packet[7] |= 1u8 << 1;
        }
        if percent >= 85 {
            packet[7] |= 1u8 << 2;
        }
        if percent >= 87 {
            packet[7] |= 1u8 << 3;
        }
        if percent >= 89 {
            packet[7] |= 1u8 << 4;
        }
        if percent >= 91 {
            packet[7] |= 1u8 << 5;
        }
        if percent >= 93 {
            packet[7] |= 1u8 << 6;
        }
        packet[10] = Self::checksum(&packet[..10]);
        self.send_packet(&packet)
    }

    /// Opens the Moza serial device on `COM1` at 115200 baud.
    pub fn create() -> std::io::Result<Self> {
        let port = serialport::new("COM1", 115200)
            .timeout(Duration::from_millis(100))
            .open()?;
        Ok(MozaSerial { port, flash_start: None })
    }

    /// Sends button LED telemetry updates.
    ///
    /// Currently a stub because button state control is not implemented yet.
    fn send_btn_telemetry_command(&mut self, _leds: u16) -> std::io::Result<()> {
        Ok(())
    }

    /// Sends startup color configuration to RPM/button LEDs when requested.
    ///
    /// Color payloads are checksummed and transmitted only for the groups whose
    /// `force_*_colors` flag is enabled.
    pub fn initialize(&mut self, force_rpm_colors: bool, force_button_colors: bool) -> std::io::Result<()> {
        sleep(Duration::from_millis(250));

        let mut p1 = RPM_COLOR_PAYLOAD_1.to_vec();
        let mut p2 = RPM_COLOR_PAYLOAD_2.to_vec();
        let mut p3 = RPM_COLOR_PAYLOAD_3.to_vec();
        let mut p4 = BTN_COLOR_PAYLOAD_1.to_vec();
        let mut p5 = BTN_COLOR_PAYLOAD_2.to_vec();

        if force_rpm_colors {
            let end1 = p1.len() - 1;
            p1[end1] = Self::checksum(&p1[..end1]);
            let end2 = p2.len() - 1;
            p2[end2] = Self::checksum(&p2[..end2]);
            let end3 = p3.len() - 1;
            p3[end3] = Self::checksum(&p3[..end3]);

            self.send_packet(&p1)?;
            self.send_packet(&p2)?;
            self.send_packet(&p3)?;
        }

        if force_button_colors {
            let end4 = p4.len() - 1;
            p4[end4] = Self::checksum(&p4[..end4]);
            let end5 = p5.len() - 1;
            p5[end5] = Self::checksum(&p5[..end5]);
            self.send_packet(&p4)?;
            self.send_packet(&p5)?;
        }

        sleep(Duration::from_millis(250));
        Ok(())
    }

    /// Updates RPM LEDs, applying a blink effect above `LED_FLASH_THRESHOLD`.
    ///
    /// When in the flashing zone, output alternates between the real value and
    /// zero on a timed cadence to create a shift-light blink.
    pub fn update_rpm_telemetry(&mut self, actual_percent: u8) -> std::io::Result<()> {
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

        self.send_rpm_telemetry_command(percent)
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
    let debug = std::env::var_os("MOZA_RPM_DEBUG").is_some();
    let force_rpm_colors = std::env::var_os("MOZA_FORCE_RPM_COLORS").is_some();
    let force_button_colors = std::env::var_os("MOZA_FORCE_BUTTON_COLORS").is_some();
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

    let mut moza = MozaSerial::create().expect("Failed to open port");
    moza.initialize(force_rpm_colors, force_button_colors).expect("Failed to initialize");

    tokio::spawn(async move {
        loop {
            match timeout(Duration::from_millis(100), rx.changed()).await {
                Err(_) => {
                    moza.send_rpm_telemetry_command(0x03).expect("Failed to send telemetry");
                    moza.send_btn_telemetry_command(0x3ff).expect("Failed to send telemetry");
                },
                Ok(_) => moza.update_rpm_telemetry(*rx.borrow_and_update()).expect("Failed to send telemetry"),
            }
        }
    });

    if std::env::args().any(|arg| arg == "acevo") {
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
                let percent = if max_rpm > 0.0 { (100.0 * rpm / max_rpm) as u8 } else { 0 };
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
        loop {
            println!("Starting connection to LMU native shared memory...");
            log_debug("Starting connection to LMU native shared memory...");

            match LmuSharedMemory::connect() {
                Ok(shared_mem) => {
                    println!("Connected!");
                    log_debug("Connected to LMU native shared memory!");
                    loop {
                        if let Some((rpm, max_rpm)) = shared_mem.read_player_rpm() {
                            let percent = if max_rpm > 0.0 { (100.0 * rpm / max_rpm) as u8 } else { 0 };
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
