# Scope 及現況差距

更新日期：2026-10-31（Asia/Hong_Kong）。

本文件記錄使用者需求、目前 source 行為及分析證據。當次 `APPROVE` 只授權整理文件，未授權修改功能 code。

## 目的及基準

建立原生 Rust NanoKVM USB viewer，支援實體 Meta／Super、可調至自然嘅畫面、清楚嘅連接操作、自動裝置識別、截圖及較低 RAM 用量。

使用者觀察：

- 現有 V4L2 使用體驗調過顏色後仍唔自然。
- 原版 NanoKVM 嘅 Node.js 實作佔用較多 RAM。
- 使用者進一步指出需要 Gamma 同其他可調色功能。新增邊啲 controls 仍可討論。

以上係問題背景，唔代表已確認色偏原因，亦唔代表 Node.js 本身係 RAM 開銷嘅唯一原因。尚未有色彩參考或 RAM baseline。

平台基準沿用原 README：Ubuntu + Sway host → NanoKVM USB → Arch Linux + GNOME target。分析暫以單部 NanoKVM 為基準；多裝置行為尚未確認。

## 要求與差距

| ID | 使用者要求 | 目前實作／證據 | 尚欠工作 |
| --- | --- | --- | --- |
| S1 | Handle 實體 Meta／Super，同相關組合鍵 | [app.rs](src/app.rs) 只有 Super toggle；[input.rs](src/input.rs) 未處理實體 Super，未實作 Wayland shortcut inhibition | 實體按鍵路徑、host shortcut 處理、release 行為及 Sway 實機驗收 |
| S2 | Gamma 同其他實用調色，讓畫面自然 | [app.rs](src/app.rs)、[gpu.rs](src/gpu.rs)、[color.wgsl](src/color.wgsl) 已有 Gamma、Exposure、Highlights、White peak、Saturation、Reset 及 preset 儲存／載入 | 確認實際畫面問題；驗證現有 controls；決定是否需要額外調整；以實機參考畫面驗收 |
| S3 | Hardware 有問題時可以斷開，修復後由使用者再 Connect | [serial.rs](src/serial.rs) 開啟／寫入失敗會報錯並退出 worker；[capture.rs](src/capture.rs) 擷取失敗會停止。GUI Connect 只重開 serial | 完整 Disconnect／故障清理、video 重啟、按鍵釋放及手動恢復流程。故障判定與 timeout 細節待分析 |
| S4 | 每次開啟 App 都唔自動 Connect | [app.rs](src/app.rs) 啟動即呼叫 `capture::spawn`；提供 serial path 時會呼叫 `connect_serial` | 將啟動狀態改為 Disconnected；等使用者 Connect 先開始連接。此要求唔涉及 system startup |
| S5 | 自動找出 NanoKVM 對應 USB serial 同 V4L2 devices，唔使自己 select | [main.rs](src/main.rs) 預設 `/dev/video0`，另收 serial path；GUI 仍有手動 Serial 欄位 | Device discovery、身份配對、重新枚舉及錯誤說明；需要實際 USB identity／topology 證據 |
| S6 | 可以 Capture 圖 | 沒有 Screenshot 按鈕或影像輸出流程 | 擷取影像、儲存及錯誤處理；格式、調色前／後、儲存位置等細節待確認 |
| S7 | 減少相對原版 NanoKVM 嘅 RAM 用量 | 已採用 native Rust、有限容量 queues 及獨立 workers；沒有 RAM 量測 | 建立原版／Rust 同條件 baseline；量度穩態、峰值及持續播放趨勢；按結果處理主要開銷 |

差距摘要：

- S1、S5、S6 缺少核心功能。
- S4 嘅現有啟動行為與要求相反。
- S2、S3 有部分實作，但未完成整體要求。
- S7 未有比較數據，改善幅度未知。
- 七項目標均未完成完整驗收。以上分類唔係工時或完成百分比估算。

## 操作原則及待決事項

建議以 Connect／Disconnect 表示裝置連接，以 Capture input／Release input 表示鍵鼠控制，以 Screenshot 表示截圖。

建議啟動時可以枚舉裝置，但保持 Disconnected。Connect 後再獨立啟用 input capture。故障後停止失效操作，由使用者手動重連。現有 App 尚未完整實現呢個流程。

以下係討論選項，唔可直接當成已批准實作：

| 項目 | 待確認內容 |
| --- | --- |
| 額外調色 | Contrast、Temperature／Tint、原圖／調色對比；先確認現有 controls 是否足夠 |
| 色彩驗收 | 使用者主要見到嘅係灰濛濛、太暗、光位過亮或偏色；使用邊幅參考圖 |
| Screenshot | PNG、原始影像解像度、套用當前調色、不含 toolbar 係建議預設，未逐項確認 |
| 故障斷線 | 持續無 frame／I/O timeout 嘅判定門檻；video 同 serial 嘅清理協調方式 |
| 多個候選 device | 單部裝置自動配對係分析基準；身份不明時保持斷線係建議保護方式，唔可猜配對 |
| RAM 驗收 | 使用邊個原版版本、相同解像度／FPS、測試時間及一致嘅 process tree 記憶體指標；未設定 MB 上限或減幅 |
| Input release | 保留一個清楚可用嘅本機 release 方法；實體 Meta、焦點離開及 host 保留 shortcuts 需實機確認 |

