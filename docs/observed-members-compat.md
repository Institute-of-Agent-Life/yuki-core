# 原群成員目錄補核

CORE-040 原工具集合仍缺 get_group_members；此工作屬 CORE-013／014 的核心讀取接線，不修改擴展倉。行為基線為 329ca7c 的 SocialService._group_directory。Rust 接線及 Mac 正常／故障驗證已通過，跨平台候選另列進度。

正常時序：按原執行者與會話取得讀取範圍，選定實際已觀察的群 binding，在單一快照按 account UUID 翻頁；同人多帳號保留不同項目，回 coverage=observed、原 account reference、目前有效 assignment，不買模型、不查平台完整名冊。群發送暫停或 bot 離線不能把歷史讀權替換成發送條件。

故障時序：模糊群／binding 不擅選，非法游標、另一私聊或沒有讀權的入口拒絕；人員合併及階段交接不借用舊帳號權限；來源清除及授權在讀取後改變時，writer 重新查原範圍，不發布晚返目錄。SELF 只能使用其明確授權群，不擴成全部入口。

先取得固定舊版的真輸出與邊界測試，再接 Rust 模型原 schema、逐項回執及具名測試 artifact。完成後獨立提交與三平台 CI。

實作從目前授權入口的非刪除原事件與原提及推導目錄。清除來源後不沿用快取；unknown mention 只建立 Account，沒有合成人物或階段。顯示名稱按原 Unicode 清理規則，較晚到達的較舊事件不覆寫新卡片，明確空卡片會清除舊卡片。名稱來源也綁入原 execution 的依賴。既有資料沒有完整原 AccountSpaceProfile 更新紀錄；此接線保證授權範圍內的可證觀察，不宣稱重建每一筆舊版 profile 更新歷史。

`observed-members.json` 包含實際 Main wire、分頁、同人多帳號、目前階段、離線／暫停、未知 mention、非法游標、外群拒絕、實際依賴及遺忘後重啟回條。`baseline/replay/observed_members.py` 在固定 SHA 的隔離舊版重跑三個原用例，保留原宣告與顯示名稱樣本。沒有查完整平台名冊或借用發送權限。
