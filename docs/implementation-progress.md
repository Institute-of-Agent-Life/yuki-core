# 核心實作進度

來源：任務書 v3.2 核心文檔補核版，ZIP SHA-256
`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。
舊行為基線：`329ca7c487414d2f54ddd98ff4727446ed2446cd`，舊倉只讀。
已依總書 K05 修正工作副本 CORE-011 第 2 步，不增加 unknown 重試權。

| 任務 | 狀態 | 分支／提交 | 驗證及產物 |
|---|---|---|---|
| CORE-001 | 完成，已進 main | `c85bf8ee182d5075c1abaa8b1a56f51e16b0a196`；分支 `codex/rust-core-v3-2` | [三平台 CI 41818720821](https://github.com/Institute-of-Agent-Life/yuki-core/actions/runs/41818720821) 全過；`artifacts/CORE-001/` 保留正常、故障與修正證據 |
| CORE-005 | 本機驗證通過，待 CI 合併 | `codex/core-005-owner`，`feat(store): enforce one writer owner and offline bootstrap (CORE-005)` | `artifacts/CORE-005/windows-05/` 34 個斷言，`mac-sigterm-green/` 36 個斷言，兩端各 3 個 Rust 故障測試。`linux-container-01/` 已驗無外網、跨 UID 拒絕及關機 |

每卡完成後獨立提交、推送，等待 Windows/macOS/Linux CI 通過再合併 main。空遠端首卡在
CI 通過後以同一已驗 SHA 建立 main；後續以 PR 合併。

尚無可供擴展使用的宿主／SDK 候選包；未發布，未進行正式跨倉驗收。
目前選 CORE-005：一個 Rust binary、真 SQLite、資料目錄 OS lock、
啟動／遷移／健康與本機管理入口。CORE-002 的固定基線已準備到 Windows 隔離副本，
commit 與 root tree 均已核對，尚未完成 oracle。
未要求擴展倉施工；穩定協議仍需矩陣指定的真消費者證據。
