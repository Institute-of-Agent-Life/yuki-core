# 原訊息撤回補核

CORE-013 已有 operator 原分段撤回；本補充把原 `recall_own_message(event_id)` 接入實際 Main，固定基線 329ca7c。開始前保留以下正常與故障時序。

正常：以原 canonical event 找到已確認 outbound、原發送帳號與原 locator。按目前執行主體和來源檢查讀取／效果許可，在 writer 同時保留工具 intent 與原撤回鍵，派發到原在線 bot，保存原回執。群目前改派、暫停或取消預設發送路由不能把撤回改派新 bot。

故障：他人／無權事件、非自己發送、無原生 locator、原 bot 停用／離線及來源清除均不派發。派發後中斷或不明回執保留 unknown；相同或新工具呼叫不能重送原撤回。晚回只結算原鍵。更新群路由不得擴大讀取權，撤回不重開已完成的原 Work。

驗證先跑固定舊版原用例，再由 Rust 真 Main 與認證 QQ socket 核物理 delete_msg、原 IDs、重啟與 unknown。只改核心；不替擴展實作其他平台撤回。

Reader 43 加入 `model_recalls`，把當次工具操作連到原出站 child、Presence 及原撤回鍵。接納與原撤回 claim 在同一 writer 交易完成，QQ request 使用該 claim 的原 session；連線換代不能改派到新 session。恢復時把已派發但未結算的原鍵及工具結果記為 unknown，晚回仍可按同一原鍵更新。

固定舊版五個原用例通過，成功結果逐欄對拍；Rust 另外核未知回執、重複呼叫、原 bot 離線而另一 bot 在線、來源清除及真正殺程序後的恢復。崩潰用例直接經同一 writer mutation 在 SQLite commit barrier 終止，正常派發則經實際 Main／認證 QQ socket；不把 writer crash 用例稱為整段 Main 崩潰 E2E。未知／失敗分類使用核心 `delivery_unknown`／`platform_rejected`，未宣稱與舊 Python 例外類名相同。既有工具效果外層保留，CORE-040 整體工具線路等價尚待後續裁決。
