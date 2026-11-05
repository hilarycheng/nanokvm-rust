# Tasks

Updated: 2026-11-05 (Asia/Hong_Kong). Revision: 7.

**18 tasks: 6 delivered, 12 open.** Each task has one checkbox. Counts do not represent effort or feature completion percentages.

[DESIGN.md](DESIGN.md) defines the design requirements. Current source determines implementation status.
[AGENTS.md](AGENTS.md) defines project rules. Code changes require a proposal and explicit `APPROVED`.
This revision adds T-07 disconnection checks and manual recovery evidence. T-07 stays open for Focus Mode inhibition integration under T-15.

Existing IDs `T01` through `T14` become `T-01` through `T-14`, with their original purpose retained.
New tasks use `T-15` through `T-18`. Tasks appear in workstream order, not numerical order.
[SCOPE.md](SCOPE.md) supplies historical S1–S7 requirements and F1–F8 findings.
Startup disconnection, automatic device pairing, RAM comparison, and build delivery remain supplementary work from that scope.
They are not additional requirements attributed to DESIGN.md.

## Source baseline and design gaps

Revision 2 reviewed all eight files under `src/`, plus `Cargo.toml`, `build.sh`, and `.gitignore`.
Revision 3 verifies the release build and existing tests under T-03. GPU rendering and hardware checks remain unverified.
Revision 4 adds host USB/V4L2 enumeration evidence under T-04; it does not verify streaming or coordinated disconnect behavior.
Revision 5 adds connection lifecycle software checks; revision 6 adds automatic discovery and pairing software checks.
Revision 7 verifies unexpected cleanup, idle tty hangup, and manual serial recovery with software checks.
Historical test results in SCOPE.md remain historical evidence.

| Requirement | Current source | Tasks |
| --- | --- | --- |
| Native Rust UI with egui/eframe | `main.rs` starts eframe with wgpu; `app.rs` implements the viewer. Hardware acceptance remains open. | T-03, T-14 |
| Full-screen screenshots, including during Focus Mode; WebP and PNG output | No screenshot action, encoder, or export path exists. | T-12 |
| Area screenshots; pause remote mouse forwarding; conditional resumption | No selection state or crop workflow exists. | T-18 |
| Separate V4L2 and GPU panels; concurrent gamma controls; clear guidance | Only a GPU panel exists. `capture.rs` configures capture format but exposes no image controls. | T-11, T-17 |
| GPU Gamma, Highlights, Saturation, and saved presets through wgpu/WGSL | Controls, shader uniforms, preset saving, and startup loading exist. Rendering and persistence need acceptance. | T-11 |
| Capture and release remote keyboard and mouse input | `remote` gates forwarding. Input release discards stale queued reports; shutdown attempts neutral reports directly. Failed device writes still cannot guarantee remote release. | T-05, T-08, T-09 |
| Focus Mode captures keyboard input from Sway; local Right-Ctrl, Right-Alt release shortcut | No shortcut inhibition or right-side modifier tracking exists. Escape currently releases input. | T-15 |
| Toolbar switches Focus Mode and Non-Focus Mode; normal VNC-style input outside Focus Mode | Capture input / Release input is the only mode switch. Released input stops all forwarding. | T-09, T-15, T-16 |
| Microsoft Remote Desktop-style top toolbar; pin, auto-hide, narrow reveal strip | A fixed top panel exists. Pinning, auto-hide, and the reveal strip are absent. | T-16 |
| Toolbar Disconnect and Fullscreen actions | Fullscreen, Connect, and Disconnect exist. Toolbar styling and modes remain open. | T-05, T-16 |
| Manual disconnect stops video and serial; clears input; attempts neutral reports before serial closes | Both workers have cancellation and completion tracking. Shutdown bypasses queued input and attempts both neutral reports. | T-05 |
| Unexpected V4L2 or serial disconnection clears resources and input; failed HID release does not block cleanup | Read/write errors, video wait timeout, and idle tty probe errors initiate shared cleanup. Software checks pass; Focus Mode inhibition integration remains under T-15. | T-07, T-15 |
| Physical Meta forwarding or another key mapped to remote Meta | Only the on-screen Super toggle exists. | T-10 |

Current behavior conflicts with DESIGN.md in these areas:

- Escape releases input instead of reaching the remote host; the required right-side release shortcut is absent.
- Release input disables forwarding; it does not provide the required Non-Focus Mode behavior.
- The GPU panel advises GPU-only tuning; DESIGN.md permits V4L2 gamma and GPU gamma together.
- T-07 verifies current input cleanup on unexpected disconnection. Focus Mode inhibition cleanup remains unverified until T-15 implements inhibition.

