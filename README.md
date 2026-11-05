# NanoKVM USB — Rust Viewer

原生 Linux NanoKVM USB viewer。目標係支援實體 Meta／Super、可調整至自然嘅畫面，以及較低 RAM 用量。

目前係可編譯嘅 MVP。鍵鼠轉發有已知問題，完整硬件行為尚未驗證。

- [Scope、功能差距及驗證紀錄](SCOPE.md)
- [工作清單及完成進度](TASKS.md)
- [Agent 工作規則及 APPROVE 流程](AGENTS.md)

## 目標環境

沿用原 project 設定：Ubuntu + Sway host → NanoKVM USB → Arch Linux + GNOME target。

## 目前已有

- 獨立 capture thread 經 V4L2 讀取 UVC MJPEG，再解碼成 RGBA。
- 要求 1920×1080 MJPG；實際解像度採用裝置協商結果。
- 透過 `wgpu`／WGSL 顯示影像及調色。
- Gamma、Exposure、Highlights、White peak、Saturation controls。
- Reset、Soft white preset、儲存及啟動時載入調色設定。
- 基本 keyboard HID、absolute mouse、Capture input／Release input。
- 獨立 serial thread，以 57,600 baud 發送 reports。

## 目前啟動方式

需要 Rust toolchain、eframe／wgpu 及 V4L2 所需系統 libraries，以及 video／serial device 存取權限。

The app starts disconnected. Without CLI paths, Automatic devices scans for the supported NanoKVM USB layout.
Select Connect to rescan and open video plus serial. Rescan alone never connects.

建置 release binary，並複製到 project root：

```sh
./build.sh
```

輸出檔案為 `./nanokvm-rust`。Build artifacts 留喺 `target/`。

亦可直接用 Cargo 執行：

```sh
cargo run --release
# Optional manual override:
cargo run --release -- --video /dev/video0 --serial /dev/ttyUSB0
```

For manual paths, disable Automatic devices and set both Video and Serial in the status bar.
Supplying CLI paths also selects manual mode. Multiple candidates or unsupported layouts require manual selection.
Disconnect clears local input and video, stops both workers, and attempts neutral HID reports before serial closes.
Connect becomes available after both workers finish. App exit uses the same shutdown path.

Once serial is ready and a video frame is available, use Capture input or click the image.
Keyboard reports follow video focus. Clicking local controls releases held remote input and keeps local typing local.
Pointer presses belong to the video only when they start there. Releasing a video drag outside the image releases its remote button.
With video keyboard focus, Escape releases input. Release input also ends forwarding. Escape forwarding remains pending.

Enable F12 → Meta in the toolbar to use held F12 as remote Meta while video has keyboard focus.
Release F12 to release its Meta hold. The Super toggle independently holds Meta until turned off.
The mapping defaults off at startup; turn it off to send ordinary F12. Changing the mapping releases held remote input.
The setting lasts for the current application session, including disconnect/reconnect.

Release input clears queued input and requests neutral reports while devices remain connected. Disconnect closes both device workers.

## 調色設定

Color 面板只調整遠端影像。Save preset 儲存至：

- `$XDG_CONFIG_HOME/nanokvm-rust/colors.json`；或
- `~/.config/nanokvm-rust/colors.json`。

Hover over each GPU control for its effect. GPU adjustments follow V4L2 device processing; both gamma controls can be active together.
The separate V4L2 panel remains pending. Reset 1:1 resets GPU adjustments only; Soft white sets white peak to 0.84.
Neither button changes the saved preset until Save preset is selected.

Startup restores finite preset values within the control ranges. Missing, invalid, or unreadable presets use GPU defaults with a status message.
Save preset writes a complete temporary file before replacing the saved preset. Failures preserve the previous preset and show their cause.
Empty or relative XDG_CONFIG_HOME values fall back to an absolute HOME; unavailable configuration paths are reported.

RX 6600 offscreen rendering verifies the GPU controls and Reset. Natural-color acceptance still needs NanoKVM hardware reference images.
Contrast, Temperature/Tint, and comparison views remain outside this task.

## 已知限制

- Optional F12 mapping and the Super toggle provide remote Meta. Physical Meta capture and Wayland shortcut inhibition remain absent.
- Ctrl+C/V/X, Shift punctuation, and modifier ordering have software coverage under T-08. Hardware HID delivery remains unverified.
- The pinned egui-winit patch preserves keys before clipboard translation. Recheck it before upgrading egui/eframe.
- Local input isolation has software coverage under T-09. Focus Mode and screenshot selection integration remain pending.
- Neutral reports bypass queued input on shutdown. Failed device writes cannot guarantee remote release.
- A driver that ignores timeouts may delay closure. Reconnect stays unavailable while a worker remains alive.
- Automatic pairing supports the recorded T-04 USB profile. Multiple candidates and unknown layouts require manual paths.
- 尚未提供 Screenshot。
- 尚未量度原版同 Rust viewer 嘅 RAM 差距。

完整問題及證據見 [SCOPE.md](SCOPE.md)。

## 驗證狀態

2026-10-31 嘅分析曾於 `/tmp` 獨立副本完成 crate test build。Rust 1.97.1 下，原有 5 個 protocol tests 全部通過。

```sh
cargo test --all-targets
```

另外 6 個臨時測試成功重現已知問題，並未加入此 repo。WGSL 通過 Naga 24.0.0 靜態驗證。GPU 實際 render 因環境沒有可用 Vulkan adapter 而未能執行。NanoKVM、Sway 實機測試及 RAM 比較仍未執行。

Cargo.lock records the dependency resolution. See [LIFECYCLE-VERIFICATION.md](LIFECYCLE-VERIFICATION.md) for T-05, T-07, and T-09 software checks and limits.
See [DISCOVERY-VERIFICATION.md](DISCOVERY-VERIFICATION.md) for T-06 matching rules, software checks, and supported hardware limits.
See [KEYBOARD-VERIFICATION.md](KEYBOARD-VERIFICATION.md) for T-08/T-10 checks, mapping limits, and the native input patch.
See [COLOR-VERIFICATION.md](COLOR-VERIFICATION.md) for current T-11 GPU rendering, preset checks, and remaining acceptance.

## Source 分工

| 檔案 | 責任 |
| --- | --- |
| [main.rs](src/main.rs) | CLI 參數及 App 啟動 |
| [app.rs](src/app.rs) | GUI、輸入狀態、連接操作及設定 |
| [capture.rs](src/capture.rs) | V4L2 MJPEG 擷取及解碼 |
| [gpu.rs](src/gpu.rs)、[color.wgsl](src/color.wgsl) | Texture upload、GPU rendering 及調色 |
| [input.rs](src/input.rs) | Keyboard mapping、modifiers 及影像座標換算 |
| [protocol.rs](src/protocol.rs) | Serial framing、HID reports 及 protocol tests |
| [serial.rs](src/serial.rs) | Serial worker 及 report queue |

## Protocol 參考

參考 `sipeed/NanoKVM-USB@1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc` 嘅 [proto.ts](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/desktop/src/main/device/proto.ts)、[device/index.ts](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/desktop/src/main/device/index.ts) 及 renderer keyboard／mouse／media 實作。

`Cargo.toml` 聲明 `GPL-3.0-only`。上游授權見 [LICENSE](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/LICENSE)。
