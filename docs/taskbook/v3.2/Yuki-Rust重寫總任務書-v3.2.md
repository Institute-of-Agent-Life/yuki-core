# Yuki Rust 重寫總任務書｜v3.2 核心文檔補核版

**已確認的設計方向（非全功能施工放行）：建 Rust 小核心；保留現成驅動與隔離後端。先通過一條等行為的 QQ 切片，再投資長任務及插件。兩倉可獨立合併、建置、驗證和發行，不要求同時發版。**

**來源與版本。** 本輪直接底稿為續核 ZIP，SHA-256 `8e73bdf30fdf12ce56d0ae3aa1b96cc1015f078afb666cd92251fdf9754fc1a3`。行為固定 [Yuki @ 329ca7c](https://github.com/YuanYeYouTao/Yuki/commit/329ca7c487414d2f54ddd98ff4727446ed2446cd)：4.0.0 開發版、遷移 0156、Plugin API 9.0、Client revision 9、Code API v2。兩個目標倉分別為 `yuki-core` 與 `yuki-extensions`；沒有為本輪建立分支或產品提交。

**本輪可施工的範圍。** [逐項裁決](全量文檔覆蓋與裁決表.md#core-review)將四份固定文檔與實作、舊測試及直接呼叫方對照。H01–H30 分配在既有卡，先設故障再實作。不要求全倉通讀；首穩、單能力和全功能切換仍分開。舊 CI、實際端點與新版產品 E2E 分列。

**前輪續核版保留的變更（2026-11-04）。** 以 v3.2 階段稿逐卡修訂，首穩、功能獨立發版和全功能切換仍分開。新通知卡 CORE-058 僅在獲准的插件命名空間工作；Jev 不再是訂閱的前置。12 份頂層升級指南已分別核讀：同為 0096 的來源可能不同形狀，歷史指南的 evidence-compaction 說明不恢復已退役的證據刪除 worker。具體原件及未讀範圍見覆蓋表，不將已完成分目錄稱為全庫驗收。

## 1　怎麼用這四份文件

| 唯一權威位置 | 回答的問題 |
|---|---|
| 本書 K00–K15 | 同一件事在不同入口應有什麼行為？誰有權？哪次提交後能對外確認？資料由誰保留？ |
| [核心工作卡](核心倉庫-提交級任務書.md) | 該核心提交要改哪裡、如何先驗失敗、產出什麼？ |
| [擴展工作卡](擴展倉庫-提交級任務書.md) | 同上，針對插件、獨立工具與連接器。卡內「交給」為直接依賴反查，不是另一套產品規格。 |
| [跨倉矩陣](跨倉庫依賴與驗收矩陣.md) | 依賴順序、能力覆蓋、試驗分支、E2E 套件、版本發布流程與相容組合。以卡內直接依賴生成圖。 |

所有新增路徑、版本號、方法與命令均是**規劃**；文末固定 GitHub 連結才是已存在的舊實作。新倉中 IDL 是 wire 欄位真源；規格只決定必要語義，不再另外手維兩份 DTO。基線源碼／測試優先於日期報告或 v2 的自述；本書明標的產品決定才改變舊行為。任何未列入凍結差異集的退化都不得當效能提升。

**v3 原版輸入與實驗紀錄（本輪未重跑）。** 原輪 v2 原檔 SHA-256 `aace963fe725f2912b4e7af54a8a53cea4ab67db67126943afa75ba1ce489675`；審閱 ZIP `49f4f2926a4c85e59f24c07ec713314d1510298c56f654ae766a153708ac8fe8`。解包兩份 Markdown 與單獨提供版本相同。v3 曾重跑包內合成 SQLite 探針：SQLite 3.46.1，舊 nullable 唯一鍵容許 2 個活私聊；修正後拒重複、轉移後剩 1 個活入口。舊 occurrence 回填方向容許 2 個工作；execution 的原 occurrence 唯一約束拒第二筆。**這是約束反例，不是 Yuki E2E。** 未讀實際用戶庫、未呼叫任何付費模型、未量 Rust 效能。

## 2　範圍、所有權與交付物

`平台信封 → 經認證 Gateway → 原作者／入口 → 事件與接納 → Main／Work → 原模型 attempt → 工具效果 → 實際交付`。公開資料流止於已確認的出站；原生模型續跑、工作郵件和設備私密問題不經這條公開投影。跨庫界線走「同一原鍵可查」而非假裝跨進程交易。單 Yuki 資料域、單主庫服務 writer；多個 Bot 並不代表多個 Yuki。插件私庫、物件庫及設備 journal 各有自己的寫入者，不能籠統承諾全產品只一個檔。

| 位置（規劃） | 所有與交付 | 不負責 |
|---|---|---|
| `yuki-core/src/{identity,ingress,gateway,history,social}` | 人物/帳號階段/群/入口/來源、原事件、QQ/OneBot、共同 Gateway 與逐段投遞 | 揣測第三方原生 ID、替換未確認路由 |
| `yuki-core/src/{runtime,context,model,tools,work}` | 唯一 Main、四個 wire、固定工具、多輪 Compact、預算、原生搜索、Work/排程 | 第三方搜索、VM、設備或終端的原生管理 |
| `yuki-core/src/{memory,media,privacy,host,operator}`、`src/store.rs`、`migrations/` | 資料讀寫政策、普通提取、混合檢索、不可變檔案/版本與清除、程序宿主、共用授權、CLI | Dream/Self Reflection 策略、通用外掛資料庫、任意 SQL |
| `yuki-core/protocol/`、`sdk/{rust,python}/` | **公共 wire 的唯一規格來源**：新插件 JSON Schema；受支持的個人/管理 OpenAPI 基準、Connector wire4 的固定 schema/金樣與來源。兩個 Plugin SDK 從同一插件規格生成，SDK 不帶主服務。對外 ACP/OneBot 等由上游定義，這裡只釘所用版本及 Yuki 對應。 | 擴展倉/桌面不另維權限模型或可手改的獨立 wire 副本 |
| `yuki-core/{baseline,fixtures,tests,bench,ops,deploy,docs,release}` | 基線真值、核心差分、核心鏡像/binary、migration/backup/cutover、最終可部署相容組清單與運維說明；無插件照常聊天 | 一次性大鏡像、桌面程式、特權容器 |
| `yuki-extensions/plugins/` | 16 個官方插件，見下表；各自實作自己持久資料的離線導入或公布無需遷移的證據。個別 packages/lock/授權通知/資料目錄/README/E2E，按 package tag 發行。相同倉不等於相同版號 | 不引 `yuki-core` 私有 crate、ORM、私有 Python；不靠 workspace 鄰居的原始碼 |
| `yuki-extensions/connector/` | 源自舊獨立包的 Python 3.12 wheel、可攜 Desktop runtime、CLI/service、三驅動與本機 journal；獨立 tag/lock。保留 wire4；更改需雙讀者先上，非首版前提 | 不在伺服器代替原生登入；不另立第三倉 |
| `yuki-extensions/{images/env,images/env-desktop,deploy,tools,compat,ci,docs,examples}` | Linux 文字與有頭 OCI、受信 Manager/網路設定、組合部署、`yuki-setup`、私庫對賬／演練、真包整合、桌面及第三方消費者測試。核心鏡像與這兩個鏡像分開計量 | 不存規格主檔；跨倉依 digest 拉 host/SDK/test kit |

| 官方插件 | 具體產品能力 | 插件自己的資料／執行 owner |
|---|---|---|
| `telegram`、`feishu` | TG 長輪詢；飛書/Lark 長連線及 REST；私聊、群、原生提及、引用、topic、媒體與支持的撤回 | bot/app 憑據、傳輸游標/分片；已接納事件及操作歸核心 |
| `client` | 多設備、App 資料/API、ACP 方向 A（編輯器調用 Yuki）、離線同步、給原 Work 補充 | 接入服務和 `yuki-acp`；設備用途及原入口由核心授權，SSE 不作歷史真源 |
| `local-agent` | ACP 方向 B（Yuki 調用本機 Agent）、Codex/Claude、無 Agent 的本機 Shell/文件 | WSS、設備實例/操作、事件續送；設備 journal 在電腦上 |
| `env` | 持久 HOME、共享 workspace、PTY、服務重啟、不可變交付、可選受控 VNC | 原 Manager/execd、jobs 庫、卷及桌面租約；只在 Linux 宿主裝配 |
| `codemode` | Monty 沙箱、腳本停續與有序子結果 | 隔離 worker/快照 codec；效果與 root 預算歸核心 |
| `participation`、`subscription-monitor` | 真人續聊、SELF 機會、Jev、安靜退出；公共訂閱條件通知 | 參與快照與訂閱 cursor；提案不授予主 Agent 權限 |
| `memory-maintenance` | Dream、Self Reflection、Rebuild 審閱與子集提交、Reembed、來源 audit/hygiene | 維護 run、preview、水位與輸入指紋；不直寫核心事實 |
| `media`、`sticker` | ASR（含 QQ 格式轉碼）、選讀影格/PDF；表情收藏/描述/檢索/補發 | 解碼、收藏的持久原圖及本次處理狀態；Core 管附件授權 |
| `web-fetch`、`tavily` | 讀具體網頁、第三方搜尋。與主模型 inline native/獨立 native 不互為自動 fallback | 自身外連/代理、結果來源與實際用量 |
| `trace`、`character`、`admin-web` | 選裝限期追蹤；角色頭像及平台套用；瀏覽器管理 UI | 追蹤/圖片/瀏覽器 session；SOUL、角色名、領域寫入及授權仍在核心 |

**外部項目，不增加自建倉。** 原 Yuki 為只讀基線/遷移 reader；現有 `Institute-of-Agent-Life/Yuki-Desktop` 是獨立發行消費者；`Yuki-Semantic-Participation` 以 `a6a13d4f04ecc36da26adb5f364abdb4f032fc1e` 為首個 pin。SnowLuma 是 QQ 依賴，參考 `e44e4ced17c5911b9aa9db449ba29f2d9e567d2a` 不代表現場映像。Monty `3f9d6ef413fb951e5b80113b7088d535bd028fcb`＋專用 launcher/補丁；execd `441042c288a3eacbe93b36236a9baf6f6254ad4d`、Docker/gVisor、Vec1、FFmpeg、OpenCC 等為上游或可重用依賴，首輪鎖 digest、架構及 notices。三驅動首先依原 wheel：ACP `0.12.1`、Claude SDK `0.2.172`；這是基線 pin，不是最新版本推薦。原養鯤、GitHub Monitor 等第三方擴展只列外部消費者，按實際清單驗證，各自維護者不受本任務自動指派。

**技術預設。** Rust 使用 Tokio、axum、reqwest/rustls、serde、clap、SQLite/sqlx，固定每次建置的 Cargo.lock/toolchain/SQLite 編譯選項。首版一個 binary、普通參數構造，不預建分層 crate 森林。保留連線池、FTS5/原字面正規化及原分數；原本已是 native 的工作不計作 Rust 新收益。替代索引、hybrid 的投資抉擇只由矩陣試驗裁決。核心與官方移植延續 AGPL-3.0-only、舊署名、保留第三方 notices 和對應源碼；不替所有第三方插件做授權結論。

## 3　共同合同（設計決定）

<a id="k00"></a>
### K00　證據、先測與完成定義

每一行為提交先寫失敗時序與正例，先記基線/期望失敗，再改實作、在同提交交綠。可用舊實作即有正例的移植先跑其 oracle，故意注入缺陷應失敗；單元隔離只用於純函數或無法由 E2E 定位的協議邊界，先列失敗集合。預設無網路、無真金鑰，使用真程序/SQLite/子插件與可控假外部。不得把 fake 改叫真端點、把缺實機算通過或把主群健康算聊天成功。

分類：**基線觀察**有固定連結；**規格選擇**由此書決定；**假設**須有裁決作業；**實測**要有當次命令、版本及產物。量測假設不阻止離線實作，不為了數字延誤合法部分成果。

<a id="k01"></a>
### K01　單一寫入者與可接入健康

服務持有資料目錄排他 OS lock 至停止派發/收攏完畢。在線變更走同一寫入服務，唯讀可開只讀連線；離線變更/遷移先取得同一把鎖。WAL 並不防兩程序先後對外派發。writer 隊列不持 reader、模型等待或檔案鎖；讀階段完成查證與準備，短交易只核真正會影響這筆動作的來源、授權及回執，確認未提交的局部 DB 操作才可重備。未知 commit 先看原結果；不得因另一無關交易提交而整批取消。

`/livez` 為程序存活，`/readyz` 為已持鎖、schema/收件正常、可接 gateway；QQ 未連仍可 ready。`/healthz` 為廉價依賴狀態，詳細診斷另核權。compose 等 ready 而非等 QQ 在線；預設 doctor 不連外、不試寫、不採購。

取消不等於資料庫驅動已停止：待 rollback、實體 close 或已確認 commit 的最後處置完成才交出寫入者。close 結果未知時停止新 writer；健康同時回報讀與寫的實際情況。已確認根提交的提示在釋放 writer 後發一次，savepoint 與回滾不發。

<a id="k02"></a>
### K02　人物、帳號階段與原路由

人物 ID 永不重用；個人無帳號也可存在。Account 的唯一鍵含平台**真正的 namespace**（例如飛書租戶/應用），不是裸數字。Assignment 記原帳號、原 Person/SELF、實際階段和可證起止；暫停保留歸屬，結束才釋放。Person/Account/group card 各持自己的名稱；原事件保留當時 mention 的位置、重複、指向和顯示名，字面 `@x` 不反造 native mention。

`merge(A,B)` 僅統一當前人物，原作者、證據主體、設備命名域、Work、已付請求與送達不回填。物理表示可平坦轉接 root；新執行保存真實入口當時的原 actor，不一律改寫為 root。當前管理資格直接歸人物；按原 actor 和 current root 有效性判斷，顯式撤權不能復活；operator 是不同入口、有限動作，不代人。轉移結束舊階段、關原私聊/停其未來路由，已派發仍回原 actor；發生與接收時間各存，證據不足只記 account，不替遲到訊息造人、造上下文或繼承私聊。

私聊鍵是原使用者歸屬入口＋接收 Bot 階段＋topic；群鍵是原 Space 入口＋topic。新階段可建新 DM；同一 Space 不因接收 Bot 切換拆群。去重辨「同一事件」，native locator 辨「哪個 Bot 能引用／撤回／取媒體」，route 辨「此次應用誰送」。@B 不因 A 先收就改 A。回覆用原有效路由；主動發送用明示路由或唯一可證選項，多個無法選則返回候選，不按 first_seen 猜最早進群，也不失敗後換人重發。多 Bot 無可信共享鍵用已配置唯一接收入口；離線可能漏收，必須回報此限制。

<a id="k03"></a>
### K03　入賬、喚起及平台確認

停用的原生群存在一個窄例外：原群 current enable 走認證控制路徑，不進普通帳本、Main、Memory 或 Jev。對未建群的實際成員查證在 writer 外；失去原連線、資格或出現歧義時不以另一 Bot 接手。

先辨完全空輸入、資料通知、編輯、撤回、自身回聲、受信控制命令、一般人類輸入；平台事件不必都成 Main。純附件/真提及/真引用不按空訊息丟掉。入站時以原事件＋當時 membership/mention 入賬；明確應接納者同一 writer 加唯一待執行來源（足以還原的已正規化原信封或不可變引用），commit 後才 ACK/推進 offset。僅觀察可入賬，不建 activation；其後合法普通 promotion 以同一 event 原子決定一次，不從歷史拼出未持久的原信封。帳本行與接納是兩個事實，不必兩表兩次提交。資料事件若需更新資料則先完成該提交再 ACK。

私聊、原生 @/原回覆、明確命令走對應基線路徑；角色名只作線索，不全域強制觸發。群開放策略/只提及/獨立庫續聊依配置。防抖與前景排隊用 v4 fixture 的實值；先把能匹配的輸入交原 Work，再決定是否開新聊天。真人、SELF 和插件身份互不轉換。打字提示、短期安靜訊號不是授權，不記錢、不阻塞正常回應。

<a id="k04"></a>
### K04　效果、識別碼與分段

| 鍵 | 由誰給／何時使用 |
|---|---|
| `event_id`、`execution_id`、`work_id` | 核心內部；不可用 platform message ID 重建。普通執行可無業務 Work。 |
| `attempt_id`、Provider tool ID | 前者為實際物理請求嘗試，不證明付費金額或拿到完整結果；後者只配原模型函式呼叫與結果。不可用參數雜湊當業務 ID。 |
| `operation_id`、`(parent,ordinal)` | 核心穩定意圖；拆片、附言、Code 子調用先分配原序號。相同參數兩次合法意圖各有鍵。 |
| `(namespace,request_id)`、JSON-RPC `id` | 前者依原 App 入口／ACP session 或原插件來源構成持久寫鍵，不按裝置另開 App UUID 空間，須核 action/target/正規化內容與可信源；後者只配本連線的一問一答。 |

`intent`（未派）→ `dispatched`（跨界前持久標記，崩潰窗可能未真正送出）→ `confirmed / failed / unknown`。發出前標記與外部接受非原子；不能假稱 exactly-once。已證未送才准原意圖繼續；已發不明只查證，無查詢能力就保 unknown，不以新鍵補發原意圖。可繼續無關的合法工作。外部 ACK 與本地結果分兩步；本地效果變更＋結果一個 writer，真正原結果可獨立於可變資源存活。重放先驗讀權及同鍵同內容；forget 後只讀去內容的事實。父效果展示每片順序、ID、狀態、已確認的內部出站；上傳不是附言送達。已取消也要保存原已派發的遲到確認。

配圖意圖不是公開送達事實。當次已選擇配圖時，在首段文字取得派發權的同一短交易保存原意圖與來源；純圖也可單獨接納。文字與圖片分別查原結果，未安裝或未選取就不產生意圖。核心只管原投遞的持久引用，插件管索引、選擇和補圖。原平台確認與本地交易之間的不確定不能用盲重送填平。

<a id="k05"></a>
### K05　Main、付費與根預算

一個 Main loop 供普通、Work、子任務、SELF、排程或受權插件使用；不是每次聊天都強迫接受業務 Work。每次激活保存真來源及所選模型/功能合同；真實邏輯模型請求接納時原子預留根額度；同一邏輯呼叫的 HTTP 重試不重扣這筆預留，額外物理嘗試另計可觀測的 usage／未知費用，額度不能靠並行、重新分段或復原刷新。輸出額/輸入估值如何保留要明示，不能假裝精確 token；未知已購保 reservation/未知量直到原憑據可對賬。已保存完整回應與 tool intents 後才逐個執行，派發與配對可分階段；只重試確定未出站的嘗試或明確安全且有界的配置，對未知效果/原生工具/已付請求不自動再買。單筆新授權行為不是舊請求重試。

工具宣告固定於部署或合法新鏈，按 scope/grant 在派發時現查；direct 預設完整清單，code 可選固定直調列表＋簡目錄。連續獨立唯讀可並行，寫/不確定效果是屏障，回填原序。工具結果依實際序列化信封保狀態/ID/正文與續讀座標，不把「64,000 JSON 字元」換算成等量 bytes/token。Artifact 讀既有原件；聊天 cursor 查新頁，兩者不混。能靜默結束；公開只准顯式 `send`，不默認補發 final；完成提議不吞後續同批合法發送。現在時間與短期記錄保按需讀寫，不退化為永久 Memory 或固定在過期 system 的時鐘。

**不同層次的原件。** `activation` 是本次喚醒或接續；執行有自己的來源與執行 ID；`Work` 是可跨段保存的目標。一次 activation 可有多個邏輯模型呼叫及物理嘗試；一個完整回應可含多個工具操作，一個 `send` 又可有多片送達。各層保存自己的 ID、狀態及關聯，不以另一層的成功補值。HTTP `dispatched`、完整 `response`、全部工具 `paired` 是三個可分別崩潰的邊界；配對還不是正常 Main 已觀察。合法無工具 final（可空）走原結束出口；已確認送達之後不重送。完整決定、原批可執行尾部、逐項回執和真正終態按此順序處理。撤銷當前來源才阻止新效果；取消其他目標不是整批停止。**不新增完成協調器。**

**呼叫樣式不是權限。** 宿主凍結完整執行清單與可直調的子集，`lookup_tools` 讀同一目錄、不修改 schema。直接與腳本子調用走同一原執行器、來源核驗及效果回執。只讀、可並行、可快取是三個不同屬性。只有明確允許且代表操作及其授權可證時才共用結果；一般同參新讀仍是新觀察。分片計畫、非零退出前已寫的檔案和部分未知都不能壓成一個成功位。

<a id="k06"></a>
### K06　上下文與 Compact：按原件移植，不另創摘要策略

固定工具/規則與 SOUL 在穩定前段；採用的公共片段有序、不可變，以原事件及版本定位；本輪實際角色／原定時時刻（非每請求牆鐘）、來源/Work brief、short_state、媒體及原生 continuation 各在合法區段。其他入口的歷史可作有來源的工具證據，不拼進自動公共歷史。SOUL 是唯一人格源、空值合法；換名字或 SOUL 不作廢舊付費 request。

原 Main 做 Compact，沿同 Profile/思考/輸出/工具/併發槽與根預算。軟觸發預設 96,000，不是輸入上限；硬輸入容量為已知 `min(max_input_tokens,context_window−本次輸出預留)`，只設一項則用該項，全未知不捏造。估值計完整請求與媒體。P 只涵蓋允許公開的有序前綴；合法且有未覆蓋原文、無衝突 native continuation 時優先 P。E 壓原執行配對前綴，留 initial、當前目標、原生狀態及未觀察的完整工具輪。P 不可行時自動走合法 E；E 建新鏈後 P **剛變合法**才再考慮。手動 `/ai compact [重點]` 只 P，私聊本人/群超管；不送摘要、不寫長期事實。

不得拆 call/result、把 opaque 改可見文字、截取最近 N 筆頂替 planner、將摘要看過算 Main 看過或重摘要 checkpoint 自身。正常 Main 後續實際 wire 才是驗收：逐比原鍵、參數、結果、簽名/順序。候選估計沿原合法邊界掃描，保留沒有單調性的私有情形。原回應先私有持久化，再核當時選片/覆蓋點/隱私世代/前 checkpoint 發布；無關新追加不拒，有關 reset、來源修訂或刪除則拒。未成 checkpoint、request 尚可容納便繼續；硬容量不足報真失敗。已知相同失敗候選不可重買，滿額可用已付回應做純 DB 收尾。完整分支與原測試見 [P/E 現行合同](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/conversation-compact.md)。

<a id="k07"></a>
### K07　查到事實，不等於可讀原私聊

所有查詢先套當前 caller、source、delegation、entry/thread、資料可見範圍，再取候選或向客戶端輸出。歷史 recent 預設本會話、search 預設全部獲准來源、around 認原 event/operation；含作者、日期、source、附件及逐段 ASR 字面查詢與 around 鄰文，精確保 NFKC/casefold、OpenCC 簡繁與 keyset。顯式跨入口讀取不改公共上下文。聯絡人與會話目錄分開；群成員標 observed/原生實查範圍，不聲稱觀測完整。

Person 本人事實可讀；其他 Person 需雙方**直接**歷史共同群；Group 需該群歷史關係；PersonGroup 需雙方與該群同時成立；SELF 只依 global/current-private/current-group。此既有取捨可能讓群友看到源自私聊的結構化事實，但**不開放私聊原文、quote 或完整 evidence**。查 fact、查 evidence、寫 fact、讀檔案、讀 trace 正文各自判權。對外摘要/結果預覽同樣核權，不讓遮住全文卻露了 quote。管理者不因此取得他人的本機操作權。

<a id="k08"></a>
### K08　Work、排程與可靠交接

每個子 Work 有自己的原執行、來源、私有 inbox、父/根 ID；可補充、問父、等待、續接、精確取消。`wait` 以原 Work/子 run、本人輸入、plugin event 或時間為條件；記已命中項，`any` 命中可另等餘項，`all` 是基線已支持的合取，局部命中留在原等待，只該組等齊，不將其變成普遍完成門檻。登記前已抵達的事件亦可命中。只合併同操作不要求決策的進度；重要問題/終態/已接真人補充要保存到原私有 mailbox，寫入與推進消費水位同交易。終態後不把晚到私料改送原群。允許父保留部分結果；取消阻止新效果，非撤銷已確認效果，不因停看/斷線或原生 detach 就宣稱取消。

排程含 once、after、interval、daily、weekly；本人或 SELF 創建、保存原指令、時區、目標與已接納的 planned UTC。唯一 `(schedule_id,planned_utc)` 和唯一 `execution.source_occurrence` 同交易。已有 occurrence 不隨排程改寫；未來按新設定。固定守時 oracle：DST 重時取 fold=0 一次，缺失牆鐘時間向前，週期完成後從 `max(scheduled_for,now)` 推下一合法時刻，不追補風暴。`cron` 不是已核對的原行為，而是 v2 的新增入口：採明選五欄 minute/hour/day-of-month/month/day-of-week，以排程 IANA 時區解譯，日與星期若同時非星號採 OR，分鐘粒度、不支持秒或供應商別名；重時/缺時採同一牆鐘政策。CORE-018 分測它，不把舊五種規則改寫成 cron。

子提問沿原 child/revision 進入父 Work。父已終態時不令子節點為等候答覆而卡死，不重開父；獨立暫停的父保留問題供顯式接續。子內部完成不等於對外投遞。

**動作准入表。** 下列為固定基線動作；兩倉可換公開拼法，不得改變哪一筆操作能產生效果。所有「不需」仍須具備此次真實來源、目標與授權。安裝時不提供的能力不憑表臆造。

| 動作 | 需要已有業務 Work？ | 不可混淆的邊界 |
|---|---|---|
| 普通回答或 silent final、顯式 `send`、記憶讀寫、查歷史／時間／短記、獲准終端／環境與排程操作 | 直接工具不需一律先 accept | 寫操作、傳輸、預算及每片回執照常；由腳本呼叫時另受外層 Code 准入。定時到期亦不替 Main 強制建 Work。 |
| `task_control.get/list`；管理已存在的工作 | 讀目錄不需新建；管理一定要有可證的原目標 | 查不等於能改。中立前景可按 local scope 接續原 ID；指定其他 work_id 的 cancel/fail/resume 走原管理權，不假造新目標。 |
| `accept`、`derive`、`subagent_start` | accept 建立第一個目標；後兩者需真實父 Work | 普通執行已有當前 Work 時另一 accept 可排新目標；已登記子工作者不可再 accept 自己。直接父不是預算根。 |
| 當前 `complete/fail/cancel/need_input/wait/update/wait_status/cancel_wait` | 對當前目標的控制要有真 Work | finish 決定與送達分開；被取消／失效的來源不再派新效果。本人的 plain 回答可接原等待，跨入口必須明指 Work。 |
| `subagent_control` 查清單／狀態／結果，補充、恢復或取消已有子節點 | 管理舊節點不用另 accept 空父 | 現查原來源、方向、子樹及可管理性；提問父任務須有真實親子關係。 |
| `lookup_tools`、`read_tool_artifact`；`execute_code` | 前兩項能直調時不需 Work；外層組合必須已接納 | 目錄與 artifact 讀不扣業務次數；不保證免費模型輪。組合父不扣業務次數，子業務各扣；原件續讀不重做原工具。 |
| 本機 Agent `list/get`、`prompt/queue/watch`、`steer/respond/cancel/detach` | 僅中間三項需先接納 Work | watch 只監督；其餘控制針對已授權的原資源／問題，不因免建新 Work 就容許控制他人的原生會話。 |

`waiting_user`、一次性的 any/all 信號、因容量或來源而 suspended 不是同一狀態。信號不清除獨立暫停原因；不以已回報、附產物或全部子節點完成作為 Work 完成門檻。

<a id="k09"></a>
### K09　供應商、搜尋、計費與狀態

管理用量保每欄的已知分母及原 provider/model。cache read 的分子與分母取同一批同報呼叫；未知、真零、失敗和未定價分開。每日使用指定時區的民曆邊界，不能當作滾動 24 小時。

顯式 `task → profile → endpoint/model/protocol/thinking/settings`；各 task 可分 chat、extraction、reflection、dream、search、ASR、vision、embedding、已批准插件。共用 HTTP client，統一執行計量，不按內容暗換供應商、提高思考檔或靜默使用付費 fallback。結構化模式 per Profile 選 `function_tool`、`text_json` 或 `json_schema`，結果驗結構與來源；回應截斷/空/普通正文不造工具。inline 搜索受真正端點權限及能力約束，不可在伺服器工具中途假裝能逐次攔截。獨立搜索只送 query/constraints 與 native 搜索，不送主聊天或函式清單；只快取完整有效結果，key 含路由/憑據域/時效與全部限制。無來源、部分成功、拒絕、未知分報；取網頁正文不再買一次主模型。

| wire | 基線／官方文件核查 | 施工裁決 |
|---|---|---|
| Chat Completions、OpenAI Responses | 兩者工具呼叫和回填不是相同 wire；Responses 的 function_call/output 以 call_id 配對，部分 reasoning 模型另有 continuation。 | 以模型與方言固定 fixture，保 usage/null；不得把一家的 `previous_response_id` 推廣到所有兼容端點。 |
| DeepSeek Responses | 基線 `supports_tool_choice=False` 是舊程式限制；官方現表列 `none/auto/required/指定工具`，原生 `web_search` 列 `deepseek-v4-pro`、對 `deepseek-flash` 忽略，且無伺服器 previous_response_id。 | 基線 golden 與現行文件 golden 各存一份；候選 profile 可顯式選新能力，未測端點標未驗。收到 200 並不能證明真的搜過。 |
| Claude Messages | manual extended thinking 下強迫 any/指定 tool 不是合法組合；不可將此限制推給每個模型或 adaptive 模式。 | 保簽名 content blocks、tool_use/result ID；基線思考開啟時將邏輯 required／具名選擇投影成 wire auto；關閉時分別為 any／tool。以唯一 emit_result 及 schema 驗回應，不偷關 thinking、不用第二輪猜。 |
| Gemini GenerateContent | thoughtSignature 隨原 Part 返回；與 Interactions API 的 server-managed state 不同。 | 保 parts 順序、函式與 grounding；逐 endpoint 測混用 native＋本地工具，不默換協議。 |

以上有原碼與文件證據；**本輪對四類實際端點的驗證結果都是「未測」**，不是不支援。欄位記 `documented / implemented / wire_replayed / endpoint_verified(date,region,model)`，不從前一欄自動晉級。DP 決定有無授權探針、失敗如何縮範圍。模型能力的官方 URL 見文末。

**分開驗收實際協議。** Chat 以 `assistant.tool_calls[].id` 對 `tool_call_id`；Responses 用 `function_call.call_id` 對 `function_call_output.call_id` 而不是 item ID；Claude 的 `tool_use.id` 對 `tool_result.tool_use_id`；Gemini 依原 Part、name／真實 id 配 `functionResponse`，無原生 ID 時不得把內部鍵送回上游。簽名、思考及 opaque 不進公共段或 `send`。基線以完整 HTTP 回應解析；連線中斷即使已有可解析參數仍不執行。完整的 length/max_tokens/MAX_TOKENS 非成功：保已付回應與未執行配對；唯有合法 pause 才沿同一 checkpoint 接續。增量 Provider SSE 若另實作是新版能力，不是 Client SSE。

部署搜索啟停、主 Profile 的 external／bridge／native／both、獨立連線及 native 宣告分開處理；disabled 不因有金鑰而翻轉。只有上游結構化來源能作搜索證據，不以正文 URL、供應商摘要或 200 當成已查原頁，原生失敗不自動轉 Tavily。成功結果快取、上游 token cache、物理計費是三個不同事實。具體測例與何時保留 unknown 見 CORE-011、012、019–021、024、049。

<a id="k10"></a>
### K10　媒體、隱私清除與可刪的副本

事件中的 attachment index、segment index 和 ASR 來源各自穩定；初始訊息先提供索引，圖片/影片/文件按需，必要語音可在真前景準備。首輪不默買視覺描述；明選 vision 才採購。暫存原件與長期收藏、不可變交付版本、私有工具原件有不同 owner/期限。讀取時以受限根安全開檔或持有 FD/handle，展示前重核來源/版本，不能把插件提供的絕對路徑當授權。共用 env 的 `/inputs/media-cache` 是既有已知共享邊界，不宣稱不同會話在任意 Shell 內互相隔離。

forget 對原 Person 同人族及歷史所有權查閉包：作者與所屬私聊、會員/裝置/碼/本人命令/安排、涉及其來源的事實和 evidence、相關 Work/私有請求及 checkpoints、他人工作裡實際使用該私料的部分、FTS/向量/ASR/attachments/artifacts/診斷正文。按原階段清，不按今天的帳號刪了新主人的資料。建立時保存已使用的 source refs 或可定點尋到的既有關聯；不靠全倉掃字串找秘密。writer 先撤讀與將舊輸入失效，清引用與派生；文件刪除/回收另在 commit 後，結果分 logical/physical/pending。檔案有其他合法引用時清訪問權及專屬版本；不得把共享底層 bytes 宣稱已物理消失。任何遲到回應須在**保存正文前**核對來源（包含模型原件、trace 及插件佇列）；已買/已發最小去內容鍵留存，防舊操作復活。

對曾持有資料的插件，即使已停用/暫離線，也以 `(forget_id,plugin_id)` 記待清義務，啟動前先核該義務、完成原鍵清除才准服務受影響資料。程序未裝仍保留清理工具/資料位置或列現場待辦；不以通知丟失宣稱成功。主機備份、外部消息、供應商資料、用戶電腦/自行複製的文件和第三方記錄不在可保證遠端抹除範圍；備份重建要先套後續已確認清除、隔離舊讀者，不能讓被忘資料復活。含正文的自管 debug/trace 不是此排除的藉口。預設 log 移除正文與祕密，不宣稱取證級抹除所有磁區。

ASR 的輸入是來源可證的本次語音，不是被引用舊消息或歷史掃描。轉寫在原事件上是資料，不是可信命令、管理確認或額外的喚醒授權。

<a id="k11"></a>
### K11　Memory 的五種行為不互相代替

普通提取與 Rebuild 只產具體來源的 Person/PersonGroup/Group 新事實，可零項；原 quote 必須來自其 source event，附近事件只消歧。同 key 可獨立存在。明確更正/撤回按 fact ID、原請求、身份與版本，保留可驗結果。Dream 在正式事實分區內整理，保原來源、指紋、可復用 preview；Self Reflection 讀本批新消息與真工具回執，按可見範圍建 SELF，不借自動提取器生成。Quality 是來源核驗/統計、可核權的 hygiene，不新增一條無預算的自動模型評審鏈。

先提交正確事實，再於 writer 外按 profile/purpose/內容分批 embed；查詢快取只重用按模型指紋與查詢計得的向量，不存 ACL 或整份結果。forget 時清退相關暫存；若不能可靠界定某查詢的來源，直接清空這一小塊可重建快取，不為它新建永久歸屬索引。索引已過期、不足覆蓋、近似、截斷及服務失敗分開。詞法與向量均在授權集合中先選候選再精排，保 exact、相關性分帶、RRF 和穩定 tie，而非只搬一條公式。**標準安裝 hybrid**；顯式 lexical-only 作為舊系統可用的獨立配置，不能拿它與 hybrid 比費用。選 hybrid 缺必要配置是啟動配置錯；運行期失聯可降級，須標 coverage 與原因。模型名、維度、區域由已配置端點確認，不內置「這一定可用」。

<a id="k12"></a>
### K12　Host、跨倉合約與管理信任

部署者信任所安裝的程式，不代表准許它代任意人。新子程序協議為雙向 stdio JSON-RPC 2.0（非 MCP）。宿主認證程序/通道並綁定 plugin_id、明示 grant 和來源；真人操作用宿主建立的不透明 call/session handle，背景維護用獨立窄 grant。權限＝部署批准∩真入口/委託∩當前資格∩操作範圍∩原預算。省略 actor 不升權，自報 Person、root 或超管布林均無效。重新握手不替換已提交 manifest；禁用立即阻止新效果，不抹已發。動態能力由狀態/目錄查，不每輪改 schema。只核真正用到的 revision/來源，不設通用門檻。

`initialize` 協商版本／必要 feature，固定 declarations（bindings、效果種類、schema、命令精確路徑、媒體 MIME、事件訂閱和 forget）；重名或必要能力不匹配局部停用並可診斷，明選 code 卻缺 VM 則該部署配置報錯。支援雙向交錯、限制在途資源但不持全域鎖跨 await；實際循環 A→B→A 沿祖先鏈拒，不用固定深度當完成門檻。雜訊事件先過濾後建 payload、允許背壓丟棄；業務接納、重要 native 回流和 forget 都走原鍵可查/可續交。憑據、一次性碼和登入連結走不入一般 ledger、Memory 或 trace 的私有控制回覆。子程序同 UID 不是敵對程式沙箱；不繼承所有父 env，不發主庫路徑，非受信程式須由部署者另設 OS 隔離或不載入。

**Code 恢復用同一份效果事實（CORE-034／EXT-024）。** T0 在 writer 外準備 VM bytes 和參數；T1 用同一交易發布 snapshot 引用、父 checkpoint、尚未派發的子意圖；T2 認原操作接納／派發，T3 存原 typed 回執，T4 才按 `feed_index` 與 `engine_call_id` 餵回原 VM。業務身份是父 operation＋child ordinal，不是 VM 序號，更不是參數雜湊。能證明未派發的原尾部才可繼續；只剩已派發未知、快照不能恢復、段額已盡或父已配對 partial，分別回報，各自保留已有結果。不為恢復新開腳本或買模型確認。核心持有授權、預算及原效果；擴展只擁有實際解譯器與受綁的快照，不能用查詢權換取執行權。詳見[驅動的 T0–T4 行為](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/codemode/driver.py#L1-L16)。

**各 public surface 的實際消費者（簡寫表示方法族，非任意讀寫端口）：**

| 能力 | 入口與輸出 | 消費者／不用此接口做的事 |
|---|---|---|
| `gateway.inbound/status`、`plan_send/dispatch_part/recall/media/members/activity/profile_set` | 認證 namespace、原 bot/native/段/原生時間 → event/admission 或原 part 回執 | TG/飛書與內置 QQ 走同領域；member list 不證明任意路由授權 |
| `tool.call/status/catalog/invoke` | 綁定 call handle、schema rev、父鍵/序號、args → 原 outcome/有權 handle | Monty 子呼叫；目錄不動工具宣告；每一跨界 write 需穩定鍵 |
| `command.invoke`、`notify/turn.trigger`、`ordinary.promote`、`self.propose` | 認證命令/插件 origin 或受准原 event/proposal → 原執行引用 | 訂閱、參與、/ai 子路徑；三種身份不互換，不准插件冒真人 |
| `device.pair/auth/revoke`、`work.get/input/cancel`、`native.notify/ack` | 用途＋原 entry/scope、request/operation/event → 原結果或已接納水位 | client、local-agent；對人操作返回可讀投影，不能用 ID 猜 owner |
| `history.page`、`memory.search/read/preview/apply/reembed` | 綁 scope、range、真來源 refs/版本 → 授權頁、候選或原寫入結果 | 維護、個人更正；fact detail/evidence 不是同一權限；無任意資料庫訪問 |
| `media.process/open/publish`、`llm.complete`、isolated session | 已授權 handle/目的/route，版本化內容與原 usage | media/sticker/隔離插件對話；不用 Core 提供通用 KV/任意文件服務 |
| `control.*`、`trace.observe`、`forget.request/ack` | 已認證 operator/設備/插件控制身份，原 request_id；目前能力、清除狀態或受讀資料 | CLI、admin-web、trace、character；metadata 與正文分權。control 僅登錄當期消費的方法，不是任意 method 轉發。 |

可丟 trace 不能代替 Personal API 要求的正式原回執。讀敏感裝置歷史時，plugin 取得核心認證的當前 scope；長時間的會話/畫面跟蹤撤權，重新接線沿原租約。版本號、矩陣格與發布順序的權威是[跨倉矩陣](跨倉庫依賴與驗收矩陣.md)。

插件有三條不同通道：普通 metadata 可丟且綁定當時實例；插件明示發布通知有具名目標／穩定原鍵；Jev 判斷只參與真人／SELF 接納。不可用同一個可丟事件取代後兩者。已綁命令即使插件離線也不可落入普通 Main。已批准的設定定義與運行中實例分開：可在停用時改設定，不能因此觸發插件或代其作現場觀察。

<a id="k13"></a>
### K13　個人端、本機及伺服器環境

個人端：chat credential 綁**設備＋原 Person**；原入口是獨立 Binding，可由多設備共享；ACP session 用原入口＋thread，原 UUID 命名空間不隨合併。本人從新授權入口補原平台 Work 的 mailbox，不建 App Main，也不改原交付目的地。HTTP 入賬 ACK、Main 結束、Work 完成、已確認出站、使用者已讀分開；SSE 提示丟失用原 GET/keyset 重讀。ACP A `load` 不跑模型，`cancel` 只針對原 prompt/精確關聯工作，斷 stdio 不自動取消。Desktop 外部 controller 以版本化公共介面接入，不能讀主庫。

**個人請求的實際鍵域。** chat token 的原 Person、當前授權人物與 `X-Yuki-Entry` 指向的原 App Binding 各有用途。同入口多設備共享 UUID；不同原入口不因人物合併而混用。原消息、命令、上傳、補充共用原入口空間；按原操作、目標、正規化正文、有序附件與可選 agent_target 比對。ACP new 以原 App namespace 接納；之後是所選 ACP Conversation＋原 UUID，舊 NULL scope 只按原資料讀。chat、connector 與獨立 operator 的憑據不互換；具資格的 chat Person 走已准管理 API 不因此成為 operator。

**補原 QQ Work。** 一次短交易保存 App 的私有補充事件、QQ Work 的 ready mailbox 與對應 UUID 回執。等待中取不到原 lease 就回 unavailable，三者不落；已終態拒絕，running 不另開 App Main。原會話、代次、預算、交付目標不變；設備能看獲准 Work 結果，不等於能讀群聊天或私人工具參數。SSE 先訂閱再 sync；滿佇列關流，以原入口 events 游標與 Work revision 補 GET，Last-Event-ID 不是業務游標。沉默可有明確結束，沒有已讀證據是未知。

**檔案與 ACP。** 僅暫存到期且原授權仍在時，舊上傳重入只讀原回執，不使已刪 bytes 復活；新使用與下載均另核當前授權。事件附件按原 Conversation/event/媒體序號和 ETag，不可變產物按 id＋version 及原可見引用，讀取後再核權。不承諾伺服器 Range。ACP load 讀完所有可讀頁後返回；prompt 查原 request、已確認送達及精確關聯的 Work 和 active，而非靠 SSE 或靜默推估。取消只收攏該原 prompt／子樹，保已准尾部送件；工作等待本人可結束本次 prompt，但不是工作已完成。**新版差異：**基線 ACP 只為有版本的 artifact 生成連結；已有 HTTP 的普通臨時附件若能以可信索引定位，新適配器才產生帶原路由但不帶憑據的連結。未定位、失權、已清理不假造可下載結果。此變更只屬 ACP 能力，不改 B0。

本機：核心 owns 根身份、設備憑據/用途與撤權；`local-agent` owns WSS 連線、實例、session/operation/監督關係；Connector 在本人電腦保原 journal/驅動及登入，完整版號與 wire 版號分離。沒有 instance 仍可用明授 Shell。`full_access` 預設關，只在本機開；關閉拒新 exec/stdin/file，但保原允許的查詢/定向取消；撤銷裝置後只查既存服務端事實，不再下發遠端控制。停用實例、刪配置、斷網、取消 run、解除監督、撤憑據、遺忘不可合併為「刪除」。編輯/刪實例不重啟無關 Shell 或 daemon。重要事件先在插件落庫並同記未交核心，再 ACK 裝置；核心以原鍵持久接納回 ACK 後才消義務。序號缺洞按見到的原鍵處理，不把最大序號誤作連續水位。原生接受≠原生完成≠Work 決定≠平台送達。

伺服器 env：沿用原 Manager/execd/gVisor、出口隔離、持久 HOME/workspace、PTY byte cursor/exit/completion、服務 restart。後端與 VM 不裝時不耗其資源；裝後整組計量。Bot/插件對 Manager 的調用不等於具有宿主 Docker socket。工作檔交付凍結版本；共享只讀媒體與不可變輸出庫分開。桌面保本人/超管適用的畫面受權、租約、撤權關流與釋放；不得用裸 noVNC 或共同鍵盤代替。首版不另造低隔離 Docker/tmux 產品。

<a id="k14"></a>
### K14　導入、切換與回退（不永久雙寫）

模型設定的原檔、舊資料行和環境來源不能假裝一次提交。離線移轉若檔案已發佈而清退未確認，輸出 partial 與實際 revision；先比對兩側，再續做同一清退。不從另一來源啟用已明關的搜索、ASR 或 Embedding。

1. **先辨所有者。** 定位 0156 主庫、WAL、原生協議物件、artifact manifest/bytes、媒體/表情、participation/插件庫、Manager jobs/HOME、配置/祕密、設備命名域與不在伺服器的資料。用 SQLite backup API 或停寫取一致副本；單庫 snapshot 不保證跨庫／檔案同時。跨 owner 短暫凍結相關寫入/引用、記水位、取副本後校 FK/sha/閉包。不得複製正在寫的裸 `.db` 後宣稱完整。
2. **唯讀導入與分類。** 核心/各插件各導自己的資料；Host 管的隔離會話只由 CORE-054 導，EXT-066 按原 namespace 驗插件引用。EXT-041 只彙合 owner 證據，不寫各插件資料。穩定原鍵可保就保，否則具可逆 lookup 映射。包括 ended/disabled 時段、遲到、舊私聊、historical membership、提及、SELF/tool evidence、非 active facts、清除標記、排程已接納/下一次、逐段效果與未知、計費、設備/授權和不可變輸出。可重建的是 FTS/向量/快取，不可重建的是發過的消息、舊 owner 或已買回應。逐類 count、孤立關聯、hash、有意排除、未明原因列帳。無真實副本時合成結果可交但該部署不可晉級。
3. **接管原 Work。** 每個原執行只指定一種：新 reader 已證明相容可接續；舊端在交接前按原權限收尾；或保原 ID 的查詢/暫停待人工處理、不重新派發。不能同一 scope 兩線持有發送或模型憑據，也不等全世界的任務終態才切。shadow 只讀錄製資料，不觸發 search、模型、exec、網關或插件通知。
4. **真正切換。** 停舊收件/排程及派發，完成/記錄已在途的原回執，固定最後水位；導入最後差異及原 pending、驗外部 cursor/文件/隱私水位、確認新版唯一 owner 後交原入口。不能證實增量完整則重取一致副本，或在舊版繼續，不能漏掉切換窗。新消息由平台可重投協議接續；無重投能力的入口另安排緩衝/短維護並明報窗口，不宣稱無損。真實切換是部署者獨立授權動作。
5. **回退分界。** 未產生新事實/效果，撤新 owner 後可回切原完整舊組。已有新寫、forget、支付或外部投遞：先停新派發，保全新庫、各插件水位、檔案和外部原鍵。只可退到**已驗能讀寫該 schema、wire、journal 與新事實**的上一個相容新版；否則保持新資料唯讀、修正前移並續原鍵。要回到 v4 writer 必先另以全量反向映射證明新寫、已付與清除沒有缺口；本計畫不承諾現成雙向橋。沒有這個證據，v4 只准受控歷史查詢，受後續 forget 遮蔽，不可重新對外執行。文件演練必在新寫入後測，不只空庫 `down`。

<a id="k15"></a>
### K15　效能／成本與發布宣告

A＝固定舊基線；B＝相同舊基線的可重現最小優化（存精確 patch），找不到可證熱點則 B=A；C＝Rust 核心＋實際安裝的 Python/原生/外部後端。同 seed、到達曲線、並發、硬體限額、SQLite 版本/持久性、資源與功能集、模型/提示/思考、schema 政策及輸出任務。私聊 200、兩群共 1,000、20 個長任務及可跨多次 Compact 的對話為基本負載；另有大歷史、稀疏權限、背壓與冷啟。先驗外部效果/交付品質，才計算速度。

分三種證據：①本機無外網錄製重放，比程式；②記錄日期價目與錄製使用量的情境估算；③另獲授權並固定額度後，實際端點留 response model、usage、receipt、時戳、區域及帳單。分層報告，不拿假模型的零延遲作真人速度、不從錄製快取猜當日帳單。冷暖交錯，保留每次耗時/樣本量與不確定區間；按工作執行為統計單位，少量樣本不外推 p99。報入站→接納→第一模型 request→第一確認可見出站→工作終態；將防抖、等人、writer 排隊/持有、SQL、序列化、跨進程、模型及外部網路分開。資料採 CPU-seconds、cgroup 整組 PSS/RSS/峰值各自定義、DB/WAL、物件/卷、共享映像層及實際增量；拆本機、核心、SnowLuma、env/VM、所有插件。

每輸入任務成本和每正確交付成本都報；後者分子含該組失敗與未知的已知支出，分母零則無從計算。模型物理請求按 Main/Compact/搜索/提取/Jev/Dream/自省/媒體/插件；拆未命中輸入、cache-read/write、輸出與已含的 reasoning 不雙計、工具 schema、lookup 回合、原生搜索/取頁、Tavily、query/document embedding、重算及本機 Agent 的可得外部用量。缺字段保持 unknown 而非零；主機固定費、可變儲存/流量與維護工時另列。未換機型或付費方案只能報容量餘裕，不能稱已省現金。對照結果可推翻擴大重寫；不得把減功能或減隔離計作收益。

## 4　物理設計起點與約束

下表是**最小可實作的資料草圖**，不是指定舊表名或數量。`PK/UQ` 為不可改的唯一語義；未標約束的派生欄可依查詢計畫調整。工作卡 migration 路徑中的 `{seq}` 在合併時取當前主線之後的唯一序號，已發布檔不改名；並行卡須 rebase 並測從此前已發布版本升級，不以預排編號要求別的功能先合併。SQLite FK 必開；外部 I/O 永不跨短 writer。將原生 payload/較大內容保存在有 owner/ref 的不可變物件，一個已付請求依賴多個來源時保來源列表；可用引用表或已有可索引關係，不需逐輪重抄全歷史。

| 實體／必要欄 | 約束／讀寫邊界 | 主要讀者 |
|---|---|---|
| person(id, root, enabled, admin), account(ns,native), assignment(id,acct,owner,kind,state,from,to), space/membership | UQ 帳號名域；同 account 最多一個未 ended 階段；owner 可空，原 actor 非 root 別名；merge 原子調整當前族 | 授權、歷史、設備、事實 |
| entry(id,scope,actor_stage,bot_stage,space_entry,thread,closed) | 活 DM 分 thread=NULL / NOT NULL 兩個 partial UQ；群入口同理，必要鍵 NOT NULL。歷史關閉不佔現行唯一鍵 | 接入、輸入、路由 |
| event(id INTEGER PRIMARY KEY AUTOINCREMENT,entry,raw_actor,source,occurred?,observed,segments,mentions...), native_ref、source/admission | 可信去重鍵 UQ(entry,key)；native 定位含平台域、bot、對話/訊息及版本，另存每接收者；一個原輸入一次 ordinary admission；接納與 source 同 transaction | ACK、history、Main、恢復 |
| execution、work、wait/inbox、occurrence | execution.source 唯一；root/parent/actor/source/lease 或單 owner activation、generation；inbox 原事件鍵 UQ、消費與 step 原子；occurrence UQ(schedule,planned UTC)；execution.source_occurrence 唯一 | 調度、個人端、子任務 |
| attempts、steps、op/part、budget | 每物理 attempt 一筆，response 與 usage 可缺；步 UQ(execution,seq)；業務鍵或命名域＋request ID UQ，子鍵 UQ(parent,ordinal,type)；簽名/設定保原件；外部 receipt 不推定新效果 | 恢復、成本、逐片送達 |
| public fragments/checkpoint、private checkpoint、object refs | 覆蓋範圍＋來源賴以發布；原 call/effect 對應、前 checkpoint 及真實 privacy invalidation；原內容可去重，私有引用不成公開讀權 | Compact、GC、forget |
| fact、evidence、history/fact FTS、embedding | subject 與 visibility 分列；evidence 類型有 event/tool/run；不設 owner/key 單 active UQ；向量 UQ(fact,profile,purpose,content hash)，按新鮮度查；授權先於 top-k | Memory、維護、證據詳情 |
| attachment、artifact/version、readable source、erasure／清理義務 | 內容 hash 不授權，handle 綁 owner/source/version；查後可失效；清除 ID＋持有資料的插件唯一，重入不再執行業務效果 | 客戶端/媒體/檔案/隱私 |
| device credential/entry/scope、授權與命令原件 | 憑據僅 hash，chat≠connector，原 UUID scope/class、去內容回執保留；外部執行/namespace 不遷給新戶 | client/local-agent、CLI |

**主模型／人機入口清單。** 核心供 send/recall、歷史 recent/search/around、contacts/conversation/members、memory search/read(fact/evidence)/change、選讀附件/工具原件、目前時間/short_state、Work/subtask/wait、schedule、web_search；不以工具名稱數量為指標。插件工具及命令按註冊路徑命名、部署時固定。CLI 至少有 init/run/migrate/doctor、stats/plugins、person/entry/binding/group/role、memory/forget、work/schedules、import-v4/查交接；`/ai help/new/compact/memory/tasks/stats/plugins/admin` 按原入口授權；`/ai trace`、`/ai memory rebuild`、頭像、配對、訂閱由對應插件精確註冊。CLI 及 Desktop model config 由同一領域寫入服務執行；磁碟已保存、已生效和需要重啟分別回報。

## 5　從 v2 與獨立審閱到本版的裁決

| 審閱條目 | 處置及最小代價 | 施工位置 |
|---|---|---|
| 01 入賬／ACK | 採納：對已接納才同交易存原來源，純觀察不建 Work。 | K03；CORE-008、EXT-003、EXT-005 |
| 02 多 Bot | 部分實測：鍵、locator、route 三分；不接受 first_seen 或內容指紋。無雙機證據先指定接收者。 | K02、DQ；CORE-004/009/013 |
| 03 歷史人物 | 採納；保原階段/原 actor，查現在人物用平坦 root，不強制遞迴鏈或永久權限快取。 | K02；CORE-006/007 |
| 04 插件授權 | 採納；用綁 handle 和具名背景 grant，不重建 API9 審批中心。 | K12；CORE-032/033/035/037 |
| 05 效果 | 採納；跨界前逐項標記而非整批 started；同鍵異參衝突，未知不自動重派。 | K04；CORE-010/013/034、EXT-024 |
| 06 長任務 | 採納；事件命中才喚醒，根預算原子預留、取消不反寫已發生事實。 | K05/K08；CORE-011/016/017/018、EXT-013 |
| 07 Compact | 採納原行為；不採「只留摘要和 up-to ID」替代原件。 | K06；CORE-022/023 |
| 08 記憶 | 採納原分工；不採模型見相似即覆寫、自動 SELF 或模型 Quality 取代來源審核。 | K07/K11；CORE-026 至 029、EXT-033 至 036 |
| 09 forget | 採納；跟實際來源依賴而不只按發起者；離線插件待清可續。 | K10；CORE-052/030/032、EXT-041/045 |
| 10 client／本機 | 採納；保多設備原入口、wire4 及具體待交義務；不為換語言升 wire5。 | K13；CORE-033、EXT-007 至 019 |
| 11 env | 採納；包現有後端，保 HOME、run、退出碼與租約；不把裸 Docker/tmux 稱等價。 | K13；EXT-020 至 022 |
| 12 模型 | 核實後修訂：歷史 adapter 的 False 不限制官方今天的 tool_choice；官方宣告也不證明用戶端點。 | K09、DP；CORE-019 至 021、024、049 |
| 13 writer／鍵 | 採納；以一個服務 owner、兩個 partial UQ、occurrence 的正向唯一關係取代多實例租約。 | K01/K02/K08；CORE-005/007/018 |
| 14 功能與凍結 | 採納；全命令路徑、真插件只凍結已見證的基底或新增切片，全套驗收獨立；匯入含原工作和付款證據。 | K12/K14；CORE-041 至 045、053；EXT-067/042/046 |
| 15 啟動循環 | 採納；/readyz 不等 QQ，便宜健康讀不觸發背景查詢或付費 probe。 | K01；CORE-005、EXT-040 |

**其他 v2 取捨。** D1 改為可對賬的新庫離線導入，不做原庫破壞升級；D2 QQ 內置、TG/飛書插件照用；D3 改設備不等於新帳號；D4 自有協議不接 MCP；D5 改保既有後端；D6 保 Python 參與庫與原 ordinary/SELF 分工；D7 VM/有頭可選但保恢復/控制；D8 延續許可與 notices；D9 多 Bot 支持範圍依鍵實測；D10 基礎原生搜索＋明選第三方；D11 本機服務插件＋獨立 Python wheel；D12 原能力逐項遷移。刪除以**無消費者**為準：重複資料掃描、無訂閱的序列化、死相容寫路、純診斷門檻可不搬；舊付費或舊 UI 尚需的 reader 不能一起刪。刪除活路時同步去設定、裝配、提示及凍結舊限制的測試。v2 的行數、表數、16 週、IPC 毫秒數、Rust 節費都不作承諾。

## 6　里程碑與真正需要授權的事項

| 里程碑 | 到手的可驗物 | 接下來的動作 |
|---|---|---|
| A：證據先行 | 基線重放、來源/資料盤點、兩份 SQL 反例、端點證據表與 QQ 待採欄 | 不等真平台/付費授權即可開 B；未知仍標示 |
| B：單條等價 QQ 切片 | 原來源→Chat→send/撤回→逐片回執；A/B/C 切片報告 | 按 DR 決定推進、用混合方案或暫停替代，原系統不中斷 |
| C：長任務等價 | Work/等待/排程、四 wire、兩種 Compact、搜索與媒體 | 必看下一正常 Main wire、原付費及長任務，而非僅看短聊速度 |
| D：核心資料與真消費者 | Memory/forget、兩端真插件、桌面/本機/Code 初步跨倉 | 先用真插件見證 B0 即可發布 1.0；其餘能力以獨立候選和有見證的 minor 前進 |
| E：全功能候選與交接 | 包含預期的官方擴展、舊資料 closure、升級/失敗/回退演練 | 只讀 shadow 允許；未接管者列原 owner，不宣告普遍可切 |
| F：全功能相容組，不是首次協議穩定 | 實際插件矩陣、外部桌面檢查、同功能 TCO、版本 BOM | 只有指定組合可標 verified。真部署另行授權，不是任務書交付的一部分 |

**現在沒有非問不可的產品問題。** 採標準 hybrid（可明選純詞法）、沿用多使用者隔離、wire4/Client9 先相容、真憑據只給一個執行服務。實施時需部署者給的是現場事實與授權：目標映像/客戶端、去識別副本或採樣許可、是否核准小額真模型探針、是否准許特定實例切換。推薦預設不探付費、不切現網。若將來要求有新寫後可直接退回 v4 writer，這是另選的反向資料橋工程；未授權前採 K14 的保資料前向修復。

## 7　固定依據與核對入口

| 證據鏈 | 固定源碼、合同及測試 |
|---|---|
| 入口與政策 | [AGENTS.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/AGENTS.md) · [README.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/README.md) · [development-contract.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/development-contract.md) |
| 接納、身份與平台 | [processor.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/services/processor.py) · [person_identity.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/identity/person_identity.py) · [feishu_ws.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/gateway/providers/feishu_ws.py) · [canonical-runtime.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/canonical-runtime.md) · [platform-adapters.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/platform-adapters.md) |
| 唯一執行、等待、排程 | [loop.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/agent_core/loop.py) · [main-agent-runtime.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/main-agent-runtime.md) · [persistent-subagents.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/persistent-subagents.md) · [worker.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/automation/worker.py) · [schedules.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/time/schedules.py) · [provider-output-boundary.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/provider-output-boundary.md) |
| Compact 真值與效果 | [context_compaction.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/services/context_compaction.py) · [conversation-compact.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/conversation-compact.md) · [test_compact_delivery_e2e.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/tests/integration/test_compact_delivery_e2e.py) · [compact-delivery-implementation-20261031.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/operations/compact-delivery-implementation-20261031.md) · [tool-kernel.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/tool-kernel.md) · [tool-results.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/tool-results.md) |
| 查詢、提取、排序與維護 | [chat-history.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/chat-history.md) · [event_extractor.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/memory/event_extractor.py) · [retrieval.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/memory/retrieval.py) · [ranking.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/memory/ranking.py) · [memory-v2.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/memory-v2.md) · [memory-v2-retrieval.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/memory-v2-retrieval.md) · [memory-quality.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/operations/memory-quality.md) |
| Profile、獨立搜索與真費用邊界 | [deepseek_responses.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/llm/deepseek_responses.py) · [structured.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/model_runtime/structured.py) · [native_bridge.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/web/native_bridge.py) · [model-providers.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/model-providers.md) |
| 裝置、插件與實際部署 | [client-api.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/client-api.md) · [admin-api.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/admin-api.md) · [pyproject.toml](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/packages/yuki-agent-connector/pyproject.toml) · [journal.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/packages/yuki-agent-connector/src/yuki_agent_connector/journal.py) · [local-agent-connector.zh-CN.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/operations/local-agent-connector.zh-CN.md) · [persistent-environment.zh-CN.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/operations/persistent-environment.zh-CN.md) · [architecture.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/plugin-development/architecture.md) · [plugin.toml](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/plugins/subscription-monitor/plugin.toml) |
| 原生依賴與性能資料（舊實驗非 Rust 成績） | [build_monty_distribution.sh](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/scripts/build_monty_distribution.sh) · [pyproject.toml](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/pyproject.toml) · [docker-compose.yml](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docker-compose.yml) · [semantic-participation.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/architecture/semantic-participation.md) · [algorithm-performance-implementation-20261026.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/operations/algorithm-performance-implementation-20261026.md) · [participation-latency-implementation-20261103.md](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/operations/participation-latency-implementation-20261103.md) · [test_participation_latency_health_runtime.py](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/tests/integration/test_participation_latency_health_runtime.py) |

外部對照（2026-11-03 查閱，活頁日後可能變化）：[Desktop 的固定 OpenAPI 來源與雜湊](https://github.com/Institute-of-Agent-Life/Yuki-Desktop/blob/bee5692292d8601033798cb2772a075b8db8004b/contracts/source.json)、[Desktop 實際安裝和生命期](https://github.com/Institute-of-Agent-Life/Yuki-Desktop/blob/bee5692292d8601033798cb2772a075b8db8004b/README.md)、[參與庫實際 pin](https://github.com/Institute-of-Agent-Life/Yuki-Semantic-Participation/blob/a6a13d4f04ecc36da26adb5f364abdb4f032fc1e/README.md)、[SnowLuma 群與私聊轉換](https://github.com/SnowLuma/SnowLuma/blob/e44e4ced17c5911b9aa9db449ba29f2d9e567d2a/packages/onebot/src/event-converter/to-message.ts)。Desktop 此固定提交的 `source.json` 指的是主項目 `004ae36dbe26046d3946b78934c4766333831bcf`（Client 9），不是 `329ca7c`；因此必須比實際 OpenAPI 和操作，不能只比 revision 數字。

官方協議：[DeepSeek Responses 能力表](https://api-docs.deepseek.com/guides/responses_api/)、[Claude 手動 thinking 與工具選擇](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)、[Gemini GenerateContent 的原 Part 簽名](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures)、[OpenAI 函式呼叫](https://developers.openai.com/api/docs/guides/function-calling)、[ACP v1 的協商與 session/load](https://agentclientprotocol.com/protocol/v1/session-setup)、[Telegram getUpdates](https://core.telegram.org/bots/api#getupdates)、[SQLite 一致備份](https://www.sqlite.org/backup.html)、[SQLite CREATE INDEX / UNIQUE](https://sqlite.org/lang_createindex.html)。上述文件證明相應公開語義，不替代實際路由/模型/硬體測試。

## 8　來源稽核與發版適用範圍

已完成部分的盤點與未讀缺件見[覆蓋表](全量文檔覆蓋與裁決表.md)，依賴反驗、舊 CI 原件指紋、分類行數的重跑方法見[核對說明](audit/README.md)。未讀的歷史文件不阻塞無關的 B0 協議；要宣稱取代某部署的所有現用能力，需先用該部署的版本及 owner 清單對賬。沒有原件的舊性能數字不得列為新版收益。

### 同一個 commit 裡，文件也可能描述三個不同版本

| 文檔衝突 | 用哪個事實裁決 | 施工約束 |
|---|---|---|
| CHANGELOG 的早期 4.0 截點寫 0143／Plugin API 7；中、英文 README 還寫 0155／Connector 0.4.2；架構入口寫 0156 | 同 commit 的 [0156 遷移](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/migrations/versions/0156_active_initiative_reconcile.py) 與 [Connector 套件描述](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/packages/yuki-agent-connector/pyproject.toml)，另核 CI 原包。0156 是既有活動範圍的部分索引；Connector 實物是 0.4.3、wire 仍為 4 | CORE-003 分別記部署 DB 實際 revision、映像、套件與 consumer 對應表。舊文件日期、推送或新倉名稱都不代表已有部署。 |
| 舊 [SnowLuma 指南](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/docs/deployment/snowluma.md) 寫 `/ai on`；現行群接入入口核 `ADMIN` 及 `current/enable` | [group_recovery.py 的實際 command、connection、enable](https://github.com/YuanYeYouTao/Yuki/blob/329ca7c487414d2f54ddd98ff4727446ed2446cd/src/yuki/services/group_recovery.py#L36-L117)；先核原 native 群與授權，不借本機客戶端或另一台 Bot 的路由 | CORE-056 使用 `/ai admin group current enable`，EXT-068/069 各驗自己的平台證明；不因舊文加入新別名，不以成員探針取代真發送回執。 |
| 1.9 合成 prompt 報告以 Planner 動態縮減 schema；9 月的部署記錄含 Rollup、舊 MCP 或工作區快照 | 行為基線改以固定聲明、Main 的 P/E Compact、單一受權操作與原回執為準。歷史診斷沒有實際 Provider 發票，舊部署不是新切換授權 | CORE-015/022/023/046 保留已確認的工具、觀察與成本，拒絕用不同工具暴露面或更弱隔離製造「加速」。 |

**可重跑的部分規模。** 舊 Host wheel 有 679 個本體 Python 檔、179,375 實體行，插件 SDK 另 18 檔、2,147 行；Connector sdist 的 16 個執行檔、3 個腳本、20 個測試檔分別為 5,693、336、5,930 行。[逐檔明細](audit/results/package-loc.tsv)與[範圍說明](audit/README.md)區分打包來源、全庫未知以及第三方二進位。全倉遷移、生成碼、第三方原件、全部測試與 Rust 新版 LOC **未知**，不拿打包子集外推。
