# Agent 工作規則

## Project 指引

- 先讀 [README.md](README.md) 嘅目前行為。
- 再讀 [SCOPE.md](SCOPE.md) 嘅需求、差距、待決事項及驗證紀錄。
- Scope 文件記錄目標，唔等於授權實作。
- 工作採用維修 Task Card 形式。每項操作有目的、前置條件、驗收結果及 HOLD point。

## 表達及準確性

1. 使用使用者嘅語言。所有可見中文使用繁體，保留 English technical terms。
2. 使用短句及主動語態。每句一個意思，每步一個動作。
3. 分清事實、推論及未知。使用者描述須標示為使用者觀察，未量度數值不可當成事實。
4. 每個問題附 source 位置、觸發情況及影響。只問會影響工作嘅必要問題。
5. 不得將討論選項變成已確認需求。不得捏造完成比例、效能數字或驗證結果。

## 工作程序

### 1. ANALYSE — 先分析

- 先提出簡短分析計劃。
- 讀取相關 source。追蹤 callers、tests、fixtures、config 及 exports。
- 找出現有可重用實作。說明變更可能影響嘅輸入、資料及使用者行為。
- 分析階段不得修改 project files，包括 code、tests、dependencies 同文件。
- 可讀取檔案及在 `/tmp` 隔離驗證。不得因此改動使用者設定或向 target 發送未獲授權嘅鍵鼠輸入。
- 未獲批准前，只提交分析及概要方案。不得提供待執行 code、commands 或最終設計。

### 2. PROPOSE — 提交 Task Card

在對話提交一張精簡 Task Card。無需為每張卡建立獨立檔案。

| 欄位 | 必須包含 |
| --- | --- |
| Task ID／Revision | 識別本次方案及其版本 |
| 目的／Scope | 對應 SCOPE.md 嘅 S1–S7，或明確嘅文件任務 |
| 前置條件／證據 | 已確認問題、必要環境及未知事項 |
| 修改範圍 | 受影響檔案、callers、tests、config 及最小方案 |
| 驗證 | 預期結果、必要測試及需要實機確認嘅項目 |
| 回退／停止條件 | 如何撤回本次改動，以及何時需要重新批准 |

### 3. HOLD — 等待 APPROVE

- 只有使用者明確回覆 `APPROVE`，先可執行對應方案。
- 批准只適用於當時清楚列明嘅範圍及 revision。使用者可在同一訊息批准一個明確範圍。
- 舊任務嘅批准、沉默、詢問、討論或 scope 確認，唔構成功能修改授權。
- 「只整理文件」只批准文件修改。不得連帶修改 code、tests、Cargo.toml 或 lockfile。
- 「discuss only」期間只討論。之後以使用者明確授權嘅範圍為準。
- 已批准範圍內嘅例行操作毋須重複確認。

### 4. EXECUTE — 執行已批准範圍

- 採用最小完整修改。優先重用現有模組、標準庫及已安裝 dependencies。
- 修正共同根因。同步完成批准範圍內必要嘅 callers、tests 及文件更新。
- 不得夾帶無關重構、功能、config、dependency 或預留抽象。
- 保留原有驗證、錯誤處理、accessibility 及硬件校準需要。
- 保留使用者原有改動。回退只撤回自己本次改動，唔假設一定有 Git metadata。
- 新增 scope 或重大方案改變時，先更新 Task Card，再等 `APPROVE`。
- 普通測試失敗可在已批准範圍內修正。需要越界先可解決嘅問題，須 HOLD 相關工作並交代原因。

### 5. VERIFY — 按風險驗證

- 非簡單邏輯或 bug fix 應有一個能驗證行為嘅精簡 test／self-check。
- 文件修改檢查內容、連結及修改範圍。毋須為純文件修改重跑完整 build。
- 使用 `PASS`、`FAIL`、`NOT RUN` 記錄每項結果，同時列出預期及實際行為。
- 成功重現 bug 唔代表 bug 已修好。編譯或 shader 靜態驗證通過唔代表實機通過。
- 硬件相關驗收須交代裝置、環境及限制。未能執行就標示 `NOT RUN`。
- 測試通過後停止無必要嘅重複檢查。不得降低驗收要求去掩蓋失敗。

### 6. RECORD — 報告結果

- 列出實際變更、驗證證據、未完成項目及剩餘風險。
- 分清「實作完成」、「測試通過」同「實機未驗證」。未完成必要驗收不得聲稱整項功能已驗證。
- 更新相關既有文件，避免另建重複紀錄。
- 完成已批准 Task 並記錄驗證後，建立 Conventional Commit，格式為 `<type>(<scope>): <summary>`；只提交該 Task 嘅檔案，保留其他既有 staged changes。
- 回覆最後用一至兩句交代未檢查項目及使用者需要知道嘅風險。

## 本 project 嘅固定解讀

- S4 指每次開啟 App 唔自動 Connect，唔係 system startup 設定。
- Device discovery 同 Connect 係不同操作。發現裝置唔代表可以開始 streaming 或控制。
- 使用者要求 Gamma 同其他實用調色功能。不要預設要全面重寫影像 pipeline 或加入全部討論過嘅 sliders。
- 色彩問題先分清實際畫面表現及可能原因。是否新增 Contrast／Temperature／Tint 須按 scope 確認。
- RAM 改善須有同條件量測。不得因為使用 Rust 就聲稱一定比原版省 RAM。
- USB 配對須有身份證據。不得用第一個 `/dev/videoN` 同第一個 serial port 猜配對。
- Release input、Disconnect 同 Screenshot 必須清楚區分。
- 不得聲稱實體斷線後仍可保證 HID release 送達。
- 修改或放寬本檔規則同樣需要明確批准。
