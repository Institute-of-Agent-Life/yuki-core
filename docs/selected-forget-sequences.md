# Selected Host 的清除回執

只選入 `forget.request` 的 Host 會收到插件對原請求的結果；該結果不需要另一個 `forget.ack` 回呼。正常時序先按已選方法驗證請求與結果 schema，再核對原 key 和 watermark。Host 將 key、watermark、proof 交給內部 `ForgetResult` 變更，在同一 Store 交易核對當前插件、generation、online、enabled 和 `forget.owner`，再沿用 privacy Ack 儲存原 obligation 的 proof。清除程序結束後才啟動業務程序。

錯誤 proof 型別或不同 watermark 都保留原 obligation，業務程序不啟動。程序在刪除後、回覆前退出時，冷啟動重送同一 key 和 watermark；已刪除的檔案不會重建，原 proof 入庫後才放行業務。已確認的 obligation 在再次冷啟動時不重送。獨立 `forget.ack` 仍按 wire 方法表處理，未選入時繼續拒絕。

`ci/native_surface_check.py` 同時選入 `gateway.activity` 和 `forget.request`。該命令指定 `--selected-cleanup`，用未選入的 `observe.metadata` 做授權對照。`tests/harness/selected_cleanup.py` 以真 Host、安裝後 Python SDK、獨立資料目錄驗證正常、錯誤 proof、錯誤 watermark 和丟失回覆四種時序。原只選 activity 的檢查入口仍使用 `forget.owner` 作未選授權對照。

本地證據位於 `artifacts/CORE-053/forget-response-before-02/`、`forget-response-after/` 和 `forget-response-selected/admission-04/`。舊 Host 收到刪除證明後，obligation 仍 pending、proof 為 null。修復版將同一 proof 入庫，清空待處理項目。Mac 預驗通過選面 200 項及原 activity-only 42 項。獨立審查另驗證管理者停用後的回覆及錯請求 ID 皆保留待處理項目，回條在 `forget-response-selected/independent-review/verdict.json`。
