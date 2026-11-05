# Scope and historical findings

Updated: 2026-11-05 (Asia/Hong_Kong). Original audit: 2026-10-31.

[DESIGN.md](DESIGN.md) defines the current design. [TASKS.md](TASKS.md) records delivery and remaining acceptance.
This document preserves supplementary requirements and historical findings; it does not authorize code changes.
[AGENTS.md](AGENTS.md) requires a proposal and explicit `APPROVED` before code modifications.
The original documentation-only authorization did not authorize feature changes. Later task approvals govern their respective work.

## Purpose and supplementary requirements

The goal is a native Rust NanoKVM USB viewer with usable input, adjustable video, clear connection controls, screenshots, and lower memory use.
The intended environment remains Ubuntu with Sway → NanoKVM USB → Arch Linux with GNOME.
The user reported unnatural colors after V4L2 adjustment and high memory usage in the original application.
Neither the color cause nor Node.js's share of memory overhead has been established by measurement.

| ID | Original requirement | Current implementation and evidence | Remaining acceptance |
| --- | --- | --- | --- |
| S1 | Physical Meta/Super and combinations | DESIGN.md also permits another mapped key. T-10 adds optional F12-to-Meta; the Super toggle remains available. [Keyboard checks](KEYBOARD-VERIFICATION.md) pass. | T-15 input modes and shortcut inhibition; Sway/NanoKVM acceptance. Physical Meta capture remains absent. |
| S2 | Gamma and useful controls for natural video | Five GPU controls, presets, validation, and offscreen rendering have [software/GPU evidence](COLOR-VERIFICATION.md). | T-17 V4L2 panel, combined gamma checks, and natural-color reference acceptance. |
| S3 | Disconnect on problems and reconnect manually after recovery | T-05/T-07 coordinate video and serial cleanup, clear input, attempt neutral reports, and permit manual retry. [Lifecycle checks](LIFECYCLE-VERIFICATION.md) pass. | T-15 inhibition cleanup and live integration. Failed writes cannot guarantee remote release. |
| S4 | Start without connecting | T-05 starts disconnected. Enumeration alone never starts device workers. | Retain behavior during final integration; this does not concern system startup. |
| S5 | Identify matching video and serial automatically | T-04 inventories the hardware. T-06 matches the supported USB profile and rescans on Connect. [Discovery checks](DISCOVERY-VERIFICATION.md) pass. | Unknown or ambiguous candidates require manual paths. This is not multi-device management. |
| S6 | Capture images | T-12 exports full native video before GPU tuning as PNG or lossless WebP. [Screenshot checks](SCREENSHOT-VERIFICATION.md) pass. | Explicit modes, final toolbar, live capture acceptance, and T-18 area selection. |
| S7 | Reduce RAM relative to the original viewer | Bounded queues and native workers exist, but no comparable memory baseline exists. | T-13 is deferred at the user's request on 2026-11-05. No memory improvement is claimed. |

## Decisions replacing historical proposals

- Connect/Disconnect controls device connections. Capture input/Release input describes current forwarding controls.
- Focus Mode and Non-Focus Mode remain planned DESIGN.md behavior, not aliases for fully implemented modes.
- Startup enumeration is allowed while disconnected. Failure cleanup never reconnects automatically.
- The user waived further physical unplug/replug exercises under T-04; software removal and recovery checks remain required.
- Automatic pairing requires the recorded identities and topology. Ambiguous inventory never selects a guessed pair.
- PNG is the primary screenshot format; lossless WebP is also available. Both save under ~/Pictures/NanoKVM without overwriting.
- T-12 records native decoded video before GPU tuning. The earlier proposed GPU-adjusted screenshot default is superseded.
- Screenshots exclude local controls and borders. Area capture remains required under DESIGN.md and T-18.
- DESIGN.md permits simultaneous V4L2 and GPU gamma adjustments. A separate V4L2 panel remains required.
- Contrast, Temperature/Tint, and comparison views were discussion options, not approved requirements.
- Natural-color references and memory acceptance thresholds remain unsettled. The proposed T-13 measurement duration was not approved.
- Escape currently releases capture. The required Right-Ctrl, Right-Alt gesture and Escape forwarding remain under T-15.

Network streaming, audio, recording, multi-device management, and system startup configuration remain outside this scope.
No completion date, memory ceiling, or percentage improvement is inferred from the implementation language.

## Historical audit findings and current disposition

The IDs and priorities below come from the 2026-10-31 source audit.
Six temporary checks reproduced F1 and F3–F7. Those results established defects, not hardware failure rates.

| ID | Original priority/evidence | Original finding | Current disposition |
| --- | --- | --- | --- |
| F1 | P1; reproduced | A full 256-entry TX queue could drop both release reports. | T-05 clears stale queued input and bypasses the queue during shutdown. Saturation and release ordering have regression checks. |
| F2 | P1; static risk | Exit queued shutdown without waiting for serial completion. | T-05 requests cancellation and waits against one deadline. Resource closure and exit releases have software checks. |
| F3 | P1; reproduced and upstream checked | Clipboard translation consumed Ctrl+C/V/X, including empty-clipboard paste. | T-08 preserves native events through the pinned egui-winit patch. App and backend tests cover clipboard shortcuts. |
| F4 | P1; reproduced | Logical-key mapping dropped shifted punctuation. | T-08 prefers physical keys and covers shifted punctuation and releases. |
| F5 | P2; reproduced | Final batch modifiers lost event ordering. | T-08 retains native modifier order and each key's modifiers before the final snapshot. |
| F6 | P2; reproduced | Forwarded key/text events could also affect local widgets. | T-09 isolates keyboard ownership and consumes forwarded events. Local TextEdit behavior has regression checks. |
| F7 | P2; reproduced | Outside-video releases moved the remote pointer after ignored presses. | T-09 tracks pointer ownership and preserves coordinates on owned outside releases. |
| F8 | P3; repository search | Unused protocol paths, counters, and imports added noise. | Optional cleanup remains deferred. Existing unused-code warnings do not establish functional failure. |

F1–F7 software fixes do not prove physical HID delivery. Driver failures can still prevent neutral reports reaching the remote host.
Evidence is retained in [lifecycle verification](LIFECYCLE-VERIFICATION.md) and [keyboard verification](KEYBOARD-VERIFICATION.md).
The native input patch remains tied to egui-winit 0.31.1 and needs review during dependency upgrades.

## Evidence provenance

The original 2026-10-31 analysis used an isolated /tmp copy with Rust/Cargo 1.97.1.
Five original protocol tests passed; six temporary checks reproduced defects. Naga 24.0.0 validated the shader statically.
That environment had no usable Vulkan adapter, so its GPU render check did not run.
Temporary artifacts under /tmp/nanokvm-rust-audit.wHsm4I are not durable regression evidence.

Later work added Cargo.lock, permanent regression tests, and task-specific verification records.
T-04 records actual USB/V4L2 enumeration. T-11 records successful RX 6600 offscreen rendering, superseding the unavailable-GPU result for that check.
Historical test counts in earlier records describe their respective revisions; they are not the current suite size.
[INTEGRATION-VERIFICATION.md](INTEGRATION-VERIFICATION.md) records the current commands, results, requirement coverage, and remaining checks.
NanoKVM streaming, Sway input, natural-color acceptance, and original/Rust RAM comparison remain unverified.
