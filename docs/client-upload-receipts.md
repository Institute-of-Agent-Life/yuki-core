# CORE-033：原 Client9 上傳回執與有序附件

EXT-015 的明確需求是：同號請求若改變有序附件必須衝突，合法附件能進入原會話。EXT-016 的插件、UI、對外部署及消費回條仍屬擴展倉；核心提供可信裝置、原請求、媒體與事件事實。

正常：讀取 bytes 前先核來源裝置、所選 scope、原 UUID 與規格；已完成的同號重送回原上傳回執。新上傳在 writer 外接收及驗 SHA-256 和長度，提交時重核當前裝置／binding，內容與回執一起成功。收到附件不建立 Main，使用它發出訊息才在原選定 entry 建立事件、原 slot 及 Main 輸入。相同內容可共用存放，知道 digest 或 media_id 並不授權。

故障：中斷、長度或內容不符、檔名越界、傳輸中撤權、同 UUID 改參數、換消息類型、外人猜測、到期後新使用、交換附件次序、冷啟後重放、forget。失敗不留可用的回執或半個事件。只有上傳到期時，同一原作用域仍可讀舊回條；清退人物另按原授權拒絕。原版本回放已在隔離副本跑過 15 個 Client／SDK 測試。

Mac 完整回執：artifacts/CORE-033/client-uploads-mac-02/manifest.json（666 項）；17,825,808 bytes 的下載 SHA-256 為 43ae47e690985a872484683c84df34de157638d2bac10b4e21f17f2e9b45adc1。六個故障邊界：同號改規格、傳輸中撤權、回執提交失敗、儲存內容被改、來源到期及忘記人物。實際背景清理刪除沒有其他引用的內容；另一人物仍能讀取自己登記的同一份 bytes。

固定舊版回放：artifacts/CORE-033/original-client-uploads-oracle-01/；相鄰媒體、人物清除、記憶遷移、Main、Social、管理和 Host 回執在 artifacts/CORE-033/client-uploads-adjacent-01/。統一清理引用視圖後另跑了 CORE-025、030，見 artifacts/CORE-033/client-uploads-gc-02/。
