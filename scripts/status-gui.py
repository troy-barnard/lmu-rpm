#!/usr/bin/env python3
import os
import re
import sys
import tkinter as tk
from tkinter import ttk

LOG_LINE_RE = re.compile(r"^\[(?P<time>[^\]]+)\]\s+\[(?P<level>[A-Z]+)\]\s+(?P<msg>.*)$")
LEGACY_LINE_RE = re.compile(r"^\[(?P<time>[^\]]+)\]\s+(?P<msg>.*)$")

LEVEL_COLORS = {
    "INFO": "#d6deeb",
    "SUCCESS": "#3bd671",
    "WARN": "#f5c542",
    "ERROR": "#ff5f56",
}

BG = "#14181f"
FG = "#d6deeb"
CONTROL_POLL_MS = 300
CONTROL_WRITE_DEBOUNCE_MS = 180

RPM_SCALE_MIN = 0.6
RPM_SCALE_MAX = 1.4
BRIGHTNESS_MIN = 0.1
BRIGHTNESS_MAX = 1.0
DEFAULT_RPM_SCALE = 1.0
DEFAULT_BRIGHTNESS = 1.0
FILL_START_MIN = 40.0
FILL_START_MAX = 80.0
FILL_END_MIN = 85.0
FILL_END_MAX = 95.0
DEFAULT_FILL_START = 65.0
DEFAULT_FILL_END = 93.0
DEFAULT_OVERRIDE_RPM_COLORS = 1
DEFAULT_ENABLE_BUTTON_OVERRIDES = 0
DEFAULT_FORCE_BUTTON_COLORS = 0
DEFAULT_MENU_FORCE_BUTTON_COLORS = 0
DEFAULT_IDLE_LED_WRITES = 0


def parse_line(raw: str):
    m = LOG_LINE_RE.match(raw)
    if m:
        return m.group("time"), m.group("level"), m.group("msg")

    m = LEGACY_LINE_RE.match(raw)
    if m:
        return m.group("time"), "INFO", m.group("msg")

    return "--:--:--", "INFO", raw


def clamp(value: float, min_value: float, max_value: float) -> float:
    return max(min_value, min(max_value, value))


def parse_bool(raw: str, fallback: int) -> int:
    value = raw.strip().lower()
    if value in {"1", "true", "yes", "on"}:
        return 1
    if value in {"0", "false", "no", "off"}:
        return 0
    return fallback


def parse_control_file(control_path: str):
    rpm_scale = DEFAULT_RPM_SCALE
    brightness = DEFAULT_BRIGHTNESS
    fill_start = DEFAULT_FILL_START
    fill_end = DEFAULT_FILL_END
    override_rpm_colors = DEFAULT_OVERRIDE_RPM_COLORS
    enable_button_overrides = DEFAULT_ENABLE_BUTTON_OVERRIDES
    force_button_colors = DEFAULT_FORCE_BUTTON_COLORS
    menu_force_button_colors = DEFAULT_MENU_FORCE_BUTTON_COLORS
    idle_led_writes = DEFAULT_IDLE_LED_WRITES

    try:
        with open(control_path, "r", encoding="utf-8", errors="replace") as f:
            for raw_line in f:
                line = raw_line.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue

                key, value = line.split("=", 1)
                key = key.strip().upper()
                value = value.strip()
                try:
                    parsed = float(value)
                except ValueError:
                    parsed = None

                if key == "RPM_SCALE" and parsed is not None:
                    rpm_scale = clamp(parsed, RPM_SCALE_MIN, RPM_SCALE_MAX)
                elif key == "BRIGHTNESS" and parsed is not None:
                    brightness = clamp(parsed, BRIGHTNESS_MIN, BRIGHTNESS_MAX)
                elif key == "FILL_START_PERCENT" and parsed is not None:
                    fill_start = clamp(parsed, FILL_START_MIN, FILL_START_MAX)
                elif key == "FILL_END_PERCENT" and parsed is not None:
                    fill_end = clamp(parsed, FILL_END_MIN, FILL_END_MAX)
                elif key == "OVERRIDE_RPM_COLORS":
                    override_rpm_colors = parse_bool(value, override_rpm_colors)
                elif key == "ENABLE_BUTTON_OVERRIDES":
                    enable_button_overrides = parse_bool(value, enable_button_overrides)
                elif key == "FORCE_BUTTON_COLORS":
                    force_button_colors = parse_bool(value, force_button_colors)
                elif key == "MENU_FORCE_BUTTON_COLORS":
                    menu_force_button_colors = parse_bool(value, menu_force_button_colors)
                elif key == "IDLE_LED_WRITES":
                    idle_led_writes = parse_bool(value, idle_led_writes)
    except OSError:
        pass

    return {
        "rpm_scale": rpm_scale,
        "brightness": brightness,
        "fill_start": fill_start,
        "fill_end": fill_end,
        "override_rpm_colors": override_rpm_colors,
        "enable_button_overrides": enable_button_overrides,
        "force_button_colors": force_button_colors,
        "menu_force_button_colors": menu_force_button_colors,
        "idle_led_writes": idle_led_writes,
    }


