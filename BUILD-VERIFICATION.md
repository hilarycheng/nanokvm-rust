# T-03 release-build verification

Date: 2026-11-05 (Asia/Hong_Kong).

Scope: release delivery, CLI help, artifact ignore rules, build failure handling, and existing protocol tests.
Application source, build configuration, and build script remain unchanged.

## Environment and dependency resolution

- Linux 7.0.0-38-generic, x86_64; Rust target `x86_64-unknown-linux-gnu`.
- Toolchain: `stable-x86_64-unknown-linux-gnu`.
- `rustc 1.97.1 (8bab26f4f 2026-07-14)`, LLVM 22.1.6.
- `cargo 1.97.1 (c980f4866 2026-06-30)`.
- C compiler: Ubuntu GCC 13.3.0-6ubuntu2~24.04.1.
- `pkg-config` versions: libudev 255, wayland-client 1.22.0, xkbcommon 1.6.0.

The initial `./build.sh` attempt exited 101 because crates.io was unreachable.
An offline attempt using the default Cargo cache exited 101 because eframe was unavailable.
The successful dependency resolution uses the existing audit cache at `/tmp/nanokvm-rust-audit.wHsm4I/cargo-home`.
These versions reflect that cache, not a fresh online registry resolution.
The generated [Cargo.lock](Cargo.lock) records the full dependency graph and registry checksums.

| Direct dependency | Resolved version |
| --- | --- |
| anyhow | 1.0.104 |
| bytemuck | 1.25.2 |
| crossbeam-channel | 0.5.17 |
| eframe | 0.31.1 |
| egui-wgpu | 0.31.1 |
| jpeg-decoder | 0.3.2 |
| serde | 1.0.229 |
| serde_json | 1.0.154 |
| serialport | 4.10.1 |
| v4l | 0.14.0 |

## Commands

Run from the repository root:

```sh
rustc -vV
cargo --version
rustup show active-toolchain
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo tree --locked --offline --depth 1
```

The temporary cache may be deleted later. A normal Cargo installation can obtain the locked packages from crates.io.

## Results

| Check | Result |
| --- | --- |
| Release build through `./build.sh` | PASS; exit 0; optimized build completed in 2m 06s. |
| `cmp target/release/nanokvm-rust nanokvm-rust` | PASS; identical bytes. |
| `stat -c '%a %s %n' target/release/nanokvm-rust nanokvm-rust` | PASS; both mode 775, size 13,668,624 bytes. |
| `file nanokvm-rust` | x86-64 Linux ELF PIE executable, dynamically linked. |
| `./nanokvm-rust --help` and `./nanokvm-rust -h` | PASS; both exit 0 with the usage text below. |
| `cargo test --release --locked --offline --all-targets` with the recorded cache | PASS; 5 passed, 0 failed, 0 ignored. |
| `git check-ignore -q --no-index` for the root binary and release artifact | PASS; both ignored. |
| The same ignore check for Cargo.lock, Cargo.toml, build.sh, and every file in src/ | PASS; none ignored; all remain trackable. |
| Isolated build failures | PASS; both scenarios below. |

```text
Usage: nanokvm-rust [--video /dev/video0] [--serial /dev/ttyACM0]
```

Both binary SHA-256 hashes are `0ca40d3e733d68bea7a1d221626efd477dca925b7c11c8a863fa8406a16e5c6c`.
The Cargo.lock SHA-256 is `0bd6935ce9cecbfe7b9699e43a687214e04d9a2b450baf462db20e41f8e77028`.
The lockfile remained unchanged during tests.

The five tests cover keyboard checksums, checksum rejection, oversized payloads, mouse coordinates, and key rollover/release.
The release build reported eight existing unused-import/dead-code warnings; the test build reported four.
Warning cleanup remains outside T-03.
Raw command logs are under `/tmp/nanokvm-t03-evidence/`; this record and Cargo.lock retain the results if temporary logs disappear.

## Failure handling

An isolated copy of the unchanged `build.sh` ran from outside a project directory containing spaces.
Its PATH placed a `cargo` symlink to `/bin/false` first, forcing a build failure without changing the repository.
Both checks passed:

- Without a root executable, the script exited 1, printed no success message, and created no executable.
- With an existing root executable, the script exited 1 and preserved its bytes and executable permissions.

A failed build leaves an older successful executable in place. Check the build exit status before using that executable.

## Scope limits

GUI launch, GPU rendering, NanoKVM hardware, Sway input, and RAM measurements were not run for T-03.
Their acceptance remains with the corresponding tasks in [TASKS.md](TASKS.md).
