# CORE-032：原 Work 的終端 run 授權

EXT-020 的 Manager 以 request_id 接納命令，回傳自己的 run_id。原版從工具呼叫的來源注入 YUKI_WORK_ID。擴展擁有 Manager、execd、PTY 及 stdout；核心只認原呼叫、Work、執行與安裝的對應。

正常：管理者為終端安裝授予 terminal.work 及 tool.call。Main 已接受 Work，然後明選 terminal_exec。寫入操作時，核心將 Host handle 固定至該次執行的 work_runs、原安裝部署與版本。插件只持該 handle 調 terminal.scope，從宿主取得 Work 和 request_id，才向 Manager 發起請求。收到實際 run_id 後，以 terminal.bind 提交該請求回條；同鍵同 run 冪等。terminal_read/control 從自己的新呼叫取得 scope，只有該 Work 原安裝已綁定的 run 能通過。原安裝的後續執行若仍在同一 Work，可續讀；取消一次觀察不會取消 Manager 的 run。

故障：沒有正式 Work、無關工具、偽造 Work 或裝置、跨 Work／安裝猜 run、來源被忘記、Work 已換、部署更換或權限已撤，均不得取得調用權。已派發而失去連線的 exec 保留 unknown；同一部署重啟後原 handle 只提供 reconcile（以原 request_id 查回），絕不重新授權實體啟動。只在原註冊上記下回條，不用它把 unknown 自動變成 confirmed。
