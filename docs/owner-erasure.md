# 原人物與來源清除

`operator privacy forget REQUEST_ID PERSON_ID` 解析原 Person 同人族、歷史 private entry 及來源。每個來源沿既有水位失效，連帶清掉已讀入其他 Work 的 fact、原請求、Checkpoint、工具原件、FTS、向量及媒體引用。重複 REQUEST_ID 讀原清單。新帳號階段的另一位原 Person 維持自己的資料。管理端沿 Client 9 的 `POST /admin/v1/persons/{id}/forget`，須由有效 chat 設備及當前超管角色授權。

`operator privacy forget-status REQUEST_ID` 分列邏輯撤權、待回收 managed blob、仍被其他合法版本引用的共享 blob、待刪 transfer 路徑及待回條 owner。writer 先提交撤權，`operator media gc` 或背景 worker 再處理自管檔案；冷啟動可以接續。已完成的效果、來源 ID、防重放鍵及數字 usage 保留，正文不可復活。

## 插件冷啟動

曾持有來源的插件即使停用或移出配置，仍保留原清除義務。重啟前有待辦時，Host 只啟動配置中明列的 `erasure_args`。它使用同一 `program`，帶入 `YUKI_PLUGIN_START_MODE=erasure_only`；握手只協商 `forget.request/ack`，原世代拒絕所有業務方法。原水位得到正確 ACK 後，Host 停止清除程序，再以一般 `args` 啟動。未配置清除入口時，該插件維持停用並報明原因。

```toml
[[plugins]]
id = "retained-owner"
namespace = "retained-owner"
program = "/absolute/path/to/plugin"
args = ["serve"]
erasure_args = ["erase-only"]
grants = ["forget.owner", "tool.call"]
```

清除入口先完成原世代的資料清除與回條，再退出；不得在這個模式啟動自己的背景服務或 webhook。Host 對正在運行但尚有清除義務的實例，也先交付清除請求。測試以獨立 SDK 程序重現刪檔後、ACK 前的崩潰，下一次只接續同一水位。

## 已驗邊界

CORE-030 對拍原版的七項清除測試，並用實際 Main HTTP、兩個人物族、歷史帳號轉交、兩份共享內容版本及冷啟動 owner 驗證。附件、ASR、public/private Compact、提取、Embedding 與 Host 的遲到回應，另由 CORE-014、022、023、025、027、028、032、052 的具名回條核對。進度檔列出本次原件位置。

自管引用和檔案回收具有可核對的狀態。外部已送消息、供應商保存、操作者原始匯入文件及自行複製的文件由各自 owner 管理。備份恢復先套後續清除水位。SQLite 可重用頁與磁碟物理磁區不列作安全抹除證據。
