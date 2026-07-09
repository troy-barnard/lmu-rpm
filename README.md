# LMU + Moza RPM lights on Linux/Proton

This project is a practical way to get RPM lights working on a Moza R9 + GS V2 wheel while running Le Mans Ultimate through Proton on Linux.

The approach uses a vendored copy of [moza-rpm](https://github.com/wildernessmith/moza-rpm) under `moza-rpm-src/`, which talks to the wheel over the serial connection and drives the wheel LEDs from telemetry.

## What you need

- Linux desktop with Proton and Steam
- Moza R9 base
- GS V2 wheel
- `protontricks`
- `cargo` and `rustup`
- `mingw-w64`
- `zenity` (optional, for live status window)

## Configuration

Before running any scripts, you need to customize `secrets.json` for your system:

```bash
cp example.secrets.json secrets.json
# Edit secrets.json with your Steam library paths, Proton installation paths, and app ID
```

Key settings in `secrets.json`:
- `steam.app_id`: Le Mans Ultimate Steam app ID (default: 2399420)
- `steam.library_paths`: Array of paths where your Steam libraries are installed
- `proton.install_paths`: Array of Proton runtime installation paths to search
- `wheel.serial_device`: Wheel serial device (default: /dev/ttyACM0)
- `bridge.start_delay_seconds`: Delay before bridge starts (default: 10)

All scripts will automatically load `secrets.json` and use these values.

## 1. Install prerequisites on CachyOS

```bash
sudo pacman -Syu --needed base-devel rustup mingw-w64 protontricks wine-staging jq zenity
rustup default stable
rustup target add x86_64-pc-windows-gnu
```

Note: `jq` is required for parsing the `secrets.json` configuration file.

## 2. Build the bridge (local source in this folder)

```bash
cd moza-rpm-src
cargo build --release --target x86_64-pc-windows-gnu
```

The resulting executable will be copied to the project root as `moza-rpm.exe` when you run the launch or setup scripts.

## 3. Make the wheel device visible to Proton

Your wheel is already detected by Linux as a serial device, so the main requirement is to expose it to the Proton wine prefix as `COM1`.

The helper script in this folder can do that for you:

```bash
chmod +x scripts/setup-moza-rpm.sh
./scripts/setup-moza-rpm.sh
```

The script will:

- find the Moza serial device (e.g., `/dev/ttyACM0`)
- create a stable `/dev/moza-r9` symlink
- configure the Proton prefix so `COM1` points to that device

## 4. Run the bridge while LMU is running

Start Le Mans Ultimate normally in Steam, then in another terminal run:

```bash
./scripts/run-moza-rpm.sh
```

For telemetry debugging output:

```bash
MOZA_RPM_DEBUG=1 ./scripts/run-moza-rpm.sh
```

That script will:

- build the Windows bridge executable from `moza-rpm-src/` if needed
- copy it into the Proton prefix
- launch it through the same Proton runtime used for LMU

The setup and run scripts also pin `wine` and `wineserver` to that selected Proton runtime to avoid Wine version mismatch errors.

## 5. Auto-start the bridge when LMU launches

If you want Steam to start LMU and the RPM bridge together, use this wrapper script:

```bash
./scripts/launch-lmu-with-rpm.sh %command%
```

Set that line in Steam for LMU:

- Library -> Le Mans Ultimate -> Properties -> Launch Options

Steam Launch Options examples:

```bash
# Normal auto-start (recommended, adjust path to your lmu-rpm folder)
/path/to/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

# Auto-start with bridge debug logging
MOZA_RPM_DEBUG=1 /path/to/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

# Increase bridge start delay to 15 seconds (overrides secrets.json)
MOZA_BRIDGE_START_DELAY=15 /path/to/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

# Force bridge-defined RPM colors (normally leave this unset)
MOZA_FORCE_RPM_COLORS=1 /path/to/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

# Force bridge-defined button colors (normally leave this unset)
MOZA_FORCE_BUTTON_COLORS=1 /path/to/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%
```

Optional environment variables:

- `MOZA_BRIDGE_START_DELAY=10` (seconds to wait before starting the bridge, overrides secrets.json)
- `MOZA_RPM_DEBUG=1` (enable bridge telemetry debug logging)
- `MOZA_RPM_SAMPLE_LOG_INTERVAL_MS=2000` (default; periodic sample log interval, with automatic immediate logs on large RPM changes)
- `MOZA_STATUS_GUI=1` (default; shows live draggable status window if `zenity` is installed)
- `MOZA_STATUS_GUI=0` (disable status window)
- `MOZA_AUTO_RESET_ON_LAUNCH=1` (default; kill stale LMU/bridge/wineserver processes before launch)
- `MOZA_AUTO_RESET_ON_LAUNCH=0` (skip runtime reset preflight)
- `MOZA_AUTO_SETUP=1` (default; runs COM1 mapping preflight at launcher start)
- `MOZA_AUTO_SETUP=0` (skip COM1 preflight)
- `MOZA_SETUP_RETRY_COUNT=8` (default; COM1 setup retry attempts at launch)
- `MOZA_SETUP_RETRY_DELAY=2` (default; seconds between COM1 setup retries)
- `MOZA_BRIDGE_CONNECT_TIMEOUT=45` (default; seconds to wait for telemetry connection before marking failure)
- `MOZA_BRIDGE_RESTART_ON_CONNECT_FAIL=1` (default; auto-restart bridge once if telemetry connection fails)
- `MOZA_REQUIRE_RPM_SAMPLE=1` (default; require real RPM sample before declaring bridge healthy)
- `MOZA_REQUIRE_RPM_SAMPLE=0` (fallback; treat shared-memory open as healthy)
- `MOZA_FORCE_RPM_COLORS=1` (default from launcher; ensures RPM LED colors are initialized each launch)
- `MOZA_FORCE_RPM_COLORS=0` (disable forced RPM color initialization)
- `MOZA_FORCE_BUTTON_COLORS=0` (default from launcher; preserve wheel profile button backlights)
- `MOZA_FORCE_BUTTON_COLORS=1` (optional; force bridge-defined button colors)
- `MOZA_MENU_FORCE_BUTTON_COLORS=1` (optional troubleshooting mode; force bridge button colors in menus/idle, then auto-disable after first telemetry update)
- `MOZA_IDLE_LED_WRITES=0` (default; do not send idle/menu LED updates before telemetry starts, preserving wheel profile in menus)
- `MOZA_IDLE_LED_WRITES=1` (optional compatibility mode; send idle RPM LED updates while waiting for telemetry)
- `MOZA_ENABLE_BUTTON_COLOR_OVERRIDES=0` (default; master safety switch, disables all bridge button color writes)
- `MOZA_ENABLE_BUTTON_COLOR_OVERRIDES=1` (required to allow `MOZA_FORCE_BUTTON_COLORS` / `MOZA_MENU_FORCE_BUTTON_COLORS`)

The wrapper logs bridge output to `moza-rpm-launch.log` in the project directory.
The wrapper also writes live status updates to `moza-rpm-status.log`.

Status window severity colors:

- Green: success events (setup connected, LMU process detected)
- Yellow: warnings/retries (temporary process misses, retry attempts)
- Red: failures (preflight failures, unexpected LMU disappearance)
- Live telemetry: periodic RPM samples (rpm/max_rpm/percent) appear as `INFO` lines while connected

## 6. Proton upgrade safety check

After changing Proton versions, run:

```bash
./scripts/check-proton-setup.sh
```

This verifies:

- prefix path exists
- selected Proton runtime exists
- matching wine/wineserver binaries are found
- wheel serial device is visible
- COM1 mapping exists in Wine registry

## 7. Button color behavior

By default, the bridge preserves your wheel profile colors.

- Default: wheel profile colors are kept for both RPM and non-RPM button LEDs
- To force bridge RPM colors, set `MOZA_FORCE_RPM_COLORS=1`
- To force bridge button colors, set `MOZA_FORCE_BUTTON_COLORS=1`
- To keep buttons lit in menus, set `MOZA_MENU_FORCE_BUTTON_COLORS=1` (opt-in; bridge refreshes colors while idle, then stops overriding once on-track telemetry begins)
- Recommended normal use: leave `MOZA_MENU_FORCE_BUTTON_COLORS` unset/0 so button colors are never overridden in menus.

## 8. Development and maintenance guide

For full project architecture, Proton migration strategy, troubleshooting playbook, and Git workflow:

See [DEVELOPING.md](DEVELOPING.md).

## 9. Expected result

When the bridge connects successfully, the wheel LEDs should light up briefly and then follow engine RPM as you drive in LMU.

If nothing happens, check:

- the wheel is connected before launching the bridge
- the device path is correct
- `COM1` was mapped to the correct device in the Proton prefix
- LMU is running and the bridge is still attached to the same Proton prefix

## 10. Quick recovery if Steam/LMU gets stuck

If LMU or the bridge gets stuck in a running state, run:

```bash
./scripts/reset-runtime.sh
```

This script safely stops stale LMU/bridge/proton processes and kills wineserver for the LMU prefix.

The launcher also runs this reset automatically by default each time you start LMU via launch options.