本輪未納入 network streaming、audio、錄影、多裝置管理或 system startup 設定。不得自動擴展至以上功能。

## 已知問題

以下編號沿用 2026-10-31 source audit。六項「已重現」指臨時軟件測試重現行為，唔代表已做 NanoKVM 實機驗證。

| ID | 優先級／證據 | 問題及影響 | Source 位置 |
| --- | --- | --- | --- |
| F1 | P1／已重現 | 256 格 TX queue 滿載時，兩個 release reports 均可被丟棄。本機退出 capture 後，遠端可能仍持有按鍵 | `app.rs`：`send`、`release_remote` |
| F2 | P1／靜態風險 | App 退出只排隊 Shutdown，沒有等待 serial worker。Process 可能早於 release／傳送完成而結束；硬件發生率未知 | `app.rs`：`Drop`；`serial.rs`：`spawn` |
| F3 | P1／已重現及上游核對 | egui-winit 將 Ctrl+C／V／X 轉成 clipboard events；hook 只接受 Key，相關按鍵未轉發。空 clipboard 亦可能沒有 Paste event | `app.rs`：`raw_input_hook` |
| F4 | P1／已重現 | 使用 logical key 而忽略 physical key，導致 `: + ? \| ! {` 等 Shift 符號被 mapping 丟棄 | `app.rs`：`raw_input_hook`；`input.rs`：`map_key` |
| F5 | P2／已重現 | 只套用整批 input 嘅最終 modifiers，忽略事件時序。同一批 Shift+A 後放開 Shift，可送出沒有 Shift 嘅 A | `app.rs`：`raw_input_hook` |
| F6 | P2／已重現 | Hook 轉發後保留 Key／Text events。本機文字欄位有焦點時，同一次輸入亦可影響本機 widget | `app.rs`：`raw_input_hook`、Serial TextEdit |
| F7 | P2／已重現 | 影像外 press 被忽略，但 release 仍會更新座標。Toolbar click 可令遠端 pointer 跳到影像邊緣 | `app.rs`：`handle_mouse` |
| F8 | P3／全 repo 搜尋 | Relative mouse 路徑、RIGHT_SUPER 未使用；frames_seen 只遞增；另有未使用 import。暫無功能影響 | `protocol.rs`、`serial.rs`、`app.rs` |

F1、F2 直接影響 S1／S3 嘅 release 可靠性。F3–F7 影響既有基本鍵鼠控制。修正方案須先按 [AGENTS.md](AGENTS.md) 提交分析及 Task Card；本文件唔授權修正。

F3 上游證據：[egui-winit 0.31.1 keyboard event handling](https://github.com/emilk/egui/blob/0.31.1/crates/egui-winit/src/lib.rs#L764)。

## 已有驗證證據

以下紀錄來自 2026-10-31 前一輪分析。今輪文件整理已比對 source，確認 `src/` 同 `Cargo.toml` 與當次分析副本一致；未重跑 build 或硬件測試。

| 檢查 | 結果 | 證據及限制 |
| --- | --- | --- |
| Crate test build | PASS | `/tmp` 副本執行 `cargo test --all-targets`；Rust／Cargo 1.97.1；eframe／egui-wgpu 0.31.1。唔係 release build 或實機驗收 |
| Repo 原有 tests | PASS | 5／5：keyboard checksum、checksum error、payload 長度、mouse coordinates、key rollover／release |
| 臨時問題重現 tests | PASS（重現） | 6／6 重現 F1、F3–F7；未修正問題，未加入 repo tests |
| WGSL 靜態驗證 | PASS | Naga 24.0.0 parse／validation 通過；確認 vertex／fragment entry points |
| GPU render／像素讀回 | NOT RUN | Harness 已編譯，但找不到可用 Vulkan adapter，未進入 render 驗證 |
| NanoKVM、USB 配對及拔插 | NOT RUN | 未有實機結果 |
| Sway 實體 Meta／release | NOT RUN | 未有實機結果 |
| 自然色彩及 preset 實機驗收 | NOT RUN | 未有使用者參考畫面或實機結果 |
| 原版／Rust RAM 比較 | NOT RUN | 未有 baseline 或量測數據 |

臨時證據位於 `/tmp/nanokvm-rust-audit.wHsm4I/`：`audit_input_tests.rs`、`audit_shader.rs`、當次 `Cargo.lock` 及 build artifacts。`/tmp` 可能被清理；唔視為持久 regression suite。此 repo 目前仍只有 5 個 protocol tests。

Repo 尚未有 `Cargo.lock` 或 CI。本次文件授權不包含新增 lockfile、tests、CI 或 dependencies。

## 下一步分析範圍

1. 確認 NanoKVM identity、V4L2 nodes 同 serial 配對依據。
2. 確認使用者實際色彩問題，並收集同條件 RAM baseline。
3. 分析 Connect／Disconnect、錯誤恢復及 release lifecycle。
4. 分析實體 Meta 同已知鍵盤／滑鼠問題。
5. 確認調色及 Screenshot 待決細節。
6. 提交最小實作 Task Card，等待功能修改嘅 `APPROVE`。

以上係後續建議次序。未有可靠硬件資料前，唔估完成日期或 RAM 改善百分比。
