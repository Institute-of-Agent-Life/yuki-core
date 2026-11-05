# CORE-026 原事實恢復

先設正常時序：create R1 → 讀原 fact → correct R2 → 讀 R2 → invalidate R3 → 讀已撤回版本與原證據 proof → restore R4。恢復同一 fact ID 的最後有效內容與 confidence，保留 R1～R3；新版本與原操作回執同一 writer 交易。FTS 只在恢復成功後重新收錄。恢復後更正 R5，再重放 R4，仍回原 R4，不覆寫 R5；冷啟後相同。

先設故障時序：未讀、只讀 evidence、過時 revision、同筆數但來源已更換、其他人、已失效來源、已忘記、已到期、仍有效事實都不能恢復。失敗不得寫入版本、證據或成功回執。恢復需要實際可核的原來源，不得用模型輸出或另一個人的入站替代。正確的原 restore 回執可重讀，但不授權另一個新 restore。

來源：固定 `329ca7c487414d2f54ddd98ff4727446ed2446cd` 的 `memory/mutation/service.py`、`memory/service.py::restore_fact`；原版只恢復 invalidated 且未過期項目。Rust 既有儲存狀態 `retracted` 對應撤回，恢復沿用 CORE-026 已讀 revision／proof 合同。此補修先擴至本人原事實與明示範圍管理；不宣告尚未實作的 contest、merge、reassign、update_metadata、SELF 或姓名定位。無資料庫 down migration。

`artifacts/CORE-026/memory-restore-mac-03/` 通過 226 項斷言；原 writer、QQ 命令、跨兩輪 Main、FTS、冷啟、過時 proof、部分證據寫入回滾、同群他人拒絕、到期及真實 privacy erase 均已驗。`memory-restore-adjacent-01/` 的 CORE-015／012／030／035／033／029 全部通過。原版 7 項測試與來源雜湊在 `fixtures/memory/v4-restore.json`，執行回條在 `original-memory-restore-oracle-01/`。乾淨三平台候選接續產出；開發自測包不是發布產物。

新 restore 沿用已撤回事實的目前 revision 及可讀證據 proof；`expected_fact_state=invalidated` 對應內部 retracted，錯誤狀態在授權讀取後拒絕。省略 evidence_quote 時使用真實入站文字，重新核原人物、入口及 bot 階段。Host／SDK 版本及資料庫 reader 43 不變；個人客戶端原 correct／retract API 未增加動作。
