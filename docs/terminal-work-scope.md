# CORE-032：原 Work 的終端 run 授權

EXT-020 的 Manager 以 request_id 接納命令，回傳自己的 run_id。原版從工具呼叫的來源注入 YUKI_WORK_ID。擴展擁有 Manager、execd、PTY 及 stdout；核心只認原呼叫、Work、執行與安裝的對應。

正常：管理者為終端安裝授予 terminal.work 及 tool.call。Main 已接受 Work，然後明選 terminal_exec。寫入操作時，核心將 Host handle 固定至該次執行的 work_runs、原安裝部署與版本。插件只持該 handle 調 terminal.scope，從宿主取得 Work 和 request_id，才向 Manager 發起請求。收到實際 run_id 後，以 terminal.bind 提交該請求回條；同鍵同 run 冪等。terminal_read/control 從自己的新呼叫取得 scope，只有該 Work 原安裝已綁定的 run 能通過。原安裝的後續執行若仍在同一 Work，可續讀；取消一次觀察不會取消 Manager 的 run。

故障：沒有正式 Work、無關工具、偽造 Work 或裝置、跨 Work／安裝猜 run、來源被忘記、Work 已換、部署更換或權限已撤，均不得取得調用權。已派發而失去連線的 exec 保留 unknown；同一部署重啟後原 handle 只提供 reconcile（以原 request_id 查回），絕不重新授權實體啟動。只在原註冊上記下回條，不用它把 unknown 自動變成 confirmed。

## 普通輪次的基線

上面的 `terminal.work` 描述已接納 Work 的路徑。固定 v4 的 SandboxClient 也接納有原會話及原入站事件的普通輪次。實跑中，客戶端把沒有 `YUKI_WORK_ID` 的 terminal_exec 交給原 Manager；Manager 登記一筆 job，再按原 request_id 查到同一個 run_id。缺少來源、或來源只有會話而缺事件／父執行時，Manager 都沒有收到請求。

回條和隔離重放程式在 `artifacts/CORE-040/original-terminal-source-03/`。

預覽 SDK alpha.38／a38 增加 `terminal.context`。管理者在同一終端插件安裝的 `terminal_entries` 指定入口到 Person 的對應，例如 `terminal_entries = { "<entry-id>" = "<person-id>" }`。普通輪次只有原人、原事件、原入口和接收 Bot 都能核對時才開放。新方法的普通輪次回 `entry_id` 並令 `work_id` 為 null。Work 輪次回 `work_id` 並令 `entry_id` 為 null。既有 `terminal.scope` 仍用原 Work 形狀。兩條路徑使用同一 `terminal.bind` 將原請求綁到 Manager run。

同一安裝、入口、Person 和接收 Bot 的後續真人輪次可讀或控制原 run。其他入口、人物、安裝或已清除的原來源拒絕派發。reader 64 新增普通輪次的綁定，既有 Work 回條仍從原表讀。CORE-032 的 `terminal-entry-scope.json` 驗證 Host 派發、續讀、拒絕與原來源清除。終端等價仍需 EXT-020 使用原 Manager／execd 的實際程序與 PTY 回條。
