# moza-rpm

A dead simple utility that drives LEDs on Moza wheels. This is intended to
be used when gaming on Linux using Proton. It may also work on Windows, but I
haven't tested it there.

To build:

    cargo build --release

Before running, you need to configure your wine prefix with the correct
serial port for your Moza devices. If you haven't already, you should set up
udev rules for your hardware following the instructions for the [boxflat][1]
application. Once the hardware is setup and you've identified the right
serial device for your hardware (in my case, /dev/ttyACM0), then you can
configure your wine prefix using regedit.

    protontricks [steam_app_id] regedit

In `HKEY_LOCAL_MACHINE\Software\Wine\Ports`, add a new string value with a
label `COM1`, and use the device path as the value (e.g., `/dev/ttyACM0`).
For now, the `COM1` port is hard-coded into moza-rpm, so you must use that
label.

To run:

    protontricks-launch --appid=[steam_app_id] moza-rpm.exe

The LEDs should all illuminate briefly, then the utility will attempt to
connect to whichever sim is running in the proton prefix. The utility
depends on [simetry][2] for reading telemetry from sims, and should
support the same games that simetry does.

If you plan on using this every time you race, then you may find it easier
to automatically start it using a tool like [Datalink][3]. If you'd prefer
a more full-featured telemetry utility, consider using [monocoque][4]
instead.

Note: This has only been tested on my own personal Moza GS V2P wheel on a
Moza R9v3 base.

[1]: https://github.com/Lawstorant/boxflat
[2]: https://github.com/adnanademovic/simetry
[3]: https://github.com/LukasLichten/Datalink
[4]: https://github.com/Spacefreak18/monocoque
