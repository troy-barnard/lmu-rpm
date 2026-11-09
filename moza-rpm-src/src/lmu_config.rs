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

/// Observed live LMU brake-pedal telemetry offset within the per-vehicle record.
///
/// Calculated from official LMU SharedMemoryInterface.hpp using RPM as an anchor:
/// - mEngineRPM (working offset) is at 0x164 (356 bytes) in per-vehicle record
/// - mEngineRPM is the 2nd field in "Vehicle status" section
/// - Vehicle status starts at 0x15c (348): mGear (0x15c), mEngineRPM (0x164)
/// - Followed by: mEngineWaterTemp (0x16c), mEngineOilTemp (0x174), mClutchRPM (0x17c)
/// - Driver input section starts at 0x184 (388):
///   - mUnfilteredThrottle: 0x184
///   - mUnfilteredBrake: 0x18c ← THIS
pub const LMU_BRAKE_PERCENT_OFFSET: usize = 0x18c;

/// Nearby alternate offset for mFilteredBrake, used as a fallback.
/// The filtered version comes after unfiltered throttle/brake/steering/clutch.
pub const LMU_BRAKE_PERCENT_FALLBACK_OFFSET: usize = 0x1a4;

/// Upper bound for valid LMU vehicle indices in shared telemetry.
pub const LMU_MAX_VEHICLES: u8 = 104;

/// Candidate offsets within a vehicle record to inspect for brake-pedal telemetry.
///
/// The official LMU SharedMemoryInterface.hpp shows driver input fields (throttle/brake/steering/clutch)
/// starting around 0x184. These are the key offsets to check, plus a few nearby addresses.
pub const LMU_BRAKE_CANDIDATE_OFFSETS: [usize; 37] = [
    // Driver input section (official offsets based on struct)
    0x184, 0x18c, 0x194, 0x19c,  // unfiltered throttle, brake, steering, clutch
    0x1a4, 0x1ac, 0x1b4, 0x1bc,  // filtered throttle, brake, steering, clutch
    // Generic backups
    0x0, 0x4, 0x8, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x48, 0x50, 0x58, 0x60,
    0x68, 0x70, 0x78, 0x80, 0x90, 0xa0, 0xb0, 0xc0, 0xd0, 0xe0, 0xf0, 0x100, 0x120,
    0x140, 0x160,
];