SCOPE.md predates these requirements. Its screenshot-format questions, input terminology, and physical-Meta-only target do not override DESIGN.md.
T-14 reconciles the remaining documentation after implementation.

## Delivered artifacts

- [x] **T-01 — Create the initial project documentation**
  - Evidence: [README.md](README.md), [SCOPE.md](SCOPE.md), and AGENTS.md exist.
  - Completion covers artifact creation only. Design alignment and current operating instructions remain under T-14.

- [x] **T-02 — Create the release build script and ignore rules**
  - Evidence: [build.sh](build.sh) is executable and builds the release binary before copying it to the project root.
  - [.gitignore](.gitignore) excludes `/target/` and `/nanokvm-rust`.
  - Completion covers file creation only. Actual build output is verified under T-03.

## Build and device evidence

- [x] **T-03 — Verify the release build and root executable**
  - Basis: native Rust desktop delivery; existing build-tool scope. Depends on T-02.
  - Touchpoints: `build.sh`, `.gitignore`, `Cargo.toml`, `src/main.rs`, and build records.
  - Work: run the release build and verify the copied binary, executable permissions, help output, and build-failure handling.
  - Done: the root binary matches the release output. Artifacts are ignored; source and Cargo.lock remain trackable.
  - Evidence: record commands, toolchain, resolved dependency versions, and results. Existing protocol tests pass with that dependency resolution.
  - Result: [BUILD-VERIFICATION.md](BUILD-VERIFICATION.md) records the successful cached offline release build, matching binaries, executable permissions, both help options, ignore rules, and two build-failure checks.
  - Result: 5/5 protocol tests pass with the generated [Cargo.lock](Cargo.lock). Online registry access failed; hardware and GUI acceptance remain outside this task.

- [x] **T-04 — Identify the physical NanoKVM device pair**
  - Basis: supplementary S5. Requires accessible NanoKVM hardware.
  - Touchpoints: USB identity, topology, serial ports, V4L2 nodes, and supported capture formats.
  - Work: record the local video/serial candidate, USB identities, shared hub topology, and pairing limits.
  - Done: record matching attributes and supported formats. Document absent, ambiguous, and multiple-device cases without guessing a pairing.
  - Evidence: hardware enumeration records. This task does not implement discovery or initiate a connection.
  - Scope adjustment (2026-11-05): the user waived further physical unplug/replug exercises. Software removal/re-enumeration handling remains required under T-05 through T-07.
  - Result: [DEVICE-IDENTIFICATION.md](DEVICE-IDENTIFICATION.md) records capture `345f:2131`, serial `1a86:7523`, shared hub `1a40:0101`, changed USB device numbers, stable observed identities, and MJPG/YUYV modes.
  - Limit: this closes the revised inventory scope, not automatic pairing or disconnect implementation. Shared hub ancestry alone does not prove enclosure identity.

## Connection and cleanup

- [x] **T-05 — Complete Connect, Disconnect, and orderly shutdown**
  - Basis: DESIGN.md manual cleanup; supplementary S3/S4; F1/F2. Depends on T-03.
  - Touchpoints: `Viewer::new`, `connect`, `disconnect`, `release_remote`, `send`, `Drop`, `CameraHandle`, and `SerialHandle`.
  - Work: start disconnected and connect video plus serial on request. Add coordinated worker stop and completion handling.
  - Done: Disconnect stops capture and serial I/O, releases resources, and clears local keyboard, mouse, and mode state.
  - Done: attempt neutral keyboard and mouse reports before closing available serial. Queue saturation cannot silently discard the release request.
  - Done: failed release attempts still allow bounded cleanup. Application exit follows the same lifecycle; later Connect restarts both workers.
  - Evidence: checks cover full queues, partially opened connections, repeated disconnects, and shutdown. Software-owned resources verify closure; the user waived physical unplugging.
  - Result: [LIFECYCLE-VERIFICATION.md](LIFECYCLE-VERIFICATION.md) records 15 passing tests and the release build. Startup is disconnected; retries wait for both workers to finish.
  - Limit: a kernel call that ignores its timeout cannot be forcibly cancelled by a Rust thread. The UI reports delayed shutdown and blocks reconnect; app exit waits at most two seconds.

