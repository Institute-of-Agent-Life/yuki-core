# 核心實作進度

來源：任務書 v3.2 核心文檔補核版，ZIP SHA-256
`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。
舊行為基線：`329ca7c487414d2f54ddd98ff4727446ed2446cd`，舊倉只讀。
已依總書 K05 修正工作副本 CORE-011 第 2 步，不增加 unknown 重試權。

| 任務 | 狀態 | 分支／提交 | 驗證及產物 |
|---|---|---|---|
| CORE-001 | Windows、Mac 本地驗證通過；三平台 CI 補驗中，未合併 | `codex/rust-core-v3-2`；初始提交 `81d82ca`，三平台設定 `08442ee` | `artifacts/CORE-001/03-commit/` 為初始乾淨提交驗證；`mac-no-dns-red` 重現反查 DNS 故障，`mac-no-dns-green`、`windows-no-dns-green` 通過全部 17 個斷言；原 CI 41818029478 的 Mac 失敗產物保留在 `ci-macos-failed` |

每卡完成後獨立提交、推送，等待 Windows/macOS/Linux CI 通過再合併 main。空遠端首卡在
CI 通過後以同一已驗 SHA 建立 main；後續以 PR 合併。

尚無可供擴展使用的宿主／SDK 候選包；未發布，未進行正式跨倉驗收。
CORE-001 後選 CORE-005：一個 Rust binary、真 SQLite、資料目錄 OS lock、
啟動／遷移／健康與本機管理入口。CORE-002 的固定基線已準備到 Windows 隔離副本，
commit 與 root tree 均已核對，尚未完成 oracle。
未要求擴展倉施工；穩定協議仍需矩陣指定的真消費者證據。
