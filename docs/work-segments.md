# 原 Work 分段續跑

正常時序從原 activation 保存單段模型上限，接納的邏輯模型請求仍向原根預留一次。完整回應及整批工具收妥後才檢查段額。已接納 Work 到額後保存 queued 與下一次可執行時刻，原執行、request、效果及 mailbox 水位保持可追溯。下一段沿同一原執行續接已配對結果，根額度累計。完成提案、need_input、wait 和未知 paid attempt 先沿原出口結算。

故障時序覆蓋最後一筆工具配對後終止、queued commit 後終止、冷啟換單段設定、原根用盡與取消後冷啟。每次核對原 request/operation ID、實際 HTTP 與外發筆數、原 Work 及根預留。已付未知仍 held。合法 final 與完成批尾先結算，沒有 Work 的無限工具輪則以單段額度用盡保留診斷。

部署設定 `[work_segments]` 的 `model_requests` 預設 24，`resume_delay_ms` 預設 2000。此為排程配置。已開段保存當時上限，下段才採新值。原 Work 的模型與工具合同保持原快照。單段計數包含屬於本執行的 Main 及輔助邏輯請求，查原結果和重配對不增加次數。舊庫首次觀察從當時已存在的 request 水位接續。
