#!/usr/bin/env python3
"""Lightweight brake graph overlay for trail-braking practice.

The overlay stays on top of the desktop and plots the most recent brake input
samples so you can watch the onset and release timing of a braking zone.
"""

from __future__ import annotations

import re
import tkinter as tk
from collections import deque
from typing import Optional


BRAKE_PATTERNS = (
    re.compile(r"(?i)\bbrake(?:_percent|percent|_input|input)?\s*[:=]\s*(\d+(?:\.\d+)?)\s*%?"),
    re.compile(r"(?i)\bbrake\s+(\d+(?:\.\d+)?)\s*%"),
    re.compile(r"(?i)\bbrake(?:\s+percent\s*[:=]?\s*|\s*[:=]\s*)(\d+(?:\.\d+)?)\s*%?"),
)


def clamp(value: float, minimum: float = 0.0, maximum: float = 100.0) -> float:
    # Keep the visual trace within a sensible 0-100% range even if a noisy log line is
    # temporarily outside the valid brake-domain bounds.
    return max(minimum, min(maximum, value))


def parse_brake_percent(raw_message: str) -> Optional[float]:
    """Return a clamped brake percentage from a log or telemetry line, if any."""
    if not raw_message:
        return None

    for pattern in BRAKE_PATTERNS:
        match = pattern.search(raw_message)
        if match is None:
            continue
        try:
            value = float(match.group(1))
        except ValueError:
            continue
        if value < 0.0 or value > 100.0:
            return None
        return float(value)
    return None


class BrakeGraphOverlay:
    """Transparent, draggable overlay window with a recent brake trace."""

    def __init__(self, root: Optional[tk.Misc] = None, max_points: int = 96, width: int = 420, height: int = 180):
        self.max_points = max_points
        self.history: deque[float] = deque(maxlen=max_points)
        self.current_value = 0.0
        self.has_data = False
        self.width = width
        self.height = height
        self.window = None
        self.canvas = None
        self.drag_x = 0
        self.drag_y = 0
        self.is_dragging = False

        if root is not None:
            self._create_window(root)

    def _create_window(self, root: tk.Misc):
        # Build a lightweight always-on-top overlay so the brake trace stays visible while
        # retaining the ability to drag it around the desktop without a full window manager.
        self.window = tk.Toplevel(root)
        self.window.withdraw()
        self.window.attributes("-topmost", True)
        self.window.overrideredirect(True)
        self.window.configure(bg="#0b1017")
        self.window.attributes("-alpha", 0.9)
        self.window.geometry(f"{self.width}x{self.height}+60+60")

        self.canvas = tk.Canvas(
            self.window,
            width=self.width,
            height=self.height,
            bg="#0b1017",
            highlightthickness=0,
        )
        self.canvas.pack(fill="both", expand=True)
        self.canvas.bind("<ButtonPress-1>", self._begin_drag)
        self.canvas.bind("<B1-Motion>", self._drag)
        self.canvas.bind("<ButtonRelease-1>", self._end_drag)

        self.render()
        self.set_visible(True)

    def _begin_drag(self, event):
        if self.window is None:
            return
        self.is_dragging = True
        self.drag_x = event.x_root - self.window.winfo_x()
        self.drag_y = event.y_root - self.window.winfo_y()

    def _drag(self, event):
        if not self.is_dragging or self.window is None:
            return
        new_x = event.x_root - self.drag_x
        new_y = event.y_root - self.drag_y
        self.window.geometry(f"+{new_x}+{new_y}")

    def _end_drag(self, _event):
        self.is_dragging = False

    def set_visible(self, visible: bool):
        if self.window is None:
            return
        if visible:
            self.window.deiconify()
        else:
            self.window.withdraw()

    def handle_line(self, raw_message: str) -> bool:
        value = parse_brake_percent(raw_message)
        if value is None:
            return False
        self.record_sample(value)
        return True

    def record_sample(self, value: float):
        sample = clamp(float(value), 0.0, 100.0)
        self.history.append(sample)
        self.current_value = sample
        self.has_data = True
        if self.canvas is not None and self.canvas.winfo_exists():
            self.render()

    def render(self):
        if self.canvas is None or not self.canvas.winfo_exists():
            return

        canvas = self.canvas
        canvas.delete("all")
        width = self.width
        height = self.height
        pad_left = 26
        pad_right = 10
        pad_top = 18
        pad_bottom = 18

        plot_left = pad_left
        plot_right = width - pad_right
        plot_top = pad_top
        plot_bottom = height - pad_bottom

        canvas.create_rectangle(1, 1, width - 2, height - 2, outline="#455a64", fill="#0b1017", width=1)

        if not self.has_data:
            canvas.create_text(width / 2.0, height / 2.0, text="Waiting for brake telemetry", fill="#9db0c0", font=("DejaVu Sans Mono", 11, "bold"))
            return

        for band in (0, 25, 50, 75, 100):
            y = plot_bottom - (band / 100.0) * (plot_bottom - plot_top)
            canvas.create_line(plot_left, y, plot_right, y, fill="#1e2d38", width=1)
            canvas.create_text(plot_left - 8, y, text=f"{band}", anchor="e", fill="#b7c6d9", font=("DejaVu Sans Mono", 9))

        canvas.create_text(width / 2.0, 10, text="Brake %", fill="#dfeaf5", font=("DejaVu Sans Mono", 10, "bold"))

        if len(self.history) <= 1:
            canvas.create_text(width / 2.0, height / 2.0, text=f"{self.current_value:.0f}%", fill="#ff6b6b", font=("DejaVu Sans Mono", 18, "bold"))
            return

        points = []
        for index, value in enumerate(self.history):
            x_ratio = index / (len(self.history) - 1)
            y_ratio = 1.0 - (value / 100.0)
            x = plot_left + x_ratio * (plot_right - plot_left)
            y = plot_top + y_ratio * (plot_bottom - plot_top)
            points.append((x, y))

        # Draw only the line trace (no fill) for a cleaner look
        canvas.create_line([point for pair in points for point in pair], fill="#ff6b6b", width=2.5)

        latest_x = points[-1][0]
        latest_y = points[-1][1]
        canvas.create_oval(latest_x - 4, latest_y - 4, latest_x + 4, latest_y + 4, fill="#ffd166", outline="#ffd166")
        canvas.create_text(width - 18, 18, text=f"{self.current_value:.0f}%", anchor="ne", fill="#ffd166", font=("DejaVu Sans Mono", 12, "bold"))


if __name__ == "__main__":
    root = tk.Tk()
    root.withdraw()
    overlay = BrakeGraphOverlay(root)
    overlay.record_sample(0)
    overlay.record_sample(12)
    overlay.record_sample(34)
    overlay.record_sample(68)
    overlay.record_sample(92)
    overlay.record_sample(51)
    overlay.record_sample(18)
    root.mainloop()
