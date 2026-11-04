# OneBot 接收與路由

先在私有 operator 建立 QQ 的 SELF 階段，將它的 assignment ID 填入設定。
每個接收 Bot 使用自己的密鑰環境變數。未配置 QQ 時可照常啟動核心。

```toml
[qq]
namespace = "office-qq"
designated_ingress = "10001"

[qq.groups]
"20001" = "mention"

[[qq.bots]]
self_id = "10001"
stage = "<active SELF assignment>"
token_env = "YUKI_QQ_BOT_10001_KEY"
typing_private = true
```

反向 Universal peer 連 `/onebot/<self_id>`。URL、X-Self-ID、每 Bot 的
Bearer 憑據及後續封包的 self_id 必須相符。請使用隔離本機或受保護的
TLS 通道。每個 Bot 的新連線取代自己的舊連線。群政策有 open、mention
及 observe，未列入的群不採集。兩次同秒同文的發送仍是兩個來源。

預設群入帳交指定 Bot，原生提及 B 仍以 B 為回覆目標。B 離線時會
顯示 offline；多個被提及的 Bot 則保留未決目標。私聊使用接收 Bot 的
SELF 階段。typing 僅於已配置的私聊能力生效，同一入口的重疊工作共享
提示，最後一個持有者退出後排入 stop。API echo 只配原連線的動作。

經 CORE-004 驗證的新採集可將 `qq.shared_decision` 指向資料目錄下的
裁決 JSON，另指定該檔 SHA-256 及目標映像摘要。只有裁決見證的群、
topic、欄及接收者可以共享鍵，所有其他群仍走指定接收端。新映像需重取證據。
本倉的 WS peer 是合成測試環境，真實 Windows QQ 部署仍缺雙端封包。
既有資料切換鍵策略需由部署 owner 核對舊收件水位及未完成重投。

協議頭及回應 echo 依 OneBot 11。 [反向 WebSocket](https://github.com/botuniverse/onebot-11/blob/master/communication/ws-reverse.md) · [鑑權](https://github.com/botuniverse/onebot-11/blob/master/communication/authorization.md) · [echo](https://github.com/botuniverse/onebot-11/blob/master/communication/ws.md)