class StatusGui:
    def __init__(self, log_path: str, control_path: str):
        self.log_path = log_path
        self.control_path = control_path
        self.offset = 0
        self.write_after_id = None

        self.root = tk.Tk()
        self.root.title("LMU RPM Bridge Status")
        self.root.geometry("860x500")
        self.root.configure(bg=BG)

        container = ttk.Frame(self.root)
        container.pack(fill="both", expand=True)

        controls = ttk.Frame(container)
        controls.pack(fill="x", padx=8, pady=(8, 2))

        self.rpm_scale_var = tk.DoubleVar(value=DEFAULT_RPM_SCALE)
        self.brightness_var = tk.DoubleVar(value=DEFAULT_BRIGHTNESS)
        self.fill_start_var = tk.DoubleVar(value=DEFAULT_FILL_START)
        self.fill_end_var = tk.DoubleVar(value=DEFAULT_FILL_END)
        self.rpm_scale_entry_var = tk.StringVar()
        self.brightness_entry_var = tk.StringVar()
        self.fill_start_entry_var = tk.StringVar()
        self.fill_end_entry_var = tk.StringVar()
        self.override_rpm_colors_var = tk.IntVar(value=DEFAULT_OVERRIDE_RPM_COLORS)
        self.enable_button_overrides_var = tk.IntVar(value=DEFAULT_ENABLE_BUTTON_OVERRIDES)
        self.force_button_colors_var = tk.IntVar(value=DEFAULT_FORCE_BUTTON_COLORS)
        self.menu_force_button_colors_var = tk.IntVar(value=DEFAULT_MENU_FORCE_BUTTON_COLORS)
        self.idle_led_writes_var = tk.IntVar(value=DEFAULT_IDLE_LED_WRITES)

        self.rpm_scale_entry = ttk.Entry(controls, width=7, justify="right", textvariable=self.rpm_scale_entry_var)
        self.brightness_entry = ttk.Entry(controls, width=7, justify="right", textvariable=self.brightness_entry_var)
        self.fill_start_entry = ttk.Entry(controls, width=7, justify="right", textvariable=self.fill_start_entry_var)
        self.fill_end_entry = ttk.Entry(controls, width=7, justify="right", textvariable=self.fill_end_entry_var)

        ttk.Label(controls, text="RPM Scale").grid(row=0, column=0, sticky="w", padx=(0, 8))
        rpm_scale = ttk.Scale(
            controls,
            from_=RPM_SCALE_MIN,
            to=RPM_SCALE_MAX,
            variable=self.rpm_scale_var,
            orient="horizontal",
            command=self.on_control_change,
        )
        rpm_scale.grid(row=0, column=1, sticky="ew", padx=(0, 8))
        self.rpm_scale_entry.grid(row=0, column=2, sticky="e", padx=(0, 12))

        ttk.Label(controls, text="Brightness").grid(row=0, column=3, sticky="w", padx=(0, 8))
        brightness = ttk.Scale(
            controls,
            from_=BRIGHTNESS_MIN,
            to=BRIGHTNESS_MAX,
            variable=self.brightness_var,
            orient="horizontal",
            command=self.on_control_change,
        )
        brightness.grid(row=0, column=4, sticky="ew", padx=(0, 8))
        self.brightness_entry.grid(row=0, column=5, sticky="e")

        ttk.Checkbutton(
            controls,
            text="Override RPM colors",
            variable=self.override_rpm_colors_var,
            command=self.on_control_toggle,
        ).grid(row=1, column=0, columnspan=2, sticky="w", pady=(6, 0))

        ttk.Checkbutton(
            controls,
            text="Enable button overrides",
            variable=self.enable_button_overrides_var,
            command=self.on_control_toggle,
        ).grid(row=1, column=2, columnspan=2, sticky="w", pady=(6, 0))

        ttk.Checkbutton(
            controls,
            text="Force button colors",
            variable=self.force_button_colors_var,
            command=self.on_control_toggle,
        ).grid(row=1, column=4, sticky="w", pady=(6, 0))

        ttk.Checkbutton(
            controls,
            text="Menu button refresh",
            variable=self.menu_force_button_colors_var,
            command=self.on_control_toggle,
        ).grid(row=1, column=5, sticky="w", pady=(6, 0), padx=(12, 0))

        ttk.Checkbutton(
            controls,
            text="Idle LED writes",
            variable=self.idle_led_writes_var,
            command=self.on_control_toggle,
        ).grid(row=2, column=0, columnspan=2, sticky="w", pady=(4, 0))

        ttk.Label(controls, text="Fill Start %").grid(row=3, column=0, sticky="w", padx=(0, 8), pady=(6, 0))
        fill_start = ttk.Scale(
            controls,
            from_=FILL_START_MIN,
            to=FILL_START_MAX,
            variable=self.fill_start_var,
            orient="horizontal",
            command=self.on_control_change,
        )
        fill_start.grid(row=3, column=1, sticky="ew", padx=(0, 8), pady=(6, 0))
        self.fill_start_entry.grid(row=3, column=2, sticky="e", padx=(0, 12), pady=(6, 0))

        ttk.Label(controls, text="Fill End %").grid(row=3, column=3, sticky="w", padx=(0, 8), pady=(6, 0))
        fill_end = ttk.Scale(
            controls,
            from_=FILL_END_MIN,
            to=FILL_END_MAX,
            variable=self.fill_end_var,
            orient="horizontal",
            command=self.on_control_change,
        )
        fill_end.grid(row=3, column=4, sticky="ew", padx=(0, 8), pady=(6, 0))
        self.fill_end_entry.grid(row=3, column=5, sticky="e", pady=(6, 0))

        self.rpm_scale_entry.bind("<Return>", lambda _e: self.on_numeric_entry_commit("rpm_scale"))
        self.rpm_scale_entry.bind("<FocusOut>", lambda _e: self.on_numeric_entry_commit("rpm_scale"))
        self.brightness_entry.bind("<Return>", lambda _e: self.on_numeric_entry_commit("brightness"))
        self.brightness_entry.bind("<FocusOut>", lambda _e: self.on_numeric_entry_commit("brightness"))
        self.fill_start_entry.bind("<Return>", lambda _e: self.on_numeric_entry_commit("fill_start"))
        self.fill_start_entry.bind("<FocusOut>", lambda _e: self.on_numeric_entry_commit("fill_start"))
        self.fill_end_entry.bind("<Return>", lambda _e: self.on_numeric_entry_commit("fill_end"))
        self.fill_end_entry.bind("<FocusOut>", lambda _e: self.on_numeric_entry_commit("fill_end"))

        controls.columnconfigure(1, weight=1)
        controls.columnconfigure(4, weight=1)

        control_values = parse_control_file(self.control_path)
        self.rpm_scale_var.set(control_values["rpm_scale"])
        self.brightness_var.set(control_values["brightness"])
        self.fill_start_var.set(control_values["fill_start"])
        self.fill_end_var.set(control_values["fill_end"])
        self.override_rpm_colors_var.set(control_values["override_rpm_colors"])
        self.enable_button_overrides_var.set(control_values["enable_button_overrides"])
        self.force_button_colors_var.set(control_values["force_button_colors"])
        self.menu_force_button_colors_var.set(control_values["menu_force_button_colors"])
        self.idle_led_writes_var.set(control_values["idle_led_writes"])
        self.update_control_labels()
        self.write_controls()

        frame = ttk.Frame(container)
        frame.pack(fill="both", expand=True)

        self.text = tk.Text(
            frame,
            bg=BG,
            fg=FG,
            insertbackground=FG,
            wrap="word",
            font=("DejaVu Sans Mono", 10),
            relief="flat",
            borderwidth=0,
        )
        self.text.pack(side="left", fill="both", expand=True)

        scrollbar = ttk.Scrollbar(frame, orient="vertical", command=self.text.yview)
        scrollbar.pack(side="right", fill="y")
        self.text.configure(yscrollcommand=scrollbar.set)

        self.text.tag_configure("INFO", foreground=LEVEL_COLORS["INFO"])
        self.text.tag_configure("SUCCESS", foreground=LEVEL_COLORS["SUCCESS"])
        self.text.tag_configure("WARN", foreground=LEVEL_COLORS["WARN"])
        self.text.tag_configure("ERROR", foreground=LEVEL_COLORS["ERROR"])

        self.root.after(100, self.poll)

    def update_control_labels(self):
        self._sync_entry_text(self.rpm_scale_entry, self.rpm_scale_entry_var, f"{self.rpm_scale_var.get():.2f}")
        self._sync_entry_text(self.brightness_entry, self.brightness_entry_var, f"{self.brightness_var.get():.2f}")
        self._sync_entry_text(self.fill_start_entry, self.fill_start_entry_var, f"{self.fill_start_var.get():.1f}")
        self._sync_entry_text(self.fill_end_entry, self.fill_end_entry_var, f"{self.fill_end_var.get():.1f}")

    def _sync_entry_text(self, entry_widget: ttk.Entry, entry_var: tk.StringVar, text: str):
        if self.root.focus_get() is entry_widget:
            return
        entry_var.set(text)

    def on_numeric_entry_commit(self, target: str):
        target_map = {
            "rpm_scale": (self.rpm_scale_entry_var, self.rpm_scale_var, RPM_SCALE_MIN, RPM_SCALE_MAX),
            "brightness": (self.brightness_entry_var, self.brightness_var, BRIGHTNESS_MIN, BRIGHTNESS_MAX),
            "fill_start": (self.fill_start_entry_var, self.fill_start_var, FILL_START_MIN, FILL_START_MAX),
            "fill_end": (self.fill_end_entry_var, self.fill_end_var, FILL_END_MIN, FILL_END_MAX),
        }
        selected = target_map.get(target)
        if selected is None:
            return

        entry_var, value_var, min_value, max_value = selected
        try:
            parsed = float(entry_var.get().strip())
            value_var.set(clamp(parsed, min_value, max_value))
        except ValueError:
            pass

        self.write_controls()

    def on_control_change(self, _value=None):
        self.update_control_labels()
        self.schedule_write()

    def on_control_toggle(self):
        self.schedule_write()

    def schedule_write(self):
        if self.write_after_id is not None:
            self.root.after_cancel(self.write_after_id)
        self.write_after_id = self.root.after(CONTROL_WRITE_DEBOUNCE_MS, self.write_controls)

    def write_controls(self):
        self.write_after_id = None
        rpm_scale = clamp(self.rpm_scale_var.get(), RPM_SCALE_MIN, RPM_SCALE_MAX)
        brightness = clamp(self.brightness_var.get(), BRIGHTNESS_MIN, BRIGHTNESS_MAX)
        fill_start = clamp(self.fill_start_var.get(), FILL_START_MIN, FILL_START_MAX)
        fill_end = clamp(self.fill_end_var.get(), FILL_END_MIN, FILL_END_MAX)
        if fill_end <= fill_start + 1.0:
            fill_end = min(FILL_END_MAX, fill_start + 1.0)

        self.rpm_scale_var.set(rpm_scale)
        self.brightness_var.set(brightness)
        self.fill_start_var.set(fill_start)
        self.fill_end_var.set(fill_end)
        self.update_control_labels()

        override_rpm_colors = 1 if self.override_rpm_colors_var.get() else 0
        enable_button_overrides = 1 if self.enable_button_overrides_var.get() else 0
        force_button_colors = 1 if self.force_button_colors_var.get() else 0
        menu_force_button_colors = 1 if self.menu_force_button_colors_var.get() else 0
        idle_led_writes = 1 if self.idle_led_writes_var.get() else 0

        tmp_path = f"{self.control_path}.tmp"
        payload = (
            "# LMU RPM bridge live controls\n"
            f"RPM_SCALE={rpm_scale:.3f}\n"
            f"BRIGHTNESS={brightness:.3f}\n"
            f"FILL_START_PERCENT={fill_start:.1f}\n"
            f"FILL_END_PERCENT={fill_end:.1f}\n"
            f"OVERRIDE_RPM_COLORS={override_rpm_colors}\n"
            f"ENABLE_BUTTON_OVERRIDES={enable_button_overrides}\n"
            f"FORCE_BUTTON_COLORS={force_button_colors}\n"
            f"MENU_FORCE_BUTTON_COLORS={menu_force_button_colors}\n"
            f"IDLE_LED_WRITES={idle_led_writes}\n"
        )

        try:
            with open(tmp_path, "w", encoding="utf-8") as f:
                f.write(payload)
            os.replace(tmp_path, self.control_path)
        except OSError as exc:
            self.append_line(f"[ERROR] Failed to write control file: {exc}")

    def append_line(self, line: str):
        line = line.rstrip("\n")
        if not line:
            return

        ts, level, msg = parse_line(line)
        if level not in LEVEL_COLORS:
            level = "INFO"

        formatted = f"[{ts}] [{level:<7}] {msg}\n"
        self.text.insert("end", formatted, level)
        self.text.see("end")

    def poll(self):
        try:
            if os.path.exists(self.log_path):
                size = os.path.getsize(self.log_path)
                if size < self.offset:
                    self.offset = 0

                with open(self.log_path, "r", encoding="utf-8", errors="replace") as f:
                    f.seek(self.offset)
                    lines = f.readlines()
                    self.offset = f.tell()

                for line in lines:
                    self.append_line(line)
        except Exception as exc:
            self.append_line(f"[ERROR] GUI poll failed: {exc}")

        self.root.after(CONTROL_POLL_MS, self.poll)

    def run(self):
        self.root.mainloop()


def main():
    log_path = sys.argv[1] if len(sys.argv) > 1 else "moza-rpm-status.log"
    control_path = sys.argv[2] if len(sys.argv) > 2 else "moza-rpm-control.env"
    app = StatusGui(log_path, control_path)
    app.run()


if __name__ == "__main__":
    main()
