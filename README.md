# NanoKVM USB — Rust Viewer

原生 Linux NanoKVM USB viewer。目標係支援實體 Meta／Super、可調整至自然嘅畫面，以及較低 RAM 用量。

目前係可編譯嘅 MVP。鍵鼠轉發有已知問題，完整硬件行為尚未驗證。

- [Scope、功能差距及驗證紀錄](SCOPE.md)
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

**以下係現有行為，尚未符合新 scope：啟動 App 會即時嘗試開啟 video。傳入 `--serial` 亦會自動連接 serial。**

```sh
cargo run --release -- --video /dev/video0 --serial /dev/ttyACM0
```

目前仍須自行提供正確 device paths。GUI 底部嘅 Serial 欄位及 Connect 只處理 serial，唔會重新啟動停止咗嘅 video thread。

連接 serial 後，按 Capture input 或雙擊影像啟用遠端輸入。按 Esc 或 Release input 退出控制。Esc 目前唔會送到 target。

**Release input 只停止輸入轉發，唔等於 Disconnect。** 目前沒有完整 video＋serial Disconnect 操作。

## 調色設定

Color 面板只調整遠端影像。Save preset 儲存至：

- `$XDG_CONFIG_HOME/nanokvm-rust/colors.json`；或
- `~/.config/nanokvm-rust/colors.json`。

Gamma 等 controls 已有實作，但實際自然度仍需用使用者嘅硬件及參考畫面驗收。Contrast、Temperature／Tint 同原圖對比目前未實作，亦未定為必須新增功能。

## 已知限制

- 只有畫面上嘅 Super toggle。實體 Meta／Super capture 同 Wayland shortcut inhibition 未完成。
- Ctrl+C／V／X、部分 Shift 符號、modifier 時序及本機／遠端輸入隔離有已重現問題。
- Queue 滿載或 App 退出時，遠端按鍵釋放未有可靠保證。
- 尚未自動識別及配對 USB serial／V4L2 devices。
- 尚未提供 Screenshot。
- 尚未量度原版同 Rust viewer 嘅 RAM 差距。

完整問題及證據見 [SCOPE.md](SCOPE.md)。

## 驗證狀態

2026-10-31 嘅分析曾於 `/tmp` 獨立副本完成 crate test build。Rust 1.97.1 下，原有 5 個 protocol tests 全部通過。

```sh
cargo test --all-targets
```

另外 6 個臨時測試成功重現已知問題，並未加入此 repo。WGSL 通過 Naga 24.0.0 靜態驗證。GPU 實際 render 因環境沒有可用 Vulkan adapter 而未能執行。NanoKVM、Sway 實機測試及 RAM 比較仍未執行。

本 repo 尚未包含 `Cargo.lock`。以上結果對應當次 dependency resolution，唔代表日後解析出嘅所有版本組合都已驗證。

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