- [x] **T-06 — Discover and pair devices automatically**
  - Basis: supplementary S5. Depends on T-04 and T-05.
  - Touchpoints: device enumeration, `Options`, CLI defaults, and connection UI.
  - Work: use verified identity and topology to select the matching video and serial devices.
  - Done: one identified NanoKVM needs no manual path selection. Replugging and changed device numbers preserve correct pairing.
  - Done: missing or ambiguous matches show a clear status. Discovery alone never starts streaming or serial I/O.
  - Evidence: software matching checks cover absent, single, multiple, and ambiguous candidates, plus removal and changed device numbers. Physical replug exercises are not required.
  - Result: [DISCOVERY-VERIFICATION.md](DISCOVERY-VERIFICATION.md) records 27 passing tests and the release build. Automatic mode scans at startup, Rescan, and Connect without automatic reconnection.
  - Limit: matching supports the T-04 VID/PID and hub-port profile only. Unknown topology and multiple candidates require manual paths; generic hardware with identical identities cannot be distinguished by inventory alone.

- [ ] **T-07 — Clean up unexpected disconnections and support manual recovery**
  - Basis: DESIGN.md unexpected cleanup; supplementary S3. Depends on T-05; discovery is not a cleanup prerequisite.
  - Touchpoints: capture errors, serial errors, worker status delivery, `refresh_messages`, and shared disconnect handling.
  - Work: define detectable disconnection and stall conditions. Route failures through coordinated cleanup and report the cause.
  - Done: V4L2 or serial loss clears local input and releases available resources, including Focus Mode inhibition.
  - Done: attempt neutral HID reports when serial remains available. Report failure never prevents cleanup or freezes the UI.
  - Done: repaired hardware supports manual Connect for both workers. No automatic reconnection or unsupported claim of delivered release occurs.
  - Evidence: software failure checks cover either worker failing, release writes failing, and device loss while inputs remain held. Physical unplug exercises are not required.
  - Result: [LIFECYCLE-VERIFICATION.md](LIFECYCLE-VERIFICATION.md#t-07-unexpected-disconnection-and-manual-recovery) records 31 passing tests. Linux pseudo-terminals verify idle serial hangup and manual reopening; injected video failure verifies cleanup after held input was transmitted.
  - Result: idle serial checks use the existing library's FIONREAD probe without sending input. Video errors include the 500 ms wait limit. Release failures retain their cause and do not prevent cleanup.
  - Remaining: keep this task open until T-15 implements and verifies Focus Mode inhibition release through the shared cleanup path. Real V4L2 streaming and hardware HID delivery remain unverified.

## Keyboard, mouse, and modes

- [ ] **T-08 — Correct keyboard forwarding**
  - Basis: DESIGN.md remote input; F3–F5. Depends on T-05.
  - Touchpoints: `raw_input_hook`, `map_key`, `sync_modifiers`, `KeyboardReport`, and the native event path where required.
  - Work: preserve physical keys and event-specific modifier order. Handle Ctrl+C/V/X despite local clipboard event translation.
  - Done: shortcuts, Shift punctuation, presses, and releases reach the target correctly, including with an empty local clipboard.
  - Evidence: focused regression checks cover clipboard shortcuts, physical mapping, and multiple modifier changes within one event batch.

- [ ] **T-09 — Isolate local controls and remote pointer input**
  - Basis: DESIGN.md keyboard/mouse control and Non-Focus Mode; F6/F7. Depends on T-05 and T-08.
  - Touchpoints: `raw_input_hook`, `render_video`, `handle_mouse`, local widgets, and input release handling.
  - Work: define video interaction ownership for normal VNC-style input. Keep toolbar, settings, and screenshot gestures local.
  - Done: forwarded keyboard input does not also edit local widgets. Local toolbar clicks do not move the remote pointer.
  - Done: dragging outside the image still releases held buttons. Focus loss clears held input without leaving stuck keys.
  - Evidence: regression checks cover local text focus, outside-image clicks, drag release, and window focus loss.

- [ ] **T-15 — Implement Focus Mode, Non-Focus Mode, and local release**
  - Basis: DESIGN.md input modes and Sway keyboard capture. Depends on T-08 and T-09.
  - Touchpoints: `Viewer` mode state, native keyboard events, viewer-window shortcut inhibition, and release/cleanup paths.
  - Work: implement the required `Right-Ctrl, Right-Alt` local release shortcut with right-side key detection.
  - Work: confirm whether the documented shortcut means a chord or sequence before implementing its gesture.
  - Done: Focus Mode sends keyboard input to the remote host instead of activating Sway shortcuts.
  - Done: the reserved shortcut releases capture locally. Escape reaches the remote host as a normal key.
  - Done: Non-Focus Mode supports normal VNC-style video interaction while local desktop shortcuts remain available.
  - Done: mode exit, focus loss, disconnect, and shutdown release inhibition and held inputs. Toolbar actions remain usable during Focus Mode.
  - Evidence: mode-transition checks and Sway hardware tests confirm shortcut capture, local release, and desktop recovery.

- [ ] **T-10 — Support a keyboard path to remote Meta**
  - Basis: DESIGN.md permits physical Meta forwarding or another key mapped to remote Meta. Depends on T-15.
  - Touchpoints: native key handling, `sync_modifiers`, HID modifier bits, and mapping UI if the remapping option is chosen.
  - Work: implement one design-compliant keyboard path. The existing on-screen Super toggle alone does not satisfy this requirement.
  - Done: Meta combinations reach the target. Releasing the physical or mapped key clears remote Meta reliably.
  - Done: mapping does not consume the reserved local release shortcut. Mode exit and focus loss clear Meta state.
  - Evidence: key-report checks and Sway hardware tests cover combinations, release, and return to local desktop input.

## Toolbar

- [ ] **T-16 — Complete the remote desktop toolbar**
  - Basis: DESIGN.md toolbar behavior. Depends on T-05 and T-15; screenshot actions integrate with T-12 and T-18.
  - Touchpoints: `toolbar`, viewport Fullscreen commands, pointer hit testing, and screenshot actions.
  - Work: style the top toolbar like Microsoft Remote Desktop. Add pinning and auto-hide with a persistent narrow reveal strip.
  - Done: pointer entry into the strip reveals the toolbar. Pinning keeps it visible; auto-hide keeps actions reachable.
  - Done: toolbar actions include Focus Mode, Non-Focus Mode, Disconnect, Fullscreen, full-screen capture, and area capture.
  - Done: windowed and fullscreen layouts preserve the reveal strip. Local toolbar interaction does not forward remote mouse events.
  - Evidence: UI checks cover both modes, pinned and hidden states, pointer reveal, fullscreen transitions, and unavailable actions.

## Color controls

- [ ] **T-17 — Add the standard V4L2 image-control panel**
  - Basis: DESIGN.md separate panels, concurrent gamma, and guidance. Depends on T-05; hardware acceptance uses T-04.
  - Touchpoints: capture-device control access, worker communication, and `color_panel`.
  - Work: enumerate supported standard image controls, values, ranges, steps, and availability. Apply edits through the device-owning path.
  - Done: V4L2 controls appear separately from GPU controls. Unsupported or unavailable controls have clear status.
  - Done: V4L2 gamma and GPU gamma can remain active together when the device supports gamma.
  - Done: labels explain device processing versus GPU processing. Replace the current GPU-only guidance.
  - Evidence: checks cover control ranges, unsupported controls, write errors, and disconnection during edits. Hardware validates actual image changes.

- [ ] **T-11 — Validate GPU controls and preset persistence**
  - Basis: DESIGN.md GPU controls and presets; supplementary S2. Depends on T-03 and T-05; combined-panel acceptance uses T-17.
  - Touchpoints: `ColorSettings`, `color_panel`, `save_colors`, `config_path`, `GpuVideo`, and `color.wgsl`.
  - Work: retain Gamma, Highlights, Saturation, Exposure, White peak, Reset, and Soft white. Verify existing wgpu/WGSL processing.
  - Done: labels explain each GPU adjustment and its interaction with V4L2 controls. Both panels remain independently adjustable.
  - Done: saving and restoring GPU presets preserves valid values. Missing, invalid, or unwritable presets produce safe, understandable behavior.
  - Evidence: persistence checks, shader validation, and actual GPU rendering verify adjustments and reset behavior.
  - Evidence: use hardware reference images for natural-color acceptance. Contrast, Temperature/Tint, and comparison views are not automatic additions.

## Screenshots

- [ ] **T-12 — Export full-screen screenshots as WebP or PNG**
  - Basis: DESIGN.md full-screen capture and file formats; supplementary S6. Depends on T-05; toolbar integration uses T-16.
  - Touchpoints: toolbar capture action, `RgbaFrame`, selected color-processing output, image encoding, and file writes.
  - Work: settle capture surface, pixel dimensions, color stage, destination, and WebP settings before implementation.
  - Work: implement both WebP and PNG output. The formats are requirements, not unresolved options from SCOPE.md.
  - Done: the toolbar captures the full screen while Focus Mode remains active. Capture does not require releasing remote input.
  - Done: saved images match the selected format and agreed capture behavior. The UI reports the saved location or failure.
  - Done: missing frames and write failures are handled. Existing files are not overwritten silently.
  - Evidence: decode exported files and verify dimensions and pixels. Exercise both formats during Focus Mode and Non-Focus Mode.

- [ ] **T-18 — Capture a user-selected area with safe mouse forwarding**
  - Basis: DESIGN.md partial capture and conditional input resumption. Depends on T-09, T-12, and T-15.
  - Touchpoints: toolbar area action, selection overlay, image-coordinate conversion, mouse routing, and screenshot export.
  - Work: start area selection from the toolbar and pause remote mouse forwarding before selection gestures begin.
  - Done: selection gestures never reach the remote host. Existing held remote buttons cannot remain stuck during selection.
  - Done: crop coordinates match the captured image across scaling and letterboxing. Empty selections and cancellation have clear outcomes.
  - Done: the selected area exports through the WebP/PNG path from T-12.
  - Done: resume remote mouse forwarding only if Focus Mode remains active after capture; apply the same guard after cancellation or failure.
  - Done: leaving Focus Mode or disconnecting during selection never restores previous capture state automatically.
  - Evidence: checks cover crop bounds, both formats, cancellation, save failure, mode changes, and disconnect during selection.

## Measurement and final acceptance

- [ ] **T-13 — Measure and reduce RAM use**
  - Basis: supplementary S7. Baselines can start after T-03 and T-04, before optimization.
  - Touchpoints: complete process-tree measurements, frame buffers, capture queues, and measured allocation hotspots.
  - Work: agree the original application version, measurement method, workload, duration, and acceptance criteria.
  - Done: compare original, Rust baseline, and final implementation under matching hardware, resolution, FPS, and workload.
  - Evidence: record steady-state usage, peaks, and long-session trends. Optimize measured costs without losing validation or hardware calibration.
  - Final comparison follows the functional tasks. Do not promise an unmeasured memory limit or percentage reduction.

- [ ] **T-14 — Complete integration acceptance and operating documentation**
  - Basis: every DESIGN.md requirement plus retained supplementary scope. Depends on T-03 through T-13 and T-15 through T-18.
  - Touchpoints: the integrated viewer, regression checks, hardware evidence, README.md, SCOPE.md, and TASKS.md.
  - Work: verify discovery, Connect, both input modes, the release shortcut, Meta, toolbar behavior, both color panels, and screenshots.
  - Work: verify manual disconnect, either device failing, bounded release attempts, shutdown, and manual reconnection.
  - Done: each requirement has acceptance evidence. Required hardware checks have no unresolved FAIL or NOT RUN result.
  - Done: operating documentation uses English and matches actual behavior, terminology, capture choices, and limitations.
  - Done: reconcile historical SCOPE.md assumptions and approval wording. Do not modify AGENTS.md or DESIGN.md.
  - Evidence: retain commands, environment, results, and remaining limitations. Reuse unaffected checks instead of repeating them unnecessarily.

## Progress and maintenance

| Workstream | Task IDs | Delivered | Open |
| --- | --- | --- | --- |
| Initial artifacts | T-01, T-02 | 2 | 0 |
| Build and device evidence | T-03, T-04 | 2 | 0 |
| Connection and cleanup | T-05, T-06, T-07 | 2 | 1 |
| Keyboard, mouse, and modes | T-08, T-09, T-15, T-10 | 0 | 4 |
| Toolbar | T-16 | 0 | 1 |
| Color controls | T-17, T-11 | 0 | 2 |
| Screenshots | T-12, T-18 | 0 | 2 |
| Measurement and final acceptance | T-13, T-14 | 0 | 2 |
| Total | T-01–T-18 | 6 | 12 |

- Keep one checkbox per task and preserve IDs when updating descriptions or ordering.
- Check a task only when its stated completion criteria and evidence are satisfied.
- Update the date, counts, and evidence whenever a checkbox changes.
- Mark unavailable hardware checks NOT RUN and leave dependent acceptance tasks open.
- Existing source alone does not prove release-build, rendering, Sway, or NanoKVM acceptance.
- Propose affected callers, tests, fixtures, configuration, and validation before seeking code approval under AGENTS.md.
- This checklist does not authorize code changes or expand scope beyond its stated requirements.
- F8 dead-code cleanup remains optional. Network streaming, audio, recording, and multi-device management remain outside this plan.
