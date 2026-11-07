/// LMU shared-memory telemetry reader for the Moza RPM display.
use std::cell::Cell;

use winapi::shared::ntdef::HANDLE;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{FILE_MAP_READ, MapViewOfFile, UnmapViewOfFile};
use winapi::um::winbase::OpenFileMappingA;

use crate::lmu_config::{
    LMU_BRAKE_CANDIDATE_OFFSETS, LMU_ENGINE_MAX_RPM_OFFSET, LMU_ENGINE_RPM_OFFSET, LMU_MAPPING_NAMES,
    LMU_MAX_VEHICLES, LMU_TELEM_INFO_OFFSET, LMU_TELEM_INFO_SIZE, LMU_TELEMETRY_OFFSET,
};

/// Wrapper around LMU shared-memory mapping resources.
pub struct LmuSharedMemory {
    mapping_handle: HANDLE,
    mapped_view: *const u8,
    preferred_vehicle_idx: Cell<Option<u8>>,
}

impl LmuSharedMemory {
    /// Opens and maps LMU shared memory in read-only mode.
    ///
    /// The game exposes the telemetry block under either LMU_Data or Global\LMU_Data,
    /// depending on the Proton/Wine environment. We probe both names and keep the first
    /// successful mapping so the bridge survives version and prefix differences.
    pub fn connect() -> std::io::Result<Self> {
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
    /// The telemetry block is sparse while the session is still starting or when a driver
    /// changes, so we fall back through the active vehicle list until a valid RPM/max-RPM
    /// pair is found instead of assuming the player index is always stable.
    pub fn read_player_rpm(&self) -> Option<(f64, f64)> {
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

    /// Tries to read a brake input value from candidate offsets in the shared-memory record.
    ///
    /// LMU does not yet expose a fixed brake offset in this project, so the probe scans for the
    /// most plausible normalized float-like value in the per-vehicle block. This is meant to be a
    /// discovery pass that can be narrowed once the correct telemetry field is identified.
    pub fn read_player_brake_percent(&self) -> Option<f64> {
        unsafe {
            let telemetry = self.mapped_view.add(LMU_TELEMETRY_OFFSET);
            let active_vehicles = *telemetry;
            let player_idx = *telemetry.add(1);

            if active_vehicles == 0 {
                return None;
            }

            let read_slot = |idx: u8| -> Option<f64> {
                let info_base = telemetry.add(LMU_TELEM_INFO_OFFSET + (idx as usize * LMU_TELEM_INFO_SIZE));
                let mut best_value: Option<f64> = None;
                let mut best_score = -f64::INFINITY;

                for &offset in &LMU_BRAKE_CANDIDATE_OFFSETS {
                    let value = *(info_base.add(offset) as *const f32) as f64;
                    if !value.is_finite() {
                        continue;
                    }

                    let mut score = 0.0;
                    if (0.0..=1.0).contains(&value) {
                        score += 100.0;
                    }
                    if (0.0..=100.0).contains(&value) {
                        score += 10.0;
                    }
                    if value >= 0.0 && value <= 1.5 {
                        score += 5.0;
                    }
                    if value > 1.0 && value <= 100.0 {
                        score += 2.0;
                    }

                    if score > best_score {
                        best_score = score;
                        best_value = Some(value);
                    }
                }

                match best_value {
                    Some(value) if value >= 0.0 && value <= 1.5 => Some(value * 100.0),
                    Some(value) if value > 1.0 && value <= 100.0 => Some(value),
                    _ => None,
                }
            };

            if player_idx < active_vehicles && player_idx < LMU_MAX_VEHICLES {
                if let Some(value) = read_slot(player_idx) {
                    self.preferred_vehicle_idx.set(Some(player_idx));
                    return Some(value);
                }
            }

            if let Some(preferred_idx) = self.preferred_vehicle_idx.get() {
                if preferred_idx < active_vehicles && preferred_idx < LMU_MAX_VEHICLES {
                    if let Some(value) = read_slot(preferred_idx) {
                        return Some(value);
                    }
                }
            }

            for idx in 0..active_vehicles {
                if idx >= LMU_MAX_VEHICLES {
                    break;
                }
                if let Some(value) = read_slot(idx) {
                    self.preferred_vehicle_idx.set(Some(idx));
                    return Some(value);
                }
            }

            self.preferred_vehicle_idx.set(None);
            None
        }
    }
}

impl Drop for LmuSharedMemory {
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
