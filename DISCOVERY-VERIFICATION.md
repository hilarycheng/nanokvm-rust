# T-06 automatic device discovery

Date: 2026-11-05 (Asia/Hong_Kong).

## Behavior and supported profile

Without CLI paths, the viewer scans at startup and automatically connects one valid pair without a Connection dialog.
Multiple valid pairs open a selection dialog. Selection triggers a fresh scan to confirm the pair's paths and canonical hub.
Missing devices and scan failures open an error prompt. The offline screen provides Connect for a manual retry.
Scanning and connecting show progress on the main screen, with Cancel available before shutdown begins.
Disconnect cancels a pending connection request; a late discovery result cannot start workers.
Supplying both CLI paths connects directly in manual mode. An incomplete manual configuration opens an error prompt.

Automatic matching uses the observed profile in [DEVICE-IDENTIFICATION.md](DEVICE-IDENTIFICATION.md):

| Component | Required identity |
| --- | --- |
| Video USB device | VID:PID `345f:2131`, child port 2 |
| Serial USB device | VID:PID `1a86:7523`, child port 4 |
| Nearest parent hub | VID:PID `1a40:0101`, same canonical sysfs parent for both endpoints |
| Video node | Device capabilities contain Video Capture and Streaming |

The host bus, upstream port, USB device number, and video/tty node numbers are not hard-coded.
Metadata-only nodes are excluded using V4L2 device capabilities, not a `video-index0` naming assumption.
Unrelated USB identities are ignored. Complete pairs on separate hubs remain separate choices, sorted by canonical hub path.
Missing components, ambiguous endpoints within a hub, mismatched hubs, and unsupported topology prevent automatic selection.
An unreadable or vanished candidate aborts the scan instead of silently reducing the candidate list.
Failure clears previously detected paths and keeps the devices disconnected.

Discovery reads sysfs and queries V4L2 capabilities. It never starts capture, sets a capture format, or opens/writes the serial port.
V4L2 capability queries use the installed v4l library's nonblocking device open and release the handle afterward.
The scan runs off the UI thread. Cancellation discards its result; it does not forcibly interrupt a kernel query already in progress.

## Startup auto-connect validation

```sh
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
```

Result: 83 passed, zero failed, and two ignored hardware/GPU tests.
The release build passes with existing unused-code warnings and updates the root executable; CLI help reflects the new flow.
Wayland mock tests require local socket operations outside the restricted sandbox; no desktop session is connected.
Discovery fixtures cover zero, single, and multiple pairs, including ambiguous or incomplete endpoints and changed paths.
Viewer tests cover startup without a Connection dialog, selection, cancellation, error acknowledgement, and the offline Connect button.
Selection revalidation checks the chosen pair, rejects changed hubs, and never substitutes another remaining device.
Startup retains unrelated errors, and incomplete manual paths never start workers.
Existing cleanup, input, screenshot, and toolbar regression tests also pass.
No live NanoKVM connection, physical unplugging, or GPU rendering check was performed for this change.

## Original T-06 software validation

The following results record the original implementation before startup auto-connect and device selection.

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
Multiple complete pairs use the selection dialog. Ambiguous endpoints within one hub still require verified manual paths.
Selection revalidation rejects changed paths or hub topology instead of falling back to another connected device.

A device can disappear after a successful scan. Worker open/read/write failures use the existing cleanup path;
the inventory does not make device opening atomic with enumeration. Full device-loss acceptance remains under T-07.
Automatic mode scans on startup, device selection, and Connect. It does not reconnect automatically after failure or cancellation.
