# CORE-033：Client9 明選原入口與會話

EXT-015 已在 Windows 重現：A 與 B 合併後，以 A 的 token 明選 B 的 binding，舊核心忽略 header 並回 A。固定 v4 `client_api/identity.py` 區分裝置的 `source_person_id`、目前 `person_id` 與所選 binding 的 `request_person_id`。App 不帶 `request_conversation_id`，其 request_scope 是所選原 Person；ACP 才按原 conversation 分隔。

正常時序：A 與 B 各有已配對裝置及原 App binding，合併後 A 明選 B，以同一讀快照或寫交易驗裝置、人物、binding／presence，再按 B 的原入口讀取、發訊及查回執。未指定時保持原裝置入口。明選既有 ACP 私聊時保留真 conversation、thread、presence；同 binding 兩條 thread 使用同 UUID 不互相重放。

故障時序：合併前跨人物明選、陌生 binding、非 ACP conversation、同時給出不匹配的兩個 header、重複 header、已撤或停用人物、已關閉／暫停入口寫入，均不得回退到裝置默認入口。拒絕前後逐項核事件、回執、模型請求。已付操作的重放與讀回仍須目前可見性；ACP 既存會話可讀與能否再發送分開驗。舊 NULL 歸屬依原裝置判定，不從合併後 root 猜回原操作。

本卡先核選擇與作用域。附件、agent_target 正反例、SSE 及真 EXT-015 消費各留獨立證據；任何尚無執行回條的項目不計入這份候選。

Host 的 Work proof、session 及原 native operation 保存所選 entry；reader 46 的 NULL 沿用發起裝置的原入口。每次兌換或寫入再次檢查該 binding。對同一個 QQ Work，A 的預設入口、A 明選 B 的 App、既有 ACP thread 可以重用相同 UUID，各自產出一份 mail；同一作用域經 Host 或 HTTP 重放只取原回執。原 QQ Work 的入口、manager、budget 不因私下補充而改變。每份候選的 `evidence/client-entry-selection.json` 含測試裝置的原／目前／請求人物、所選 binding、conversation、event 與拒絕前後計數。
