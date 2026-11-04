# 核心實作進度

來源：任務書 v3.2 核心文檔補核版，ZIP SHA-256
`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。
舊行為基線：`329ca7c487414d2f54ddd98ff4727446ed2446cd`，舊倉只讀。
已依總書 K05 修正工作副本 CORE-011 第 2 步，不增加 unknown 重試權。

| 任務 | 狀態 | 分支／提交 | 驗證及產物 |
|---|---|---|---|
| CORE-001 | Windows 本地驗證通過；待提交及 CI | `codex/rust-core-v3-2`；本卡 SHA 由其 `manifest.source.commit`／Git 提交記錄辨識 | `artifacts/CORE-001/01/manifest.json`：17 個斷言通過；swap、empty 各自非零退出，保留失敗 manifest；`02` 為補驗 |

每卡完成後獨立提交、推送，等待 CI 通過再合併 main。空遠端首卡在
CI 通過後以同一已驗 SHA 建立 main；後續以 PR 合併。

尚無可供擴展使用的宿主／SDK 候選包；未發布，未進行正式跨倉驗收。
CORE-001 後可做 CORE-002、CORE-005，按實際交付選卡。
未要求擴展倉施工；穩定協議仍需矩陣指定的真消費者證據。
