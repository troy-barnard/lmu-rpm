# Moza RPM Bridge - Architecture Guide

Welcome! This document explains how the Moza RPM bridge project is organized and how all the pieces fit together. It's written for developers new to Rust or this codebase.

## Table of Contents

1. [Project Overview](#project-overview)
2. [Module Structure](#module-structure)
3. [Data Flow](#data-flow)
4. [Concurrency Model](#concurrency-model)
5. [Key Concepts](#key-concepts)
6. [Common Tasks](#common-tasks)

## Project Overview

The bridge connects three systems:

```
Le Mans Ultimate (Game)
        ↓ (shared memory)
   Moza RPM Bridge (this project)
        ↓ (serial port)
   Moza R9 Wheel Base
```

**What the bridge does:**
1. Reads engine RPM and brake pedal data from LMU's shared memory (running under Wine/Proton)
2. Calculates how many of the 15 RPM indicator LEDs should light up
3. Sends serial commands to the Moza wheel to update the LED display
4. Allows real-time configuration changes via a GUI without restarting

## Module Structure

The project is organized into a library (`lib.rs`) and a binary (`main.rs`). Here's what each module does:

### `src/lib.rs` - Library Documentation
The entry point for the library. Contains:
- Overall architecture overview (the diagram you see in `cargo doc`)
- Links to all submodules with brief descriptions
- Example of how to add new telemetry values
- Concurrency model explanation

**When to use it:** Read this first! It has the big picture.

### `src/main.rs` - Binary Entry Point
The executable that runs the bridge. Contains:
- Initialization code: set up logging, load config, open serial port
- Spawn telemetry reading tasks (brake and RPM)
- Main event loop: listen for telemetry updates and send to wheel
- Status GUI launcher

**Rust pattern:** By separating library code from the binary, you can:
- Write tests that use just the library
- Reuse modules in other binaries (e.g., `lmu_dump`, `brake_monitor`)
- Keep concerns separate

### `src/lmu.rs` - LMU Shared Memory Reader
Reads telemetry data from Le Mans Ultimate's memory-mapped file using Windows APIs.

**Key types:**
- `LmuSharedMemory`: Wraps the shared memory handle and provides methods to read data
  - `connect()`: Opens the LMU_Data memory mapping
  - `read_player_rpm()`: Extracts current engine RPM
  - `read_player_max_rpm()`: Gets maximum engine RPM
  - `read_player_brake_percent()`: Reads brake pedal percentage

**Key functions:**
- `normalize_simetry_brake_percent(value)`: Converts raw brake value (f64) to a valid percentage (0-100)

**Why it exists separately:**
- Encapsulates all Windows API complexity (handle management, memory mapping)
- Makes it easy to mock for testing
- Keeps raw memory access in one place

### `src/lmu_config.rs` - Shared Memory Layout Constants
Defines all the magic numbers: where to find RPM and brake data in LMU's memory block.

**Key constants:**
- `LMU_ENGINE_RPM_OFFSET = 0x164`: Where RPM is stored (relative to vehicle record)
- `LMU_BRAKE_PERCENT_OFFSET = 0x1a4`: Where brake input is stored
- `LMU_TELEM_INFO_SIZE = 1888`: Size of one vehicle's data

**Why separate:**
- Easy to update if LMU's structure changes
- Clear documentation of the memory layout
- Single source of truth for magic offsets

### `src/moza.rs` - Moza Serial Communication
Sends commands to the Moza wheel over the serial port.

**Key types:**
- `MozaSerial`: Wraps the open serial port and handles communication
  - `send_rpm_telemetry_command()`: Updates RPM LEDs based on current RPM %
  - `send_color_payload()`: Configures LED colors
  - `initialize()`: Sets up the wheel on startup

**Key constants:**
- `RPM_MASK_TEMPLATE`: The binary packet format for LED updates
- `RPM_COLOR_PAYLOAD_*`: Pre-built color configuration packets
- `LED_FLASH_THRESHOLD = 95`: RPM % at which LEDs start flashing

**Why it exists separately:**
- Encapsulates serial protocol details (checksums, packet structure)
- Could be tested without real hardware by mocking the serial port
- Easy to swap serial implementation if needed

### `src/runtime.rs` - Configuration & Control
Manages runtime configuration that can change while the bridge is running.

**Key types:**
- `RuntimeControl`: A struct holding current settings
  ```rust
  pub struct RuntimeControl {
      pub rpm_scale: f32,           // Multiplier: 0.6 to 1.4
      pub brightness: f32,          // LED brightness: 0.1 to 1.0
      pub fill_start_percent: f32,  // RPM % at first LED: 40 to 80
      pub fill_end_percent: f32,    // RPM % when full: 85 to 95
      // ... more fields
  }
  ```
- `RuntimeControlFile`: Watches the `config/moza-rpm-control.env` file for changes

**Key constants:**
- `MIN_*` and `MAX_*`: Boundary values to prevent invalid configs
- `DEFAULT_*`: Starting values if env vars aren't set

**Key functions:**
- `read_env_f32()`, `read_env_u64_ms()`: Parse environment variables with fallbacks
- `clamp_f32()`: Ensure values stay within bounds

**Why it exists separately:**
- Configuration logic is complex enough to deserve its own module
- Easy to test config validation
- Keeps constants organized and well-documented

### `src/lmu_dump.rs` - Utility Binary
A separate binary that dumps raw memory values for debugging. Not part of the main bridge but uses shared library code.

### `src/bin/brake_monitor.rs` - Utility Binary
Another separate binary that monitors brake input in real-time.

## Data Flow

Here's what happens from the time you start the bridge until an LED lights up:

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Startup (main.rs)                                        │
│   - Load config from environment / config/secrets.json      │
│   - Open serial port to Moza wheel                          │
│   - Create watch channels for RPM, brake, control updates   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 2. Spawn Telemetry Tasks (main.rs)                          │
│   - RPM task (spawn_blocking):                              │
│     - Connect to LMU shared memory (lmu.rs)                 │
│     - Loop: read RPM → normalize → send via channel         │
│   - Brake task (spawn_blocking):                            │
│     - Connect to LMU shared memory (lmu.rs)                 │
│     - Loop: read brake → normalize → send via channel       │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 3. Main Event Loop (main.rs, async task)                    │
│   - Listen on watch channels                                │
│   - When RPM updates:                                       │
│     a. Read current RuntimeControl values                   │
│     b. Apply rpm_scale (multiply RPM % by this factor)      │
│     c. Calculate fill range (fill_start to fill_end)        │
│     d. Determine which LEDs should be on                    │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 4. Build Serial Packet (moza.rs)                            │
│   - Create RPM_MASK_TEMPLATE (base packet)                  │
│   - Set bits for each LED that's on                         │
│   - Calculate checksum                                      │
│   - Populate packet (moza.rs)                               │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│ 5. Send to Wheel (moza.rs)                                  │
│   - Write packet to serial port                             │
│   - Wheel receives and updates LEDs                         │
│   - You see LEDs light up! ✨                               │
└─────────────────────────────────────────────────────────────┘
```

## Concurrency Model

This project uses **Tokio** (an async runtime for Rust) and **watch channels** for safe inter-task communication.

### Why Async/Tokio?

Reading from shared memory and serial ports can block (wait for data). Tokio lets us:
- Run multiple tasks that appear to run in parallel
- Not block the entire program while waiting for I/O
- Use limited system resources efficiently

### Task Layout

```
┌────────────────────────────────────────────────────────────────┐
│                      Tokio Async Runtime                       │
│                                                                │
│  ┌──────────────────┐  ┌──────────────────┐  ┌───────────┐   │
│  │ RPM Read Task    │  │ Brake Read Task  │  │ GUI Task  │   │
│  │ (spawn_blocking) │  │ (spawn_blocking) │  │(spawned)  │   │
│  │                  │  │                  │  │           │   │
│  │ blocking I/O     │  │ blocking I/O     │  │ subprocess│   │
│  └────────┬─────────┘  └────────┬─────────┘  └───────────┘   │
│           │                     │                             │
│           └─────────────────────┼──→ watch channel            │
│                                 │                             │
│  ┌────────────────────────────────────────────────────────┐  │
│  │         Main Wheel Update Task (spawn)                 │  │
│  │                                                        │  │
│  │ Listen on RPM/Brake/Control channels                  │  │
│  │ → Calculate LED pattern                               │  │
│  │ → Build serial packet                                 │  │
│  │ → Send to wheel                                       │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                                │
└────────────────────────────────────────────────────────────────┘
```

### Watch Channels

A watch channel is like a bulletin board:
- **Sender** (`tx`): Posts updates
- **Receiver** (`rx`): Checks the board for latest value
- **Benefit**: Sender doesn't wait for receiver, receiver only gets latest

```rust
// Create a channel that broadcasts u8 values
let (rpm_tx, mut rpm_rx) = watch::channel(0u8);

// Task 1: Send updates
tokio::task::spawn_blocking(move || {
    loop {
        let rpm = read_from_lmu();
        let _ = rpm_tx.send(rpm);  // Send doesn't block
    }
});

// Task 2: Receive updates
tokio::spawn(async move {
    loop {
        // Wait for a change
        if rpm_rx.changed().await.is_ok() {
            let current_rpm = *rpm_rx.borrow();  // Get latest value
            // React to the change
        }
    }
});
```

## Key Concepts

### Memory Safety
Rust's borrow checker ensures that:
- Only one task can mutate a value at a time
- Multiple tasks can read simultaneously
- Watch channels handle this safely using reference counting

### Blocking vs Async
- **Blocking** (spawn_blocking): Can't do async operations, but can call sync APIs
- **Async** (spawn): Can await on futures, but can't call blocking APIs

The RPM/brake readers use `spawn_blocking` because they read from synchronous Windows APIs. The main update loop uses `spawn` because it awaits on channels.

### Error Handling
The project uses `Result` types and `?` operator for error propagation:

```rust
fn open_serial_port() -> Result<Box<dyn SerialPort>> {
    let port = serialport::new("/dev/ttyACM0", 115200)
        .open()?;  // ? unwraps or returns early
    Ok(port)
}
```

## Common Tasks

### Adding a New Telemetry Value

Let's say you want to add throttle percentage:

1. **Define the offset** (`src/lmu_config.rs`):
   ```rust
   pub const LMU_THROTTLE_OFFSET: usize = 0x1ac;
   ```

2. **Add a read method** (`src/lmu.rs`):
   ```rust
   pub fn read_throttle(&self) -> Option<f64> {
       // Read f64 at offset + LMU_THROTTLE_OFFSET
   }
   ```

3. **Create a channel** (`src/main.rs`):
   ```rust
   let (throttle_tx, mut throttle_rx) = watch::channel(None::<f64>);
   ```

4. **Spawn a reader task** (`src/main.rs`):
   ```rust
   tokio::task::spawn_blocking(move || {
       loop {
           if let Some(throttle) = shared_mem.read_throttle() {
               let _ = throttle_tx.send(Some(throttle));
           }
       }
   });
   ```

5. **Listen in main loop** (`src/main.rs`):
   ```rust
   if throttle_rx.changed().await.is_ok() {
       let throttle = *throttle_rx.borrow();
       // Use throttle value
   }
   ```

### Debugging with Logs

Enable debug output:
```bash
MOZA_RPM_DEBUG=1 ./moza-rpm.exe
```

Check the logs:
```bash
tail -f logs/moza-rpm-debug.log
```

The debug log includes:
- Task lifecycle events (started, connected, disconnected)
- Raw telemetry values
- Serial packet checksums
- Configuration changes

## Next Steps

- Read `README.md` for setup and usage
- Run `cargo doc --open` to see full API documentation
- Look at the binary definitions (`lmu_dump.rs`, `brake_monitor.rs`) for examples of using the library
- Check `DEVELOPING.md` for development workflow

Happy coding! 🚀
