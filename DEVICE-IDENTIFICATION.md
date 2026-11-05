# T-04 device identity and pairing evidence

Date: 2026-11-05 (Asia/Hong_Kong).

T-04 completes the device inventory under the user's revised acceptance: no further physical unplug/replug exercises are required.
Software handling of removal, changed device numbers, and ambiguous candidates remains required under T-05 through T-07.
This record does not claim those features are implemented.

## Observed devices

Host enumeration shows one video device and one USB serial device beneath the same four-port USB hub.

| Role | USB VID:PID | Identity | Sysfs USB path | Device node |
| --- | --- | --- | --- | --- |
| Parent hub | `1a40:0101` | Terminus USB2.0 HUB | `2-11` | USB hub |
| Video | `345f:2131` | MACROSILICON USB2 Video, serial `52081161` | `2-11.2` | `/dev/video0` |
| Serial | `1a86:7523` | QinHeng CH340 / USB Serial; no unique serial string exposed | `2-11.4` | `/dev/ttyUSB0` |

`/dev/video0` exposes Video Capture through `uvcvideo`.
`/dev/video1` belongs to the same USB device but exposes Metadata Capture in its device capabilities.
It must not be selected as the image capture node.
The serial driver is `ch341`.

Observed links:

```text
/dev/v4l/by-id/usb-MACROSILICON_USB2_Video_52081161-video-index0 -> ../../video0
/dev/serial/by-id/usb-1a86_USB_Serial-if00-port0 -> ../../ttyUSB0
/dev/serial/by-path/pci-0000:00:14.0-usb-0:11.4:1.0-port0 -> ../../ttyUSB0
```

The video ID_PATH is `pci-0000:00:14.0-usb-0:11.2:1.0`.
The serial ID_PATH is `pci-0000:00:14.0-usb-0:11.4:1.0`.
The video ID_SERIAL is `MACROSILICON_USB2_Video_52081161`; the serial ID_SERIAL is `1a86_USB_Serial`.
The latter contains no per-unit serial number and cannot distinguish multiple identical adapters.

## Re-enumeration evidence and limits

Two host snapshots captured the following bus 002 device numbers:

| Device | Initial snapshot | Final snapshot |
| --- | --- | --- |
| Hub | 028 | 084 |
| Video | 035 | 087 |
| Serial | 032 | 086 |

The final snapshot retains the same VID/PID values, video serial number, topology, udev identities, and node names.
This demonstrates changed USB enumeration numbers; it does not demonstrate changed video or tty node numbers.
The stopped udev monitor also reported CH340 binding at `2-11.4` with DEVNUM 086.
Its full event output was truncated, so it is not treated as a complete removal/addition trace.
No further physical exercise is required by the user's revised T-04 scope.

Shared hub ancestry and these identities support the observed local pairing candidate.
They do not prove physical enclosure identity or establish a universal rule for every NanoKVM revision.
Do not hard-code host bus numbers, device numbers, `/dev/video0`, or `/dev/ttyUSB0` as identities.

## Advertised capture formats

`v4l2-ctl --list-formats-ext -d /dev/video0` reports the following discrete modes.
Rates are the FPS values printed by the driver, not measured streaming performance.

| Resolution | MJPG FPS | YUYV FPS |
| --- | --- | --- |
| 2560x1440 | 30, 25, 20, 10 | 10, 5 |
| 1920x1080 | 50, 30, 25, 20, 10 | 10, 5 |
| 1360x768 | 60, 50, 30, 20, 10 | 15, 8 |
| 1280x1024 | 60, 50, 30, 20, 10 | 15, 8 |
| 1280x960 | 60, 50, 30, 20, 10 | 15, 8 |
| 1280x720 | 60, 50, 30, 20, 10 | 20, 10 |
| 1024x768 | 60, 50, 30, 20, 10 | 20, 10 |
| 800x600 | 60, 50, 30, 20, 10 | 30, 20, 10 |
| 720x576 | 60, 50, 30, 20, 10 | 50, 25, 20, 10 |
| 720x480 | 60, 50, 30, 20, 10 | 60, 30, 20, 10 |
| 640x480 | 60, 50, 30, 20, 10 | 60, 30, 20, 10 |

The existing request for 1920x1080 MJPG is among the advertised modes.
Streaming, image quality, and serial HID output were not exercised.

## Missing, ambiguous, and multiple-device cases

These are constraints for T-06 implementation, not current application behavior:

- No eligible capture or serial endpoint: report the missing component and do not select a pair.
- Metadata-only video nodes: exclude them using device capabilities.
- One candidate pair: check USB identities and nearest hub ancestry; do not pair solely by enumeration order.
- Multiple compatible endpoints on one hub: report ambiguity unless additional evidence identifies the pair.
- Multiple hubs or identical CH340 adapters: retain each candidate's topology; the serial by-id link alone is insufficient.
- Re-enumeration: refresh the inventory and reject stale paths. Host port changes can change by-path links.

Only the current single-candidate topology was observed.
Absent and multiple-device behavior needs software tests under T-06; no such behavior is claimed as tested here.

## Current code handling

- `src/main.rs` still defaults to `/dev/video0` and accepts a manual serial path. Automatic pairing is absent.
- `src/capture.rs` propagates open, format, and frame-read errors out of its worker and reports a stopped status.
- `src/serial.rs` reports open/write failures and exits. On worker exit it attempts neutral reports without checking delivery.
- `src/app.rs::refresh_messages` updates status text; it does not coordinate cleanup of both workers.
- `connect_serial` restarts only serial. Video recovery and complete input/resource cleanup remain T-05/T-07 work.

The existing partial handling does not meet DESIGN.md's coordinated disconnect requirements.
Removing physical replug acceptance does not waive these code requirements.

## Read-only commands and environment

Host: Linux 7.0.0-38-generic, x86_64. The sandbox exposed sysfs/udev data but omitted device nodes.
Host-level read-only execution provided USB and V4L2 access; sandbox absence was not treated as physical device absence.

```sh
lsusb -t
lsusb -d 345f:2131
lsusb -d 1a86:7523
udevadm info --query=all --name=/dev/video0
udevadm info --query=all --name=/dev/ttyUSB0
v4l2-ctl -d /dev/video0 --list-formats-ext
v4l2-ctl -d /dev/video1 --info
ls -l /dev/v4l/by-id /dev/serial/by-id /dev/serial/by-path
```

The udev monitor was stopped. No device reset, serial transmission, or capture stream was initiated by these checks.
