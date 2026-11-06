# CORE-033：Client9 的提交提示與斷線續讀

正常時序：GET /client/v1/stream 先訂閱提交提示，再用同一讀交易核裝置、明選入口與人物，回第一個 sync。Social Outbound 只有在已提交且目前可見時才提高該會話的 latest_event_id；客戶端用 GET events?after=原事件號 補讀正文。SSE 的 id 是單次連線內的序號，重連從新的 sync 開始。

故障時序：別人的私聊、尚未提交／已回滾的結果及已撤設備不得流出。斷線期間收到的出站回覆可從新快照及持久事件頁恢復；廣播落後時結束連線，不能假定漏掉的提示仍完整。activity 只報目前進程真正進入執行的 turn，不從過去的資料庫狀態推測仍在線。本次只用隔離 HTTP、模型與 QQ peer；真 EXT-015 消費另記。

Rust owner 在訂閱後按選定 binding／會話重核設備。慢訂閱者若錯過提交，即使仍有尚未送出的 frame 也結束；另一條沒有落後的連線繼續。關閉 owner 時會先通知 SSE 退出，讓 HTTP 正常排空。對使用中的 Work 回傳原 revision 與工具步驟 token；活動來源只含 origin、kind 與開始時間。綁定原 superuser 才可見 SOUL 的 revision／active_revision，沒有本文。沒有已連接的 Computer provider 時省略該可選欄位。

驗證：固定原版五項測試取自 `tests/unit/test_client_views.py`，含訂閱與提交間隙、遺失最後提示後重連、實際 live span 與撤設備。原始回條在 `artifacts/CORE-033/original-client-stream-oracle-01/`。Rust 的 `client-stream-adjacent-02/CORE-033/client-outbound-stream.json` 記錄真測試 QQ peer 回應前後的持久游標、重新連線序號及投遞數；同目錄的 CORE-012、013、016、037、032、030 全通過。Rust 單測令兩個訂閱者接收同一批提交、只有一個落後，驗證慢讀者不能排出舊私有 frame。首次相鄰檢查因測試模組位置未通過 Clippy，移至檔尾後重驗。
