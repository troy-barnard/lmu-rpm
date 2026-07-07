# LMU RPM Bridge Development Guide

This guide explains how the project works, how to maintain it, and how to keep it resilient when Proton versions change.

## 1. Repository layout and responsibilities

Current folders involved:

- /home/troy/Documents/SimRacing/lmu-rpm
  - Runtime wrapper project for Linux and Steam integration.
  - Owns launch scripts, setup scripts, logs, and deployed moza-rpm.exe.
- /home/troy/Documents/SimRacing/moza-rpm
  - Rust bridge implementation.
  - Owns protocol logic, telemetry ingestion, and binary builds.

Practical rule:

- Edit Rust logic in moza-rpm.
- Edit startup/runtime behavior in lmu-rpm.

## 2. Runtime architecture

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

- /home/troy/Documents/SimRacing/moza-rpm/src/main.rs
  - LMU shared-memory reader offsets and map names.
  - LED threshold mapping and initialization behavior.
- /home/troy/Documents/SimRacing/lmu-rpm/scripts/setup-moza-rpm.sh
  - COM1 mapping in the LMU prefix.
- /home/troy/Documents/SimRacing/lmu-rpm/scripts/run-moza-rpm.sh
  - Launches bridge in matching Proton runtime.
- /home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh
  - Steam launch wrapper that starts LMU then starts bridge.

## 3. Build and deploy loop

Build and deploy manually:

1. cd /home/troy/Documents/SimRacing/moza-rpm
2. cargo build --release --target x86_64-pc-windows-gnu
3. cp target/x86_64-pc-windows-gnu/release/moza-rpm.exe /home/troy/Documents/SimRacing/lmu-rpm/moza-rpm.exe

Run bridge manually:

1. cd /home/troy/Documents/SimRacing/lmu-rpm
2. STEAM_APP_ID=2399420 ./scripts/run-moza-rpm.sh

Run with debug logging:

1. cd /home/troy/Documents/SimRacing/lmu-rpm
2. MOZA_RPM_DEBUG=1 STEAM_APP_ID=2399420 ./scripts/run-moza-rpm.sh
3. Check /home/troy/Documents/SimRacing/lmu-rpm/moza-rpm-debug.log

## 4. Color behavior controls

Defaults are profile-preserving:

- Bridge does not push RPM color init unless requested.
- Bridge does not push button color init unless requested.

Optional overrides:

- MOZA_FORCE_RPM_COLORS=1
  - Bridge pushes RPM color payloads during init.
- MOZA_FORCE_BUTTON_COLORS=1
  - Bridge pushes button color payloads during init.

If you want wheel profile colors untouched, do not set either variable.

## 5. Proton version change strategy

Question: will changing Proton versions break this project?

Short answer:

- It can break if LMU and bridge run with different Proton runtime/prefix assumptions.
- The scripts are built to reduce this risk by selecting a single runtime and matching wine/wineserver tools.

Most common break points after a Proton/tool update:

1. New runtime path (old PROTON_BIN no longer valid).
2. LMU switched runtime but wrapper still points to previous one.
3. Prefix moved, rebuilt, or regenerated.
4. COM1 mapping missing in new prefix.

Hardening checklist after any Proton change:

1. Confirm LMU launch runtime path exists.
2. Re-run setup mapping:
   - STEAM_APP_ID=2399420 ./scripts/setup-moza-rpm.sh
3. Verify bridge still launches:
   - STEAM_APP_ID=2399420 ./scripts/run-moza-rpm.sh
4. If no telemetry, run with debug and verify map detection in log.

Recommended policy:

- Keep one known-good custom Proton in compatibilitytools.d.
- Upgrade by adding new runtime alongside old one, not replacing immediately.
- Test bridge with new runtime before removing old runtime.

## 6. Steam auto-start configuration

Launch option for LMU:

- /home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

Optional environment controls in launch options:

- MOZA_BRIDGE_START_DELAY=10
- STEAM_APP_ID=2399420
- MOZA_FORCE_RPM_COLORS=1 (optional)
- MOZA_FORCE_BUTTON_COLORS=1 (optional)

Example with delay and default profile-preserving colors:

- MOZA_BRIDGE_START_DELAY=10 /home/troy/Documents/SimRacing/lmu-rpm/scripts/launch-lmu-with-rpm.sh %command%

## 7. Troubleshooting playbook

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

## 8. Git and release workflow

Current state:

- /home/troy/Documents/SimRacing/moza-rpm is already a git repo.
- /home/troy/Documents/SimRacing/lmu-rpm is currently not a git repo.

Recommended setup:

Option A (simple): keep two repos.

- Commit Rust code in moza-rpm.
- Initialize lmu-rpm as its own repo for scripts/docs.

Initialize lmu-rpm repo:

1. cd /home/troy/Documents/SimRacing/lmu-rpm
2. git init
3. printf "moza-rpm-debug.log\nmoza-rpm-launch.log\n" > .gitignore
4. git add .
5. git commit -m "Add LMU launcher scripts and docs"

Commit moza-rpm changes:

1. cd /home/troy/Documents/SimRacing/moza-rpm
2. git add Cargo.toml Cargo.lock src/main.rs
3. git commit -m "Add LMU native shared-memory telemetry path and configurable LED init"

Then push each repo to GitHub remotes you create.

## 9. Safe future modifications

When changing telemetry logic:

1. Keep debug logging path intact.
2. Test map connection first, then RPM scaling, then LED protocol.
3. Avoid mixing multiple protocol and telemetry changes in one commit.

When changing LED protocol:

1. Keep threshold-only changes separate from payload-format changes.
2. Test at idle, mid RPM, and near redline.
3. Keep profile-preserving defaults unless intentionally overriding colors.
