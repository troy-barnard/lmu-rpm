/// Moza structures and serial communication for the Moza RPM display.
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::runtime::{
    clamp_f32, LED_COUNT, MAX_BRIGHTNESS, MIN_BRIGHTNESS, MIN_FILL_END_PERCENT, MIN_FILL_START_PERCENT,
    MAX_FILL_END_PERCENT, MAX_FILL_START_PERCENT,
};

/// Base packet template for Moza RPM LED bitmask updates.
///
/// Bytes at indices 6 and 7 are populated with LED on/off bits before sending.
/// The final byte (index 10) is replaced with the computed packet checksum.
pub const RPM_MASK_TEMPLATE: [u8; 11] = [0x7e, 0x06, 0x3f, 0x17, 0x1a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// First color-configuration packet for RPM LEDs.
pub const RPM_COLOR_PAYLOAD_1: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x00, 0x00, 0x00, 0xff, 0x00, 0x01, 0x80, 0xff, 0x00, 0x02, 0xff, 0xff, 0x00, 0x03, 0xff, 0x00, 0x00, 0x04, 0xff, 0x00, 0x00, 0x00];

/// Second color-configuration packet for RPM LEDs.
pub const RPM_COLOR_PAYLOAD_2: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x00, 0x05, 0xff, 0x00, 0x00, 0x06, 0xff, 0x00, 0x00, 0x07, 0xff, 0x00, 0x00, 0x08, 0xff, 0x00, 0x00, 0x09, 0xff, 0x00, 0x00, 0x00];

/// Third color-configuration packet for RPM LEDs.
pub const RPM_COLOR_PAYLOAD_3: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x00, 0x0a, 0xff, 0x7f, 0x00, 0x0b, 0xff, 0x7f, 0x00, 0x0c, 0xff, 0x7f, 0x00, 0x0d, 0x00, 0x00, 0xff, 0x0e, 0x00, 0x00, 0xff, 0x00];

/// First color-configuration packet for button LEDs.
pub const BTN_COLOR_PAYLOAD_1: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x01, 0x00, 0xff, 0x00, 0x00, 0x01, 0xff, 0x00, 0x00, 0x02, 0xff, 0x00, 0x00, 0x03, 0xff, 0x00, 0x00, 0x04, 0xff, 0x00, 0x00, 0x00];

/// Second color-configuration packet for button LEDs.
pub const BTN_COLOR_PAYLOAD_2: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x01, 0x05, 0xff, 0x00, 0x00, 0x06, 0xff, 0x00, 0x00, 0x07, 0xff, 0x00, 0x00, 0x08, 0xff, 0x00, 0x00, 0x09, 0xff, 0x00, 0x00, 0x00];

/// Third color-configuration packet for button LEDs.
pub const BTN_COLOR_PAYLOAD_3: [u8; 27] = [0x7e, 0x16, 0x3f, 0x17, 0x19, 0x01, 0x0a, 0xff, 0x00, 0x00, 0x0b, 0xff, 0x00, 0x00, 0x0c, 0xff, 0x00, 0x00, 0x0d, 0xff, 0x00, 0x00, 0x0e, 0xff, 0x00, 0x00, 0x00];

/// RPM percentage above which the shift lights enter flashing mode.
pub const LED_FLASH_THRESHOLD: u8 = 95;

/// Serial connection state for a Moza wheel/base.
///
/// Holds the open serial port and optional flash timing state used to toggle
/// LEDs when RPM enters the flashing threshold region.
pub struct MozaSerial {
    pub port: Box<dyn serialport::SerialPort>,
    pub flash_start: Option<Instant>,
}

impl MozaSerial {
    /// Computes Moza packet checksum using the protocol's additive scheme.
    pub fn checksum(buf: &[u8]) -> u8 {
        let mut ret: u8 = 0x0d;
        for b in buf {
            ret = ret.wrapping_add(*b);
        }
        ret
    }

    /// Writes a packet payload to the serial port.
    pub fn send_packet(&mut self, payload: &[u8]) -> std::io::Result<()> {
        self.port.write(payload)?;
        Ok(())
    }

    /// Builds and sends an RPM LED bitmask update for the given percent.
    pub fn send_rpm_telemetry_command(
        &mut self,
        percent: u8,
        fill_start_percent: f32,
        fill_end_percent: f32,
    ) -> std::io::Result<()> {
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

    pub fn send_rpm_color_configuration(&mut self, brightness: f32) -> std::io::Result<()> {
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
        Ok(MozaSerial {
            port,
            flash_start: None,
        })
    }

    /// Sends startup color configuration to RPM/button LEDs when requested.
    pub fn initialize(
        &mut self,
        force_rpm_colors: bool,
        force_button_colors: bool,
        brightness: f32,
    ) -> std::io::Result<()> {
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
    pub fn refresh_button_colors(&mut self) -> std::io::Result<()> {
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
    pub fn update_rpm_telemetry(
        &mut self,
        actual_percent: u8,
        fill_start_percent: f32,
        fill_end_percent: f32,
    ) -> std::io::Result<()> {
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
