/// LMU shared-memory telemetry reader for the Moza RPM display.
use std::cell::Cell;

use winapi::shared::ntdef::HANDLE;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{FILE_MAP_READ, MapViewOfFile, UnmapViewOfFile};
use winapi::um::winbase::OpenFileMappingA;

use crate::lmu_config::{
    LMU_BRAKE_CANDIDATE_OFFSETS, LMU_BRAKE_PERCENT_FALLBACK_OFFSET, LMU_BRAKE_PERCENT_OFFSET,
    LMU_ENGINE_MAX_RPM_OFFSET, LMU_ENGINE_RPM_OFFSET, LMU_MAPPING_NAMES, LMU_MAX_VEHICLES,
    LMU_TELEM_INFO_OFFSET, LMU_TELEM_INFO_SIZE, LMU_TELEMETRY_OFFSET,
};

fn normalize_brake_percent_candidate(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    if value > 0.0 && value < 0.05 {
        return None;
    }
    if (0.0..=1.5).contains(&value) {
        return Some(value * 100.0);
    }
    if (1.0..=100.0).contains(&value) {
        return Some(value);
    }
    None
}

pub fn normalize_simetry_brake_percent(value: f64) -> Option<u8> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }

    let percent = if value <= 1.0 {
        value * 100.0
    } else if value <= 100.0 {
        value
    } else {
        return None;
    };

    let clamped = percent.clamp(0.0, 100.0);
    if clamped < 5.0 {
        return Some(0);
    }

    Some(clamped.round() as u8)
}

