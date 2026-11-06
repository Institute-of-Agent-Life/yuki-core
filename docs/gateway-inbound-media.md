# CORE-038 / CORE-025：取回原 gateway 收件附件

EXT-004 和 EXT-006 需要在本來的 Bot 或 app／tenant 取回 Telegram file_id 或飛書 resource key。宿主只在實際授權的執行選中 event、attachment_index 後向原安裝取件；原生下載、憑證與平台限額屬擴展。現有出站 media.open 不變。

正常時序：管理者核准原安裝的 gateway.inbound 與 gateway.media，明定 application 與 tenant（單 Bot 可不設 tenant）→ gateway.inbound 隨原訊息標明同一組非機密身份 → 同一 writer 提交原安裝 revision、namespace、Bot stage、來源 event 和原平台 message → 讀者選擇 event/slot，核心取出原 segment/reference → 原插件按順序回傳同一長度、SHA-256 的分塊 → 最後一次複核原執行、安裝、裝置階段與來源 → 寫入既有 media_sources。協商的 callback 為 gateway.media；核心不接受 URL、宿主檔案路徑或候選安裝作為下載指令。

故障時序：沒有授權或舊收件沒有來源綁定時，不向其他插件、QQ 或 HTTP 回退。相同 key 在其他 namespace 仍是另一來源。換 application／tenant、換安裝 revision、插件重啟、錯 event/slot、到期或 forget 在讀取前拒絕；讀取途中停用，不再送下一塊，也不快取收到的部分內容。錯 offset、長度、SHA 或超限都沒有可讀的半份檔案；已快取但到期的原件不重抓。驗收以真實 stdio peer 計實際 callback 次數，獨立擴展回條另列。

來源與證據：原版固定 SHA 329ca7c487414d2f54ddd98ff4727446ed2446cd，18 個 Telegram／飛書行為用例，artifacts/CORE-038/original-gateway-media-oracle-01/。新核心的 207 項檢查和分塊回條在 artifacts/CORE-038/inbound-gateway-mac-02/；相鄰 CORE-031／032／039／025／030／026／033 在 artifacts/CORE-038/inbound-gateway-adjacent-01/。

## Telegram 原生插件的三端消費記錄

在 `[[plugins]]` 核准 `gateway.inbound`、`gateway.media` 後，將同一安裝的 `[plugins.gateway_media]` 中的 `application` 設為 Bot 的 ID 字串，例如 `application = "101"`。測試使用真正的 Telegram 0.1.0-alpha.2 插件、原生核心與本機 Bot API 錄製端。Mac、Linux、HJW Windows 各自從入站 ACK 取得原 event，選取同一附件，經兩個 callback 取回 29,367 bytes。原 Bot 各收到一次 getFile 與一次檔案下載，第二次選取沿用已驗快取。將授權改為另一個 Bot ID 後，Host 回覆 -32003，沒有媒體 callback、原生下載或 offset 推進。

核心來源 `4a41470e89b4`，插件來源 `5ad17ab4507b`，插件使用的 SDK 為 Rust 1.0.0-alpha.13。三份原生執行檔及逐幀記錄的雜湊均可從 `artifacts/CORE-038/telegram-5ad17ab-three-platform-01/consumer-receipts.json` 核對。出站逐段和穩定協議由各自的見證卡驗收。

當安裝只授權收件與媒體時，Main 仍可呼叫其他已安裝的工具。若 Main 隨後要求這個安裝發送，Host 應在派發前記下本次拒絕，向原模型交還失敗回執。故障重放查核確認過的網頁回條仍在、發送計畫與原生發送均為零，並再次讀取同一筆拒絕回執。

有發送權的網關收到未支援的媒體請求時，Main 可從原操作讀到 `gateway_media_unsupported`。選定附件另需安裝具備 `media.open`，缺少授權回傳 `gateway_media_read_not_granted`。版本不存在或原執行無權讀取都回傳 `gateway_artifact_unavailable`；可讀版本的類型與請求不同時回傳 `gateway_attachment_kind_mismatch`。這四種拒絕均發生在規劃與派發前。

Telegram alpha.3 與 web-fetch 同時安裝時，三端 Host 均讀回固定網頁，再經原 Bot 發送三段文字。第二段遺失原生回應的案例保留 unknown，重啟後沒有新的平台呼叫 sendMessage。逐段定位、內容及二進位檔雜湊在 `artifacts/CORE-038/paired-consumers-pr97/consumer-receipts.json`。
