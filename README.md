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

## 1. Install prerequisites on CachyOS

```bash
sudo pacman -Syu --needed base-devel rustup mingw-w64 protontricks wine-staging
rustup default stable
rustup target add x86_64-pc-windows-gnu
```

If `protontricks` is already present, you can skip that part.

## 2. Build the bridge (local source in this folder)

```bash
cd /home/troy/Documents/SimRacing/lmu-rpm/moza-rpm-src
cargo build --release --target x86_64-pc-windows-gnu
```

The resulting executable will be at:

```bash
/home/troy/Documents/SimRacing/lmu-rpm/moza-rpm-src/target/x86_64-pc-windows-gnu/release/moza-rpm.exe
```

To deploy it for runtime:

```bash
cp /home/troy/Documents/SimRacing/lmu-rpm/moza-rpm-src/target/x86_64-pc-windows-gnu/release/moza-rpm.exe /home/troy/Documents/SimRacing/lmu-rpm/moza-rpm.exe
```

## 3. Make the wheel device visible to Proton

Your wheel is already detected by Linux as a serial device, so the main requirement is to expose it to the Proton wine prefix as `COM1`.

The helper script in this folder can do that for you once you provide the Steam app ID for Le Mans Ultimate:

```bash
chmod +x scripts/setup-moza-rpm.sh
STEAM_APP_ID=<your_lmu_steam_app_id> ./scripts/setup-moza-rpm.sh
```

The script will:

- find the Moza serial device (for example `/dev/ttyACM0`)
- create a stable `/dev/moza-r9` symlink
- configure the Proton prefix so `COM1` points to that device

## 4. Run the bridge while LMU is running

Start Le Mans Ultimate normally in Steam, then in another terminal run:

```bash
STEAM_APP_ID=<your_lmu_steam_app_id> ./scripts/run-moza-rpm.sh
```

For telemetry debugging output:

```bash
MOZA_RPM_DEBUG=1 STEAM_APP_ID=<your_lmu_steam_app_id> ./scripts/run-moza-rpm.sh
```

That script will:

- build the Windows bridge executable from `moza-rpm-src/` if needed
- copy it into the Proton prefix
- launch it through the same Proton runtime used for LMU

The setup and run scripts also pin `wine` and `wineserver` to that selected Proton runtime to avoid Wine version mismatch errors.

## 5. Auto-start the bridge when LMU launches

If you want Steam to start LMU and the RPM bridge together, use this wrapper script:

```bash
/home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%
```

Set that line in Steam for LMU:

- Library -> Le Mans Ultimate -> Properties -> Launch Options

Steam Launch Options examples:

```bash
# Normal auto-start (recommended)
/home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

# Auto-start with bridge debug logging
MOZA_RPM_DEBUG=1 /home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

# Increase bridge start delay to 15 seconds
MOZA_BRIDGE_START_DELAY=15 /home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

# Force bridge-defined RPM colors (normally leave this unset)
MOZA_FORCE_RPM_COLORS=1 /home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

# Force bridge-defined button colors (normally leave this unset)
MOZA_FORCE_BUTTON_COLORS=1 /home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%
```

Optional environment variables:

- `MOZA_BRIDGE_START_DELAY=10` (seconds to wait before starting the bridge)
- `STEAM_APP_ID=2399420` (defaults to LMU app ID)
- `MOZA_FORCE_RPM_COLORS=1` (optional; by default bridge does not override RPM colors)
- `MOZA_FORCE_BUTTON_COLORS=1` (optional; by default bridge does not override button colors)

The wrapper logs bridge output to:

```bash
/home/troy/Documents/SimRacing/lmu-rpm/moza-rpm-launch.log
```

## 6. Proton upgrade safety check

After changing Proton versions, run:

```bash
STEAM_APP_ID=2399420 ./scripts/check-proton-setup.sh
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

## 8. Development and maintenance guide

For full project architecture, Proton migration strategy, troubleshooting playbook, and Git workflow:

See `/home/troy/Documents/SimRacing/lmu-rpm/DEVELOPING.md`.

## 9. Expected result

When the bridge connects successfully, the wheel LEDs should light up briefly and then follow engine RPM as you drive in LMU.

If nothing happens, check:

- the wheel is connected before launching the bridge
- the device path is correct
- `COM1` was mapped to the correct device in the Proton prefix
- LMU is running and the bridge is still attached to the same Proton prefix
