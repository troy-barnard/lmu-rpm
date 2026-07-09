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


def parse_line(raw: str):
    m = LOG_LINE_RE.match(raw)
    if m:
        return m.group("time"), m.group("level"), m.group("msg")

    m = LEGACY_LINE_RE.match(raw)
    if m:
        return m.group("time"), "INFO", m.group("msg")

    return "--:--:--", "INFO", raw


class StatusGui:
    def __init__(self, log_path: str):
        self.log_path = log_path
        self.offset = 0

        self.root = tk.Tk()
        self.root.title("LMU RPM Bridge Status")
        self.root.geometry("860x500")
        self.root.configure(bg=BG)

        frame = ttk.Frame(self.root)
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

        self.root.after(300, self.poll)

    def run(self):
        self.root.mainloop()


def main():
    log_path = sys.argv[1] if len(sys.argv) > 1 else "moza-rpm-status.log"
    app = StatusGui(log_path)
    app.run()


if __name__ == "__main__":
    main()
