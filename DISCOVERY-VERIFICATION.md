# T-06 automatic device discovery

Date: 2026-11-05 (Asia/Hong_Kong).

## Behavior and supported profile

Without CLI paths, the viewer starts in Automatic devices mode and scans without connecting.
Rescan refreshes the displayed paths. Each Connect performs a fresh scan before opening either worker.
Disconnect cancels a pending connection request; a late discovery result cannot start workers.
Supplying CLI paths selects manual mode. Manual mode requires both Video and Serial paths.

Automatic matching uses the observed profile in [DEVICE-IDENTIFICATION.md](DEVICE-IDENTIFICATION.md):

| Component | Required identity |
| --- | --- |
| Video USB device | VID:PID `345f:2131`, child port 2 |
| Serial USB device | VID:PID `1a86:7523`, child port 4 |
| Nearest parent hub | VID:PID `1a40:0101`, same canonical sysfs parent for both endpoints |
| Video node | Device capabilities contain Video Capture and Streaming |

The host bus, upstream port, USB device number, and video/tty node numbers are not hard-coded.
Metadata-only nodes are excluded using V4L2 device capabilities, not a `video-index0` naming assumption.
Unrelated USB identities are ignored. Missing components, multiple candidates, different hubs, and unsupported topology prevent automatic selection.
An unreadable or vanished candidate aborts the scan instead of silently reducing the candidate list.
Failure clears previously detected paths and keeps the devices disconnected.

Discovery reads sysfs and queries V4L2 capabilities. It never starts capture, sets a capture format, or opens/writes the serial port.
V4L2 capability queries use the installed v4l library's nonblocking device open and release the handle afterward.
The scan runs off the UI thread. Cancellation discards its result; it does not forcibly interrupt a kernel query already in progress.

## Software validation

Rust/Cargo 1.97.1 with the existing Cargo.lock; no new dependency or configuration file.

```sh
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
```

Result: 27 tests pass, including the previous 15 checks, eight discovery fixture checks, and four viewer integration checks.
The release build passes with existing unused-code warnings.

Filesystem fixtures exercise the production sysfs traversal and matching logic with an injected capability probe.
They cover one pair, metadata exclusion, unrelated devices, no devices, either component missing, different hubs, multiple pairs,
multiple capture nodes, changed node numbers and hub location, unknown hub IDs, wrong child ports, malformed identities,
capability-query failure, missing device nodes, broken sysfs links, and removal during capability queries.

Viewer checks verify that discovery alone never starts workers, an explicit connection uses freshly returned paths,
failed discovery clears stale paths, and Disconnect suppresses a pending discovery connection.
Existing manual-path and connection lifecycle tests also pass.

No physical unplugging, live serial I/O, GPU session, or hardware streaming test was performed for T-06.

## Limits

This is an observed-layout profile, not a universal NanoKVM identifier.
Generic peripherals with the same identities and layout cannot be distinguished by this inventory alone.
Other hardware revisions require additional evidence before extending automatic matching; manual paths remain available.
Multiple matching units intentionally require manual paths because multi-device management is outside scope.

A device can disappear after a successful scan. Worker open/read/write failures use the existing cleanup path;
the inventory does not make device opening atomic with enumeration. Full device-loss acceptance remains under T-07.
Automatic mode rescans on startup, Rescan, and Connect. It does not reconnect automatically.
