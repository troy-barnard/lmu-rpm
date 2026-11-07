/// Candidate shared-memory mapping names exposed by LMU under Wine/Proton.
///
/// Some environments expose the object in the default namespace (`LMU_Data`)
/// while others expose it in the global namespace (`Global\\LMU_Data`).
pub const LMU_MAPPING_NAMES: [&[u8]; 2] = [b"LMU_Data\0", b"Global\\LMU_Data\0"];

/// Byte offset from the start of the LMU shared-memory block to telemetry data.
pub const LMU_TELEMETRY_OFFSET: usize = 128_464;

/// Offset within the telemetry block where per-vehicle telemetry records begin.
pub const LMU_TELEM_INFO_OFFSET: usize = 4;

/// Size in bytes of a single per-vehicle telemetry record.
pub const LMU_TELEM_INFO_SIZE: usize = 1_888;

/// Offset within a vehicle telemetry record for current engine RPM (`f64`).
pub const LMU_ENGINE_RPM_OFFSET: usize = 356;

/// Offset within a vehicle telemetry record for engine max RPM (`f64`).
pub const LMU_ENGINE_MAX_RPM_OFFSET: usize = 532;

/// Upper bound for valid LMU vehicle indices in shared telemetry.
pub const LMU_MAX_VEHICLES: u8 = 104;

/// Candidate offsets within a vehicle record to inspect for brake-pedal telemetry.
///
/// The spiral offset list is intentionally broad: typical pedal/channel values are stored as
/// small normalized floats near the start of a vehicle telemetry record, and this lets the
/// bridge surface the most plausible brake value before a precise field offset is fixed.
pub const LMU_BRAKE_CANDIDATE_OFFSETS: [usize; 22] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64, 72, 80, 88, 96, 112, 128, 168, 224, 304,
];
