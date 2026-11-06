# CORE-033 Connector 未選 Work 的認證

消費者是 EXT-011，具體缺口見擴展倉 `docs/core-interface-requests.md` 的 CORE-033 → EXT-011：原 `/agents/v1/connect` 在 WebSocket 升級時只有 Connector Bearer，尚無 Work。核心負責認證原裝置並提供窄用途憑證；擴展保有 wire4、當前連線、instance／catalog 與 native journal，不把設備實例搬進核心資料庫。

正常時序先設：已配對 Connector 透過核心私有認證入口，以真 Bearer 和明選安裝換短效一次性 proof；已批准的同一插件程序兌換連線 handle，得到原 device ID、穩定 journal namespace 與連線代次。操作前重新核有效裝置、原 Person／目前 root、插件 grant／generation 與目前連線代次。重新連線不換原 device ID 或原操作命名域，不建立 chat entry、Work 或模型請求。

故障時序先設：chat token、偽造 hello device_id、錯插件／重啟後 generation、重用或過期 proof、被撤設備、停用原人物／root、帳號換人、A→B 合併與舊連線晚到的 catalog／close。舊連線不能覆蓋或關閉新連線。Connector 認證不授予 Work.input、人物管理或一般 chat；原 native Work 回流繼續使用既有 Work scope。

原身份語義以固定 `329ca7c487414d2f54ddd98ff4727446ed2446cd` 的 `identity/credentials.py`、`external_agents/identity.py` 及 `test_connector_identity.py` 對拍。現有 Connector 不因 chat 入口暫停或原帳號換人而移轉給另一人；原憑據仍屬於原 Person，撤設備及停用人物會拒絕。秘密不進 ledger、trace、普通回條或候選證據。

本補核是未發布的生產者合同。候選須帶 schema／SDK／宿主來源和三平台回條，交 EXT-011 再作真實消費驗證；此次不修改擴展倉。

## 候選呼叫合同

私有入口 `POST /core/v1/entry/exchange` 接受真 Connector Bearer 與 `{"purpose":"connector","plugin":"<approved installation>"}`，省略 `work`。回傳 `data.proof`、`scope=connector` 及 `expires_at`；proof 有效 60 秒、一次性，綁定目前安裝與程序 generation。若明選 `work`，仍走既有 Work scope；兩種 proof 不能混用。chat token 無法換 Connector proof。

同一安裝須獲 `device.connections` grant，透過 Host 2／personal 1 的新可選方法呼叫：

| 方法 | 參數 | 成功結果 |
|---|---|---|
| `device.connect` | `proof` | 原 `device_id`、`purpose=connector`、`connection_epoch`、`journal_namespace`、`expires_at`、`closed=false` 與秘密 `handle` |
| `device.check` | `handle` | 同一公開投影，不含 handle；重核當前授權，已關閉或舊 epoch 拒絕 |
| `device.close` | `handle` | 同一投影且 `closed=true`；同一有效當前連線重放相同結果 |

handle 有效 300 秒。消費者到期前重新交換 proof 並 `device.connect`，以新 epoch 取代舊連線；重啟也必須重新認證。namespace 由可信 installation ID 與原 device ID 決定，帳號換人、人物合併或重連不改原 namespace，epoch 永不回退。raw Bearer 不送插件；插件只短暫持有 proof／handle，資料庫只存雜湊。

擴展在接受 catalog、派發／接收原 native 操作及處理異步回調前，用當前 handle 重新 check 並核本地綁定的 epoch。一次 check 只證明該時點的裝置授權，未授予未來任意業務效果；Work 回流仍以既有 Work handle／原操作鍵及 native ack 合同核驗。舊連線晚到的資料由擴展拒絕，核心不保存 instance／catalog。關閉已到期或舊 epoch 回拒絕，不能拿它關閉新連線。

新增可選方法由 Rust SDK `1.0.0-alpha.12`、Python SDK `1.0.0a12` 分發；Host 2／handshake 1 保持相容，reader 46 僅新增授權表。原版 oracle 為八項實際身份測試；CORE-033 以獨立安裝 SDK、真 HTTP 憑據及三個子程序覆蓋上述正常／故障時序。正式 EXT-011 消費證據仍待擴展用此候選取得。

首次乾淨來源 `3e4080d` 在 HJW 完成 369 項斷言，但打包後清理測試執行檔遇到 Windows sharing violation，沒有產出 Windows ZIP。修正保留系統回報的原始錯誤，只在清理時限內重試 WinError 32。HJW 持有真實不可刪除的檔案控制代碼，驗證釋放後可清理；持續鎖定和其他權限錯誤仍失敗。證據：`artifacts/CORE-039/portable-cleanup-windows-lock-01/file-lock.json` 及 `portable-cleanup-mac-01/manifest.json`。
