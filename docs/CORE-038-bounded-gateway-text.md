# CORE-038 原長文字的有界傳輸

飛書實際候選在 `gateway.plan_send` 前暴露 64 KiB 單幀限制。核心 #114 的三端候選保持該上限，飛書依 v4 的 150,000 bytes 原生文字上限切段。原測試 `rest-delivery-preflight-02/feishu-parts` 已接納入站並取得兩次模型回應，沒有派發任何原生文字。小文字的私聊、群與話題已通過真 Host。

正常時序先以原 plan handle 分頁讀取原文字，核完整摘要後，計畫回傳連續 UTF-8 byte ranges。每個 range 綁原 parent、原完整文字及片段摘要。Host 保存有界計畫，派發每段時只允許以該段 handle 讀取該段的頁面。平台每個物理發送仍有獨立 operation、locator 與結果。

新增開發方法 `gateway.read_text` 使用 `gateway.send` 授權，頁面最多 12 KiB。文字保留於原不可變 `host_calls.args`。分頁讀取核原安裝、revision、generation、dispatched handle、execution 與隱私來源。需要保留來源時沿用既有 owner／forget 合同。所有單幀保持 64 KiB 內。

故障驗收包含錯 handle、舊 generation、缺權、越界頁、非 UTF-8 邊界、錯摘要、重疊／缺口／漏尾的計畫，以及失去第二段回應後第三段零派發。來源刪除後的頁面及後續派發拒絕，冷啟只讀原效果。舊 inline 文字入口保留具名回歸。

交付仍屬未發布開發合同。兩套 SDK 隨單一 IDL 產出，核心測試與飛書真消費回條共同綁定後再進穩定協議見證。