fn brake_candidate_score(value: f64) -> f64 {
    let mut score = 0.0;
    if value >= 0.0 && value <= 1.5 {
        score += 100.0;
    }
    if value >= 0.0 && value <= 100.0 {
        score += 10.0;
    }
    if value >= 0.0 && value <= 1.5 {
        score += 5.0;
    }
    if value > 1.0 && value <= 100.0 {
        score += 2.0;
    }
    score
}

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

                // Log raw values for debugging
                let raw_0x190 = *(info_base.add(0x190) as *const f64);
                let raw_0x198 = *(info_base.add(0x198) as *const f64);
                let raw_0x1a0 = *(info_base.add(0x1a0) as *const f64);
                eprintln!("[BRAKE_DEBUG] 0x190={:.6} 0x198={:.6} 0x1a0={:.6}", raw_0x190, raw_0x198, raw_0x1a0);

                let read_candidate = |offset: usize| -> Option<f64> {
                    let value = *(info_base.add(offset) as *const f64);
                    let normalized = normalize_brake_percent_candidate(value)?;
                    if normalized >= 0.0 && normalized <= 100.0 {
                        return Some(normalized);
                    }
                    None
                };

                if let Some(value) = read_candidate(LMU_BRAKE_PERCENT_OFFSET)
                    .or_else(|| read_candidate(LMU_BRAKE_PERCENT_FALLBACK_OFFSET))
                {
                    eprintln!("[BRAKE_DEBUG] Found via primary/fallback: {}", value);
                    return Some(value);
                }

                let mut best_value: Option<f64> = None;
                let mut best_score = -f64::INFINITY;

                for &offset in &LMU_BRAKE_CANDIDATE_OFFSETS {
                    let value = *(info_base.add(offset) as *const f64);
                    if !value.is_finite() {
                        continue;
                    }

                    let score = brake_candidate_score(value);
                    if score > best_score {
                        best_score = score;
                        best_value = normalize_brake_percent_candidate(value);
                    }
                }

                if best_value.is_some() {
                    eprintln!("[BRAKE_DEBUG] Found via candidate scan: {:?}", best_value);
                }
                best_value
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

    /// Emits the most plausible brake candidate values from the current player's LMU telemetry,
    /// sorted by their likelihood so the live debug stream reveals the field that moves under pedal input.
    pub fn debug_brake_probe(&self, max_results: usize) -> Vec<(usize, f64)> {
        unsafe {
            let telemetry = self.mapped_view.add(LMU_TELEMETRY_OFFSET);
            let active_vehicles = *telemetry;
            let player_idx = *telemetry.add(1);

            if active_vehicles == 0 {
                return Vec::new();
            }

            let target_idx = if player_idx < active_vehicles && player_idx < LMU_MAX_VEHICLES {
                player_idx
            } else if let Some(preferred_idx) = self.preferred_vehicle_idx.get() {
                preferred_idx
            } else {
                0
            };

            let info_base = telemetry.add(LMU_TELEM_INFO_OFFSET + (target_idx as usize * LMU_TELEM_INFO_SIZE));
            let mut results: Vec<(usize, f64, f64)> = Vec::new();

            for &offset in &LMU_BRAKE_CANDIDATE_OFFSETS {
                let value = *(info_base.add(offset) as *const f32) as f64;
                if !value.is_finite() {
                    continue;
                }
                let score = brake_candidate_score(value);
                if score <= 0.0 {
                    continue;
                }
                if let Some(normalized) = normalize_brake_percent_candidate(value) {
                    results.push((offset, normalized, score));
                }
            }

            results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            results
                .into_iter()
                .take(max_results)
                .map(|(offset, normalized, _)| (offset, normalized))
                .collect()
        }
    }

    /// Emits plausible float-like values from the current player's LMU telemetry block.
    /// This is meant for live debugging while the game is running, to narrow down the real brake field.
    pub fn dump_player_float_candidates(&self, max_results: usize) -> Vec<(usize, f64)> {
        unsafe {
            let telemetry = self.mapped_view.add(LMU_TELEMETRY_OFFSET);
            let active_vehicles = *telemetry;
            let player_idx = *telemetry.add(1);

            if active_vehicles == 0 {
                return Vec::new();
            }

            let target_idx = if player_idx < active_vehicles && player_idx < LMU_MAX_VEHICLES {
                player_idx
            } else if let Some(preferred_idx) = self.preferred_vehicle_idx.get() {
                preferred_idx
            } else {
                0
            };

            let info_base = telemetry.add(LMU_TELEM_INFO_OFFSET + (target_idx as usize * LMU_TELEM_INFO_SIZE));
            let mut results = Vec::new();
            let mut count = 0usize;

            for offset in (0..LMU_TELEM_INFO_SIZE).step_by(4) {
                let value = *(info_base.add(offset) as *const f32) as f64;
                if !value.is_finite() {
                    continue;
                }
                if value >= 0.0 && value <= 150.0 && (value > 0.01 || offset < 256) {
                    results.push((offset, value));
                    count += 1;
                    if count >= max_results {
                        break;
                    }
                }
            }

            results
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{brake_candidate_score, normalize_brake_percent_candidate};
    use crate::lmu_config::LMU_BRAKE_CANDIDATE_OFFSETS;

    #[test]
    fn normalize_brake_percent_candidate_handles_fractional_and_percent_like_values() {
        assert_eq!(normalize_brake_percent_candidate(0.0), Some(0.0));
        assert_eq!(normalize_brake_percent_candidate(0.032), None);
        assert_eq!(normalize_brake_percent_candidate(0.5), Some(50.0));
        assert_eq!(normalize_brake_percent_candidate(12.5), Some(12.5));
        assert_eq!(normalize_brake_percent_candidate(250.0), None);
    }

    #[test]
    fn brake_candidate_score_prefers_realistic_brake_values() {
        assert!(brake_candidate_score(0.5) > brake_candidate_score(999.0));
        assert!(brake_candidate_score(12.5) > brake_candidate_score(999.0));
    }

    #[test]
    fn brake_candidate_offsets_cover_the_known_driver_input_window() {
        assert!(LMU_BRAKE_CANDIDATE_OFFSETS.contains(&0x264));
        assert!(LMU_BRAKE_CANDIDATE_OFFSETS.contains(&0x27c));
        assert!(LMU_BRAKE_CANDIDATE_OFFSETS.iter().all(|&offset| offset <= 0x2f0));
    }

    #[test]
    fn normalize_simetry_brake_percent_converts_common_pedal_inputs() {
        assert_eq!(super::normalize_simetry_brake_percent(0.0), Some(0));
        assert_eq!(super::normalize_simetry_brake_percent(0.032), Some(0));
        assert_eq!(super::normalize_simetry_brake_percent(0.5), Some(50));
        assert_eq!(super::normalize_simetry_brake_percent(0.25), Some(25));
        assert_eq!(super::normalize_simetry_brake_percent(72.0), Some(72));
        assert_eq!(super::normalize_simetry_brake_percent(1.5), Some(0));
        assert_eq!(super::normalize_simetry_brake_percent(-0.1), None);
        assert_eq!(super::normalize_simetry_brake_percent(101.0), None);
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
