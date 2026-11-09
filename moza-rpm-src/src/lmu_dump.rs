use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use winapi::shared::ntdef::HANDLE;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{FILE_MAP_READ, MapViewOfFile};
use winapi::um::winbase::OpenFileMappingA;

const LMU_MAPPING_NAMES: [&[u8]; 2] = [b"LMU_Data\0", b"Global\\LMU_Data\0"];

fn read_f32(view: *const u8, offset: usize) -> f32 {
    unsafe { *(view.add(offset) as *const f32) }
}

fn parse_u32_hex_or_dec(value: &str) -> Option<usize> {
    let trimmed = value.trim();
    if let Ok(parsed) = usize::from_str_radix(trimmed.trim_start_matches("0x").trim_start_matches("0X"), 16) {
        return Some(parsed);
    }
    if let Ok(parsed) = trimmed.parse::<usize>() {
        return Some(parsed);
    }
    None
}

fn parse_offset_list() -> Vec<usize> {
    if let Ok(value) = std::env::var("LMU_OFFSETS") {
        let mut offsets = Vec::new();
        for part in value.split(',') {
            if let Some(parsed) = parse_u32_hex_or_dec(part) {
                offsets.push(parsed);
            }
        }
        if !offsets.is_empty() {
            return offsets;
        }
    }

    if let Ok(value) = std::env::var("LMU_OFFSET") {
        if let Some(parsed) = parse_u32_hex_or_dec(&value) {
            return vec![parsed];
        }
    }

    if let Ok(contents) = fs::read_to_string(".lmu_offset_watch.txt") {
        let trimmed = contents.trim();
        if let Some(parsed) = parse_u32_hex_or_dec(trimmed) {
            return vec![parsed];
        }
    }

    vec![0x01f960]
}

fn maybe_log_line(line: &str) {
    if let Ok(log_path) = std::env::var("LMU_OFFSET_LOG") {
        let mut file = OpenOptions::new().create(true).append(true).open(log_path).ok();
        if let Some(handle) = file.as_mut() {
            let _ = writeln!(handle, "{line}");
            let _ = handle.flush();
        }
    }
}

fn main() {
    unsafe {
        let mut mapping_handle: HANDLE = std::ptr::null_mut();
        let mut success = false;

        for mapping_name in LMU_MAPPING_NAMES {
            mapping_handle = OpenFileMappingA(FILE_MAP_READ, 0, mapping_name.as_ptr() as *const i8);
            if !mapping_handle.is_null() {
                success = true;
                break;
            }
        }

        if !success {
            eprintln!("LMU shared memory not found. Start LMU and try again.");
            std::process::exit(1);
        }

        let view = MapViewOfFile(mapping_handle, FILE_MAP_READ, 0, 0, 0) as *const u8;
        if view.is_null() {
            let err = io::Error::last_os_error();
            let _ = CloseHandle(mapping_handle);
            eprintln!("MapViewOfFile failed: {err}");
            std::process::exit(2);
        }

        let offsets = parse_offset_list();
        println!("LMU mapping opened; watching offsets: {}", offsets.iter().map(|o| format!("0x{o:06x}")).collect::<Vec<_>>().join(", "));
        println!("Press and hold the brake pedal, then release it. Watch for a clean 0 -> high -> 0 transition.");
        println!("timestamp       offset        value");
        let start = Instant::now();
        let mut last_values: Vec<f32> = vec![0.0; offsets.len()];

        loop {
            let elapsed_ms = start.elapsed().as_millis();
            for (idx, offset) in offsets.iter().enumerate() {
                let value = read_f32(view, *offset);
                if value.is_finite() {
                    let delta = (value - last_values[idx]).abs();
                    if delta > 0.01 || elapsed_ms % 500 == 0 {
                        let line = format!("{elapsed_ms:08}ms  0x{offset:06x}  {:.6}", value);
                        println!("{line}");
                        maybe_log_line(&line);
                        last_values[idx] = value;
                    }
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}
