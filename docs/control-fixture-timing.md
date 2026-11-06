# CORE-037：Windows CI 的測試資料庫建立時間

PR #64 的 Windows job `127624280279` 在 CORE-037 的 300 秒上限中止。保留的 ZIP 顯示最後一份 SQLite 建置回條距中止僅兩秒；同一提交的另一個 Windows job 約 212 秒通過。原測試會在不同情境建立歷史版資料庫，逐條 DDL 自動提交，放大磁碟等待。

現在每個歷史 migration 與該次 SQLx checksum 記錄使用同一交易。實際 Rust migration、同步耐久性、斷言及時限不變。14 個測試階段各在開始與結束時寫入 `control-phase-timings.json`，中斷也能辨認最後執行的階段。

HJW 對拍 reader 20、40、44；前後的 schema、種子資料、migration checksum 均一致，SQLite 完整性及外鍵檢查通過。三個實例的建立時間分別由 0.750、1.172、1.922 秒減為 0.078、0.157、0.703 秒。記錄在 `artifacts/CORE-037/ci-timeout-fixture-comparison-01/windows.json`。

同一測試來源樹 `6c3a7aa71fe6e2680a57afdf1d95599c970f29d24e2d9e15570b4d2508f79b46` 在 HJW Windows、macOS、Linux 各通過 1052 項斷言。14 段測試本身分別合計 70.62、29.52、23.87 秒，不含編譯。三份 manifest 在 `artifacts/CORE-037/ci-fixture-transactions-windows-01/`、`ci-fixture-transactions-mac-01/`、`ci-fixture-transactions-linux-01/linux-validation/`。GitHub 新提交的三平台檢查另行計算。
