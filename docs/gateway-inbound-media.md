# CORE-038 / CORE-025：取回原 gateway 收件附件

EXT-004 和 EXT-006 需要在本來的 Bot 或 app／tenant 取回 Telegram file_id 或飛書 resource key。宿主只在實際授權的執行選中 event、attachment_index 後向原安裝取件；原生下載、憑證與平台限額屬擴展。現有出站 media.open 不變。

正常時序：管理者核准原安裝的 gateway.inbound 與 gateway.media，明定 application 與 tenant（單 Bot 可不設 tenant）→ gateway.inbound 隨原訊息標明同一組非機密身份 → 同一 writer 提交原安裝 revision、namespace、Bot stage、來源 event 和原平台 message → 讀者選擇 event/slot，核心取出原 segment/reference → 原插件按順序回傳同一長度、SHA-256 的分塊 → 最後一次複核原執行、安裝、裝置階段與來源 → 寫入既有 media_sources。協商的 callback 為 gateway.media；核心不接受 URL、宿主檔案路徑或候選安裝作為下載指令。

故障時序：沒有授權或舊收件沒有來源綁定時，不向其他插件、QQ 或 HTTP 回退。相同 key 在其他 namespace 仍是另一來源。換 application／tenant、換安裝 revision、插件重啟、錯 event/slot、到期或 forget 在讀取前拒絕；讀取途中停用，不再送下一塊，也不快取收到的部分內容。錯 offset、長度、SHA 或超限都沒有可讀的半份檔案；已快取但到期的原件不重抓。驗收以真實 stdio peer 計實際 callback 次數，獨立擴展回條另列。

來源與證據：原版固定 SHA 329ca7c487414d2f54ddd98ff4727446ed2446cd，18 個 Telegram／飛書行為用例，artifacts/CORE-038/original-gateway-media-oracle-01/。新核心的 207 項檢查和分塊回條在 artifacts/CORE-038/inbound-gateway-mac-02/；相鄰 CORE-031／032／039／025／030／026／033 在 artifacts/CORE-038/inbound-gateway-adjacent-01/。獨立消費與發布待交接。
