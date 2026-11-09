/// Real-time brake telemetry monitor for LMU shared memory validation.
/// 
/// Scans all candidate brake offsets in real-time and shows which ones change when you
/// press/release the brake pedal. This helps identify the correct offset without restarting LMU.
///
/// Run with: cargo run --release --bin brake_monitor

use std::collections::HashMap;
use std::io::Write;
use std::thread;
use std::time::Duration;

use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{FILE_MAP_READ, MapViewOfFile, UnmapViewOfFile};
use winapi::um::winbase::OpenFileMappingA;

const LMU_TELEMETRY_OFFSET: usize = 128_464;
const LMU_TELEM_INFO_OFFSET: usize = 4;
const LMU_MAPPING_NAMES: [&[u8]; 2] = [b"LMU_Data\0", b"Global\\LMU_Data\0"];

// Candidate offsets to scan - focusing on driver input fields in TelemInfoV01
// Based on official header: mUnfilteredThrottle should be around 0x190, mUnfilteredBrake around 0x198
const CANDIDATE_OFFSETS: [usize; 41] = [
    // Low offsets
    0x0, 0x4, 0x8, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x48, 0x50, 0x58, 0x60,
    0x68, 0x70, 0x78, 0x80, 0x90, 0xa0, 0xb0, 0xc0, 0xd0, 0xe0, 0xf0, 0x100, 0x120,
    0x140, 0x160,
    // Driver input area (0x190-0x1c0)
    0x188, 0x190, 0x198, 0x1a0, 0x1a8, 0x1b0, 0x1b8, 0x1c0,
    // Old guessed offsets (for reference)
    0x264, 0x26c, 0x274, 0x27c,
];

fn read_f64(view: *const u8, offset: usize) -> f64 {
    unsafe {
        let ptr = view.add(offset) as *const f64;
        *ptr
    }
}

fn main() {
    println!("=== LMU Brake Monitor (Multi-Offset Scanner) ===");
    println!("Scanning {} candidate offsets for brake signal...", CANDIDATE_OFFSETS.len());
    println!("Press/release brake repeatedly to see which offsets change.\n");

    let mut mapping_handle = std::ptr::null_mut();
    let mut view = std::ptr::null_mut::<u8>();

    // Try to open the LMU shared memory
    for mapping_name in &LMU_MAPPING_NAMES {
        unsafe {
            mapping_handle = OpenFileMappingA(FILE_MAP_READ, 0, mapping_name.as_ptr() as *const i8);
            if !mapping_handle.is_null() {
                println!("✓ Opened mapping: {}", 
                    String::from_utf8_lossy(mapping_name).trim_end_matches('\0'));
                break;
            }
        }
    }

    if mapping_handle.is_null() {
        eprintln!("✗ ERROR: Could not open LMU shared memory mapping");
        eprintln!("  Make sure LMU is running and this program runs with Proton/Wine");
        return;
    }

    // Map the view
    unsafe {
        view = MapViewOfFile(mapping_handle, FILE_MAP_READ, 0, 0, 0) as *const u8 as *mut u8;
        if view.is_null() {
            eprintln!("✗ ERROR: Could not map view of file");
            CloseHandle(mapping_handle);
            return;
        }
    }

    println!("✓ Memory mapped successfully. Starting scan...\n");

    let mut last_values: HashMap<usize, f64> = HashMap::new();
    let mut activity_count: HashMap<usize, u32> = HashMap::new();
    
    loop {
        // Read from the first vehicle's telemetry record (player is usually index 0)
        let telem_addr = unsafe { view.add(LMU_TELEMETRY_OFFSET + LMU_TELEM_INFO_OFFSET) };
        
        // Read all offsets
        for &offset in &CANDIDATE_OFFSETS {
            let value = read_f64(telem_addr, offset);
            
            // Check if value changed significantly
            if let Some(last_val) = last_values.get(&offset) {
                if (value - last_val).abs() > 0.001 {
                    // Value changed!
                    let count = activity_count.entry(offset).or_insert(0);
                    *count += 1;
                    
                    // Print with activity indicator
                    if *count % 5 == 1 {
                        println!("ACTIVE: Offset 0x{:03x} → {:.6} (activity level: {})", 
                            offset, value, count);
                        std::io::stdout().flush().ok();
                    }
                }
            }
            
            last_values.insert(offset, value);
        }

        thread::sleep(Duration::from_millis(50));
    }

    // Cleanup (unreachable in infinite loop, but good practice)
    #[allow(unreachable_code)]
    {
        unsafe {
            UnmapViewOfFile(view as *const _);
            CloseHandle(mapping_handle);
        }
    }
}
