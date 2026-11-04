# QQ 入帳鍵裁決

`python baseline/qq-key/decision.py <capture.json>` 讀取離線輸入並輸出 JSON。
`fixtures/qq-dual/deployment.json` 記錄目前可用的部署證據。
Windows 的 QQ 客戶端已開啟，尚無目標 SnowLuma 映像摘要或同群雙端原件。
CORE-009 在這個狀態只允許配置的一個接收端入帳。該端斷線可能漏收。

部署者可提供 `kind=target_capture`、目標及採集時的相同映像 SHA-256、
候選群序號欄名與 observations。每筆指定獨立標注的 occurrence、連線 session、
本地去識別原封包 file 與 SHA-256。重新連線重送應保留同一原 occurrence，
同秒同文的第二次發送則另編 occurrence。每次按一組群及 topic 裁決，再換其他群重跑。
每個 Bot 的 message_id 只作該 Bot 的 locator。入帳裁決本身不選發送路由。

`rehearsal.json` 的兩個合成 Bot 用於驗證判別器，固定涵蓋反向到達、
指定 @B、回聲、同秒同文兩條與重連。其序號欄名僅用於演練。
真正封包原件留在授權本地。換映像後需重新採集；私聊不使用群序號。
