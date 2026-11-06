# 模型批次逐項拒絕

CORE-015 H12：正常時序為保存原完整回應、按原 call_id／順序建立效果、逐項驗證原 paid schema／choice／工具可見性，再派發可執行項及配對全部回條。參數錯誤、未知／未聲明工具或錯誤 choice 都有失敗回條；不購買該業務效果、不讓查詢授權其他工具。讀寫屏障及原呼叫順序保留。

故障時序：同批合法寫入、無效參數、未知工具、合法讀取與送達需各自結算；錯誤項保持零派發、零業務扣額，冷啟動不重送。Chat、三種 Responses、Messages、Gemini 使用原封不動的供應商回應與原 ID；不能配對的重複／缺失 ID、截斷或未知結構仍 held，不猜呼叫身份。來源於 writer 前撤權時仍由原共同授權拒絕整個執行。

先用實際 Main loopback 重現目前整批 held，再核逐項效果、原序回條、寫後讀、唯一外部送達、原回應保存、冷啟動與重複 ID 故障。Mac 六協議驗證已通過，三平台乾淨候選另列進度。

普通模型呼叫使用三種拒絕分類：tool_not_declared、invalid_tool_arguments、tool_choice_mismatch。分類由可信 decoder 產生，不讀模型參數中的同名欄位。writer 在原 response 交易中建立 failed 操作與配對鍵，派發器看見終態後不扣額、不執行；原錯誤參數仍可從不可變 provider 回應核查。結構化 emit_result 依自己的 paid 宣告驗證，不套用普通工具 choice；截斷、重複 ID 與無法配對的 provider 結構保留 held。沒有修改 unknown 請求的重試權。

Mac `CORE-015/call-isolation-mac-04/` 363 項通過；`call-isolation-adjacent-02/CORE-019` 及 `call-isolation-adjacent-03/{CORE-020,CORE-021,CORE-012,CORE-011,CORE-010,CORE-030,CORE-034}` 全部通過。原版四個未聲明工具／續跑用例在 `original-call-rejections-oracle-01/` 通過；它們不代替 Rust 六協議實測。Host 2、reader 43 及 SDK alpha.11／a11 不變；新 rejected 欄位僅用於內部接收的 intent 記錄，既有 failed 效果及原 reader 仍可讀。
