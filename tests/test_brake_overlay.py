import importlib.util
import sys
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

MODULE_PATH = SCRIPTS_DIR / "brake_overlay.py"
SPEC = importlib.util.spec_from_file_location("brake_overlay", MODULE_PATH)
module = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(module)

parse_brake_percent = module.parse_brake_percent

status_gui_path = SCRIPTS_DIR / "status-gui.py"
status_gui_spec = importlib.util.spec_from_file_location("status_gui", status_gui_path)
status_gui = importlib.util.module_from_spec(status_gui_spec)
assert status_gui_spec and status_gui_spec.loader
status_gui_spec.loader.exec_module(status_gui)
parse_control_file = status_gui.parse_control_file


def test_parse_brake_percent_from_common_labels():
    assert parse_brake_percent("brake_percent=54.5") == 54.5
    assert parse_brake_percent("Brake: 42") == 42.0
    assert parse_brake_percent("brake 88%") == 88.0


def test_parse_brake_percent_rejects_invalid_values():
    assert parse_brake_percent("rpm=8200") is None
    assert parse_brake_percent("brake=abc") is None


def test_brake_history_tracks_recent_samples():
    overlay = module.BrakeGraphOverlay(None)
    assert overlay.has_data is False

    overlay.record_sample(10)
    overlay.record_sample(30)
    overlay.record_sample(85)

    assert list(overlay.history)[-3:] == [10, 30, 85]
    assert overlay.current_value == 85
    assert overlay.has_data is True


def test_status_gui_tracks_brake_overlay_toggle(tmp_path):
    control_path = tmp_path / "moza-rpm-control.env"
    control_path.write_text("BRAKE_OVERLAY=0\n", encoding="utf-8")

    values = parse_control_file(str(control_path))

    assert values["brake_overlay"] == 0
