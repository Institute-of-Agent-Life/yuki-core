# CORE-026 記憶 metadata 版本

正常時序先設：以原事件建立內容及 category／kind／importance → 讀原 fact → update_metadata 改分類／key → 再讀並更正正文 → 重放先前 update_metadata。最後正文及 metadata 保持最新版本，舊回執返回原 revision；冷啟一致。withdraw／restore 保留最後有效 metadata；FTS 分類及 key 隨同一 writer 交易更新。

故障時序先設：未讀、過時 proof、同 revision 但 metadata 已變、其他人、空分類／key、非法種類／重要度及錯誤原證據，均不寫版本或部分 metadata。忘記原來源或原人物須清除所有 metadata 版本的內容，保留原 fact／revision 操作身份。從 reader 43 升級只回填目前實際 metadata，過往未留存的版本保持未知。

本次只擴大既有本人事實的 create／correct／update_metadata；原 category、kind、importance、memory_key 均取固定舊版實作核對。日期、selector、其他主體及其他狀態仍待後續，不宣告尚未落實的欄位。metadata 更新省略 confidence 時沿核心既有更正合同保留原值。

reader 43 升級 fixture 由來源 d107b186066b887456fdfdffc76a3d462f18c7f9 的真實 binary 建立 add／correct 回執；先前 extraction metadata 投影由 fixture 明確填入。測試先載入記錄的 reader 43 rows，恢復原 triggers，再以 reader 44 migrate，逐一重放兩個舊請求並比對原 request hash／receipt。只回填目前 revision 2；revision 1 的 metadata 保持未知。接著以新 writer 寫 revision 3、冷啟及檢查外鍵。

QQ 命令測試依原 effect payload 收齊所有真實分段，ACK 各段後逐字比對，防止列表增長後只檢查首段。候選包綁定舊版 metadata oracle、reader 43 fixture 及本次驗證結果的 SHA-256。

升級時 opaque read proof 增綁目前 metadata／key；尚未執行的舊讀取需重新 read，已提交的 add／correct 保留舊序列化形狀與原 request hash。這項 reader 44 變更沒有改 Host 2、handshake 1 或 SDK alpha.11／a11，新增 metadata 僅沿既有 JSON fact 結果提供。

相鄰 CORE-035 首輪捕獲既有 maintenance.fact.page 的嚴格結果 schema 拒絕新增 metadata。維護入口現在按已安裝合同保留原投影，opaque proof 仍綁完整事實；新增 metadata 不擅自擴張 Host 欄位或放寬 schema。失敗回條保留於 memory-metadata-adjacent-01/CORE-035。
