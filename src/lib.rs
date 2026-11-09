//! # Moza RPM Bridge Library
//!
//! This library provides the core functionality for bridging telemetry data from Le Mans Ultimate (LMU)
//! running on Linux/Proton to a Moza R9 steering wheel base via the RPM display lighting system.
//!
//! ## Architecture Overview
//!
//! The bridge works by connecting three main components:
//!
//! ```text
//! ┌─────────────────────┐
//! │  LMU Game (Proton)  │
//! │ Shared Memory Block │
//! └──────────┬──────────┘
//!            │
//!            │ (read telemetry: RPM, brake)
//!            ▼
//! ┌─────────────────────────────────────────────┐
//! │       Moza RPM Bridge (this crate)          │
//! │                                             │
//! │  ┌──────────────┐      ┌──────────────────┐│
//! │  │ LMU Reader   │      │  Moza Serializer ││
//! │  │ (Windows API)│      │  (Serial I/O)    ││
//! │  └──────────────┘      └──────────────────┘│
//! │         │                      ▲           │
//! │         │ RPM, Brake           │           │
//! │         └──────────┬───────────┘           │
//! │                    │                       │
//! │              Runtime Control               │
//! │         (Configuration & State)            │
//! └────────────────────────────────────────────┘
//!                      │
//!                      │ (serial protocol)
//!                      ▼
//!            ┌──────────────────┐
//!            │  Moza R9 Base    │
//!            │  RPM LEDs (15x)  │
//! ```
//!
//! ## Key Concepts
//!
//! - **LMU Shared Memory**: Le Mans Ultimate exposes game state (RPM, brake, vehicle info) through a Windows
//!   memory-mapped file accessible from Wine/Proton. This module reads that data.
//!
//! - **Telemetry**: Engine RPM and brake pedal percentage are extracted from the shared memory block
//!   at known offsets (configured in [`lmu_config`]).
//!
//! - **Serial Protocol**: The Moza wheel communicates via a custom serial protocol. This module
//!   builds and sends packets to control LED colors, brightness, and which LEDs are lit based on RPM.
//!
//! - **Runtime Control**: Configuration values (RPM scale, brightness, fill percentages) can be
//!   updated at runtime from an environment file (moza-rpm-control.env) without restarting the bridge.
//!
//! ## Concurrency Model
//!
//! The bridge uses Tokio async runtime with the following task layout:
//!
//! 1. **Brake Telemetry Task** (`tokio::task::spawn_blocking`): Reads LMU shared memory in a loop,
//!    extracts brake percentage, sends updates through a watch channel.
//!
//! 2. **RPM Telemetry Task** (`tokio::task::spawn_blocking`): Similar to brake, reads RPM data from
//!    shared memory and sends through watch channel.
//!
//! 3. **Wheel Update Task** (`tokio::spawn`): Async task that listens on watch channels, receives
//!    RPM/brake updates, applies runtime controls, builds serial packets, and sends to the wheel.
//!
//! Watch channels allow tasks to communicate without blocking: reader tasks can update values without
//! waiting for consumers, and consumers only get the latest value when they check.
//!
//! ## Module Organization
//!
//! - [`lmu`]: Reading engine data from LMU shared memory (Windows API integration)
//! - [`lmu_config`]: Shared memory offsets and mapping names (configuration constants)
//! - [`moza`]: Serial communication with the Moza wheel (packet building and serial I/O)
//! - [`runtime`]: Configuration state, environment variable parsing, and control constants
//!
//! ## Example: Adding a New Telemetry Value
//!
//! To add a new value (e.g., throttle percentage):
//!
//! 1. Add the offset to [`lmu_config`] (e.g., `pub const LMU_THROTTLE_OFFSET: usize = 0x1ac;`)
//! 2. Add a read method to [`lmu::LmuSharedMemory`]
//! 3. Create a watch channel in main for throttle (like `(throttle_tx, throttle_rx)`)
//! 4. Spawn a task to read and broadcast throttle updates
//! 5. Listen to `throttle_rx` in the wheel update task and apply logic
//!
//! ## Debugging
//!
//! Enable debug logging by setting `MOZA_RPM_DEBUG=1` when running. This creates `logs/moza-rpm-debug.log`
//! with detailed lifecycle and value traces.

pub mod lmu;
pub mod lmu_config;
pub mod moza;
pub mod runtime;
