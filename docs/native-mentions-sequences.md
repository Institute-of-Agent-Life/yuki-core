# 原帳號提及的派發合同

正常時序先從接收路由的原平台與 namespace 查原 account，依原事件的名稱觀測解析文字。完整 account token 使用核心 account ID，同名依原會話最近發話與 account ID 排定，未知名稱保留文字。URL、email 與跳脫字元依固定舊版規則處理。原操作 payload 保存原輸入，派發副本另保存渲染後文字、UTF-8 byte 範圍、native ID 與原觀測來源。

Host 以原 call handle 供應文字與提及的 bounded pages。插件計畫須完整、連續覆蓋原文字，分段邊界避開整個提及。每次物理派發只取得該段的原提及，原 account、階段與來源在派發前及分頁時再核對。分頁讀取登記來源持有者，來源遺忘可取消插件中的讀取。

故障時序包含跨 app／namespace 的 account token、未觀測姓名、同名歧義、分段截斷提及、錯 digest、遺漏或重疊區間、替換 native ID、來源遺忘、階段終止、外掛重啟與晚到頁。每一種故障都驗實際 wire 與原操作回執。原生投遞失回應保留 unknown，冷啟查回原逐步結果。

消費者為擴展倉的飛書與 Lark，逐原域產生 native at 標記並保留原 topic／引用及每段回執。SDK 與 Host 先交未發布候選，完整穩定合同等待真消費者重放。
