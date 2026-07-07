# LMU RPM Bridge Development Guide

This guide explains how the project works, how to maintain it, and how to keep it resilient when Proton versions change.

## 1. Repository layout and responsibilities

This project folder structure:

- `.` (project root)
  - Self-contained runtime wrapper and bridge source.
  - Owns launch scripts, setup scripts, docs, deployed moza-rpm.exe, and bridge source at `moza-rpm-src/`.

Optional external upstream mirror (not required for runtime):

- `../moza-rpm` (sibling directory)
  - Independent repo if you also maintain upstream-facing bridge changes there.

Practical rule:

- For this project, edit Rust bridge logic in `moza-rpm-src/src/main.rs`.
- Edit startup/runtime behavior in `scripts/` directory.

## 2. Configuration management (secrets.json)

The project uses `secrets.json` to store system-specific paths and settings:

**Initial setup:**

```bash
cp example.secrets.json secrets.json
# Edit secrets.json with your Steam paths, Proton installations, etc.
```

**Configuration file structure:**

- `steam.app_id`: Le Mans Ultimate Steam app ID (default 2399420)
- `steam.library_paths`: Array of Steam library install paths (e.g., `/ssd2/SteamLibrary`, `~/.local/share/Steam`)
- `proton.install_paths`: Array of Proton runtime paths to search
- `wheel.serial_device`: Serial device for wheel (default `/dev/ttyACM0`)
- `bridge.start_delay_seconds`: Delay before bridge starts (default 10)

**Why this approach:**

- Removes hardcoded paths that are specific to your system
- Allows portable distribution of the project
- Makes it easy to run on different machines or after system changes
- `secrets.json` is in `.gitignore` (never committed)
- `example.secrets.json` is committed as a template for others

**How scripts use it:**

All scripts source `scripts/read-secrets.sh` which:
1. Reads `secrets.json` using `jq` (JSON parser)
2. Exports configuration as shell variables
3. Falls back to arrays for multi-valued settings

**Dependencies:**

- `jq` is required: `sudo pacman -S jq` (or equivalent)
- All scripts validate `secrets.json` exists before running

## 3. Runtime architecture

Data path:

1. LMU runs in Proton.
2. LMU exports telemetry through native shared memory map names:
   - LMU_Data
   - Global\\LMU_Data (fallback)
3. moza-rpm.exe runs in the same Proton prefix and reads player RPM/max RPM.
4. moza-rpm.exe writes serial protocol packets to COM1.
5. Wine COM1 is mapped to Linux serial device /dev/moza-r9 (or /dev/ttyACM*).
6. Moza base/wheel receives LED mask updates.

Key files:

- `moza-rpm-src/src/main.rs`
  - LMU shared-memory reader offsets and map names.
  - LED threshold mapping and initialization behavior.
- `scripts/setup-moza-rpm.sh`
  - COM1 mapping in the LMU prefix.
- `scripts/run-moza-rpm.sh`
  - Launches bridge in matching Proton runtime.
- `scripts/launch-lmu-with-rpm.sh`
  - Steam launch wrapper that starts LMU then starts bridge.

## 4. Build and deploy loop

Build and deploy manually:

1. cd moza-rpm-src
2. cargo build --release --target x86_64-pc-windows-gnu
3. cp target/x86_64-pc-windows-gnu/release/moza-rpm.exe ../moza-rpm.exe

Run bridge manually:

1. ./scripts/run-moza-rpm.sh

Run with debug logging:

1. MOZA_RPM_DEBUG=1 ./scripts/run-moza-rpm.sh
2. Check moza-rpm-debug.log for telemetry connection and LED updates

## 5. Color behavior controls

Defaults are profile-preserving:

- Bridge does not push RPM color init unless requested.
- Bridge does not push button color init unless requested.

Optional overrides:

- MOZA_FORCE_RPM_COLORS=1
  - Bridge pushes RPM color payloads during init.
- MOZA_FORCE_BUTTON_COLORS=1
  - Bridge pushes button color payloads during init.

If you want wheel profile colors untouched, do not set either variable.

## 6. Proton version change strategy

Question: will changing Proton versions break this project?

Short answer:

- It can break if LMU and bridge run with different Proton runtime/prefix assumptions.
- The scripts are built to reduce this risk by selecting a single runtime and matching wine/wineserver tools.

Most common break points after a Proton/tool update:

1. New runtime path (old Proton installation path in `secrets.json` no longer valid).
2. LMU switched runtime but wrapper still points to previous one.
3. Prefix moved, rebuilt, or regenerated.
4. COM1 mapping missing in new prefix.

Hardening checklist after any Proton change:

1. Update `secrets.json` with new Proton installation paths if they changed.
2. Confirm LMU launch runtime path exists.
3. Re-run setup mapping:
   - ./scripts/setup-moza-rpm.sh
4. Verify bridge still launches:
   - ./scripts/run-moza-rpm.sh
5. If no telemetry, run with debug and verify map detection in log.

Recommended policy:

- Keep one known-good custom Proton in compatibilitytools.d.
- Upgrade by adding new runtime alongside old one, not replacing immediately.
- Update `proton.install_paths` in `secrets.json` to add the new path.
- Test bridge with new runtime before removing old runtime from `secrets.json`.

## 7. Steam auto-start configuration

Launch option for LMU (replace path with your actual lmu-rpm location):

- /path/to/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

Optional environment controls in launch options:

- MOZA_BRIDGE_START_DELAY=10 (overrides secrets.json setting)
- MOZA_FORCE_RPM_COLORS=1 (optional)
- MOZA_FORCE_BUTTON_COLORS=1 (optional)

Example with delay and default profile-preserving colors:

- MOZA_BRIDGE_START_DELAY=10 /path/to/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

## 8. Troubleshooting playbook

No RPM LEDs at all:

1. Confirm wheel serial device exists on Linux.
2. Re-run setup script for COM1 mapping.
3. Run bridge with debug log enabled.
4. Check for LMU shared memory detection lines.

Bridge starts but no telemetry values:

1. Ensure LMU is actually in session/on-track (not only menus).
2. Check log for repeated LMU native shared memory not found.
3. Validate runtime/prefix pairing and retry.

Buttons or colors overridden unexpectedly:

1. Ensure MOZA_FORCE_RPM_COLORS and MOZA_FORCE_BUTTON_COLORS are unset.
2. Restart LMU and bridge after removing these variables.

## 9. Git and release workflow

Current state:

- This project is a git repo (branch `main`).
- Bridge source required by runtime is vendored at `moza-rpm-src/`.

Recommended setup:

- Commit all runtime and bridge-source changes together in the same repo.

Typical commit flow:

1. git add README.md DEVELOPING.md scripts moza-rpm-src
2. git commit -m "Update bridge and launcher"
3. git push -u origin main

## 10. Safe future modifications

When changing telemetry logic:

1. Keep debug logging path intact.
2. Test map connection first, then RPM scaling, then LED protocol.
3. Avoid mixing multiple protocol and telemetry changes in one commit.

When changing LED protocol:

1. Keep threshold-only changes separate from payload-format changes.
2. Test at idle, mid RPM, and near redline.
3. Keep profile-preserving defaults unless intentionally overriding colors.
