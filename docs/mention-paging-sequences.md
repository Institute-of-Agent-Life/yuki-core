# 大量原帳號提及分頁

Windows 的真飛書消費者在 11,000 個提及、1,977,223 bytes 原註記的計畫讀取中逾時。原件在 `artifacts/CORE-032/mention-paging-windows-before/`，回條保留首次完整擴展快照與 Windows ZIP 雜湊。

正常時序由計畫接納驗證完整註記並登記原來源依賴。每頁以 SQLite JSONB 解析一次原呼叫，再取回原 JSON 陣列 bytes，只向授權檢查交付相異的原帳號、原平台 ID、原 assignment 和原事件。每頁再次核對當前安裝、grant、generation、execution、引用、路由及各原身份。原件摘要沿用目前 Session 中該 handle 已接納的出站 descriptor，Session 僅保存摘要。缺少 descriptor 時由原資料計算摘要。每頁回覆攜帶完整原件摘要與原 byte 範圍，實際 SDK 消費完所有頁後才交分段計畫。

故障時序沿原回歸覆蓋換 grant、revision、generation、原 assignment 結束及來源擦除。呼叫接納已將提及來源登記為 execution dependency，分頁依既有依賴清單取得持有者責任。忘記後原執行與 Host 材料同步失效，晚到讀取得到拒絕。原 12 秒呼叫期限及每頁 12,288 bytes 上限繼續作為驗收邊界。

`CORE-032` 增加 11,000 個密集提及的正常入口案例。SQLite 回歸另核對原 JSON 陣列的字元、跳脫、次序與 bytes，包含 11,000 筆註記。修正須通過原帳號授權與清除回歸，再以同一真飛書情境完成 HJW 及三端重放。
