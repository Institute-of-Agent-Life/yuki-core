# 核心實作進度

CORE-023 CI 補修：完整原工具宣告已超過舊 fixture 首輪上限；明示 64,000 容量並放大必要未觀察輪，保留硬上限拒絕並加驗恰好一次物理 attempt。Windows／Mac／Linux 全卡通過；詳見 [故障與修正證據](compact-capacity-fixture.md)。

任務書 v3.2 ZIP SHA-256：`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。舊倉只讀，行為基線 `329ca7c487414d2f54ddd98ff4727446ed2446cd`；K05 補正已套用工作副本，不增加 unknown 重試權。原提交、驗證、失敗診斷及完整交接雜湊見 [實作驗證記錄](implementation-evidence.md)。

CORE-037：修正歷史資料庫 fixture 的逐條落盤；[HJW、Mac、Linux 各 1052 項及超時診斷](control-fixture-timing.md)。

PR #71：Windows CI 的 Rust SDK 測試讀取完整 JSON 時，寫入端可能尚未關檔。兩個 SDK 的測試現在先關閉暫存檔，再原子交給獨立程序；逾時會記錄卡住的方法與程序狀態。Mac、HJW 同一工作樹 d839315809be 各通過 CORE-037 全部 1052 項。回條：`artifacts/CORE-037/client-stream-atomic-inbox{,-hjw}-01/manifest.json`。

PR #51 已整合前置；Mac 037=977、048=663、039=84 項通過，回條在各卡 `control-integrated-parent-nov06-03/manifest.json`。

PR #44 已整合前置；Mac 005=36、050=561 項通過，回條在各卡 `aux-model-parent-nov06-03/manifest.json`。

來源：任務書 v3.2 核心文檔補核版，ZIP SHA-256
`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。
舊行為基線：`329ca7c487414d2f54ddd98ff4727446ed2446cd`，舊倉只讀。
已依總書 K05 修正工作副本 CORE-011 第 2 步，不增加 unknown 重試權。

## 完成與進行中

已合併 main：CORE-001～027、031～033、039、049、052、056 的原卡實作；CORE-016 分段 #42 已合併至 `ce66753`。後續原工具相容性補核見下表；實作與離線驗證不替代真擴展消費或實際供應商驗收。

| 任務 | 目前實作／PR | 狀態與驗證 |
|---|---|---|
| CORE-028、029、030 | #35 `3765d21`、#37 `87a07f9`、#39 `b745a9d` | 向量、檢索、忘記已實作及產證；等待前置與最新六項 CI。 |
| CORE-038 | #40 `7cd4e42` | 媒體 broker 已實作；候選及 EXT-038 實測證據見記錄。 |
| CORE-016 分段、034 Code | #42 `e542c27`、#43 `9258e35` | 本地三平台候選已驗；真 Monty 原 feed 補修需擴展再測。 |
| CORE-050、036、035 | #44 `c26e6c4`、#45 `8368392`、#46 `402a9aa` | 輔助模型、參與判斷、維護合同已實作；待前置及 CI。 |
| CORE-051、058、048 | #47 `671d8c5`、#48 `f5f961b`、#49 `8915b3f` | 隔離會話、通知、Profile 已驗；未知通知不重送。 |
| CORE-037、055、057 | #51 `9bb02fc`、#52 `e087e88`、#54 `b63d111` | 管理、用量、離線模型移轉已實作；三平台候選見下表。 |
| CORE-014、015 補核 | #53 `77c85c4`、#56 `bc551a0` | 原歷史查詢、Artifact 模型合同已驗；Code peer 整合回歸 387 項通過。 |
| CORE-013 補核 | #57 `bf121e5`、#58 `47fae2a` | 原群目錄與撤回；撤回來源 `266e9aa` 三平台各 252 項通過。 |
| CORE-037 權限目錄 | #59 `a638fab`，`codex/core-037-permission-directory` | 247 個原 descriptor、16 組結果；Mac／Linux／HJW Windows 乾淨候選各 1052 項通過，索引見下表。 |
| CORE-015 工具視圖 | PR #60 `cf14cc6`，`codex/core-015-original-tool-views` | 原宣告、Code 核心可見性、lookup 多詞搜尋與回條補核；50 組原對拍；三平台乾淨候選各 127 項，五張相鄰回歸通過。 |
| CORE-015 逐項拒絕 | #61 `969fb62` | Windows／Linux／Mac 各 363 斷言，八張相鄰卡及固定原版四項測試通過；等待前置與最新六項 CI。 |
| CORE-026 原事實恢復 | #62，`806d160`／候選 `d107b18` | Windows／Linux／Mac 各 226 項、六張相鄰卡及原版 7 項通過；GitHub 重試推送已成功，等待最新六項 CI。 |
| CORE-026 metadata | PR #63，`f5855e5` | Windows／Linux／Mac 乾淨候選各 370 項；9 張相鄰卡、原版 7 項與逐 ZIP 成員檢查通過；reader 44，尚未發布。 |
| CORE-026 selector | PR #64，`fed00d2` | Windows／Linux／Mac 乾淨候選各 823 項；原版 9 組、5 張相鄰卡及逐 ZIP 檢查通過，尚未發布。 |
| CORE-026 有效期間 | PR #65，`6d47464`，`codex/core-026-memory-temporal` | Windows／Linux／Mac 各 1181 項；7 張相鄰卡、72 組原解析、8 步原修改及逐 ZIP 檢查通過。reader 45，尚未發布。 |
| CORE-033 未選 Work 認證 | PR #66，`3e4080d`，`codex/core-033-connector-auth` | 三平台各 369 項、原版 8 項與 6 張相鄰卡通過。首次 HJW 打包因 Windows 執行檔鎖失敗；清理修正後三平台乾淨候選各 369 項且 ZIP 成員已驗。reader 46，SDK alpha.12／a12。 |
| CORE-033 Client9 明選 | PR #67，`389d586`，`codex/core-033-entry-selection` | A→B／既有 ACP、舊 NULL、原 Work mailbox 與 Host 授權；[方法與失敗時序](client-entry-selection.md)。reader 47；乾淨 Windows／Mac／Linux 各 558 項、7 張相鄰卡通過。 |
| CORE-033 Client9 出站串流 | PR #71，來源 d8bf54c | [提交提示、快照與按事件續讀](client-stream.md)。Mac、HJW Windows、Linux 各 581 項，候選索引見下。 |
| CORE-033 Client9 上傳 | codex/core-033-client-uploads | [回執與有序附件](client-upload-receipts.md)。PR #72，來源 c616cf3；Windows／macOS／Linux 各 666 項、原版 15 項與 7 張相鄰卡通過。 |
| CORE-038／025 原 gateway 取件 | codex/core-038-inbound-media | [原 Bot、app 與租戶取件](gateway-inbound-media.md)；SDK alpha.13／a13，reader 49。Mac 207 項、固定原版 18 項及 7 張相鄰卡通過；待三平台交接與 EXT-004／006。 |
| CORE-040 | #55 `400c9b8`，部分完成 | 12 次冷暖 A/B/C 各 1200 輸入已逐筆核效果；完整工具／DTO／Compact 等價與受控資源比較未完成，不判 Rust 勝出。 |

每個 PR 均等其前置合併與最新 head 的 push／PR × Windows／Linux／Mac 六項檢查全通過才合併。待合併分支以 PR 的 head ref 為準；本地 worktree 在 `.cache/worktrees/`。不直接提交 main。

## 可供擴展倉只讀消費

除 initialize 預發布外，候選均未發布。索引含各 ZIP 絕對路徑、來源 commit/tree、reader／Host／SDK 版本、實含能力、逐成員 SHA-256 及三平台驗證。不同候選驗證的能力不可拼成一次全棧驗收。

| 候選 | 來源 commit | 索引 | 索引 SHA-256 |
|---|---|---|---|
| CORE-013 | `266e9aa23e13` | [recall-handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-013/recall-handoff-final-01/verified-candidates.json) | `d7b86f0244ca8064e13e67cd78ad96256d9879bd35444bd4b68a2add1e73ebe2` |
| CORE-014 | `43b67d35d996` | [handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-014/handoff-final-01/verified-candidates.json) | `c8d373992ec7e286337a4d79deb2caec81e79ee8fca5c22a3cd1f1330edc7c4c` |
| CORE-015 | `cf14cc6d45b7` | [views-handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-015/views-handoff-final-01/verified-candidates.json) | `302ade325eed2927def6135cb9ba3047e150d48f6d20971b6a0b5bd939053170` |
| CORE-037 | `a638fab4066b` | [permissions-handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-037/permissions-handoff-final-01/verified-candidates.json) | `e485af6cee3c586469e5665a3eef61b2496bce238ebd47d48934578f1b5d8ca3` |
| CORE-055 | `214197a1a8d3` | [handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-055/handoff-final-01/verified-candidates.json) | `cda5ff398b353784f013fef7d39e7b72d1d9a219a834c18846db8672a53b26f5` |
| CORE-057 | `4dfb1c026a7f` | [handoff-final-02](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-057/handoff-final-02/verified-candidates.json) | `6a1c274e4002cd0b96b43870a80c5a478af997143f0df31ab81fe2cf09bab2b7` |

CORE-015 逐項拒絕候選：[isolation-handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-015/isolation-handoff-final-01/verified-candidates.json)；索引 SHA-256 `1e2e368f0f2996e024f3d0afba92c9fee199af4781e6de3cff4482932cf7614b`。來源 `969fb62`，三平台各 363 項；原 batch 各 call 單獨拒絕，六協議均保留合法後續效果；尚未發布。

CORE-026 原事實恢復候選：[restore-handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-026/restore-handoff-final-01/verified-candidates.json)；索引 SHA-256 `9778d445446d3b69d9118e2860441da220dc7f071c05142320029419d377f298`。來源 `d107b18`，三平台各 226 項，已驗原 fact 版本、實際 QQ／Main、FTS、冷啟與忘記後拒絕。尚未發布。

CORE-026 metadata 候選：[metadata-handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-026/metadata-handoff-final-01/verified-candidates.json)；索引 SHA-256 `5282ee47d50e31f8877683850ecb654c79c33a5223dd8b6c5660f4a8054ef4a9`。來源 `f5855e5`，三平台各 370 項；含 update_metadata、reader 43→44 原回執升級、歷史清除及完整 QQ 分段。尚未發布。

CORE-026 selector 候選：[selectors-handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-026/selectors-handoff-final-01/verified-candidates.json)；索引 SHA-256 `bb483640d365bfd755831c8ef53910aa44db125d76685737f9f4154438c62c56`。來源 `fed00d2`，三平台各 823 項；含本人精確定位、第 129 筆分頁、歧義候選來源依賴與原恢復。尚未發布。

CORE-026 有效期間候選：[temporal-handoff-final-01](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-026/temporal-handoff-final-01/verified-candidates.json)；索引 SHA-256 `fe110e66e41cc41ea0155ea81c2ba82207610fa224f2053f865e4f580d943c13`。來源 `6d47464`，三平台各 1181 項；原期間解析、替換規則、reader 44→45 清除及原回執已驗。尚未發布。

CORE-037 權限目錄候選根目錄：`/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-037/permissions-handoff-final-01/`，來源 `a638fab4066b2913a2d4f00171ba0b4fe78a5e83`、tree `61a8622b8c3f84b88743b096e36804aed5a4f22ce2164ba16600654db2c2181f`；三平台 ZIP 與逐成員雜湊已驗，Host 2／reader 43／SDK alpha.11、a11；候選尚未發布。其他 Host／Code／媒體／Profile／分發候選的固定路徑與簽章見 [實作驗證記錄](implementation-evidence.md)。

唯一已授權並完成的發布：[handshake-preview-dd694a0db605](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/handshake-preview-dd694a0db605)，僅 initialize；ZIP SHA-256 `6e2ca1040f6d1eac2f70b0ff363887faf662782f72a7acd367d4a52a17b2caa2`。EXT-002 兩 SDK Windows 消費已通過。

CORE-033 未選 Work 認證：[乾淨三平台候選](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-033/connector-auth-handoff-final-02/verified-candidates.json)，來源 `c85a00b`、索引 SHA-256 `750e7f878cfe0749dc01c0602a0d047c612554ab3e661dfa0e4dfbf85cea729e`。Host 2、reader 46、SDK alpha.12／a12；僅本地未發布候選。

CORE-033 Client9 明選：[三平台索引](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-033/entry-selection-handoff-final-01/verified-candidates.json)，SHA-256 `11123ac0502c2f86a74dc31c52ca81f05e50b6cd0b561db1377d86853650e080`；含所選人物、binding、會話、事件與拒絕前後計數。未發布，待 EXT-015 對拍。

CORE-033 Client9 出站續讀：[三平台索引](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-033/client-stream-handoff-final-01/verified-candidates.json)，SHA-256 739ecae7768f860e4d7a4e9801a5d7c51e38eac623e155a88cacd3e6f8cd08c1。來源 d8bf54c1174741a23fdfad0d99b516a2bb584218；Windows、macOS、Linux 各通過 581 項檢查；未發布，待 EXT-015。

PR #72 已整合原版客戶端對拍及 SDK 測試修正 `df3670b`。Mac CORE-033 666 項、CORE-037 1052 項通過：各卡 `uploads-independent-client-01/manifest.json`。

CORE-033／真 EXT-015：HJW 以原插件 `5b890f5e8244` 消費 `d8bf54c11747`。配對、一次 QQ 派發後的 REST 游標續讀及撤權 401 通過。直連宿主收到 SSE 初始同步、ACK 提示及重連；同一宿主及憑證經原插件的首訂閱／重連各 10 秒均未收到 HTTP 標頭，明選標頭由該插件回 503。SSE 跨倉驗收未過。[原始回條](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-033/client-stream-real-ext015-02/real-client-stream.json) SHA-256 `f11cfe5de0a657ff373a09a429111f23224beb255ce63736718a1f958ba6a664`。[重現程式](../ci/consume-ext015.py) 對照兩端，回條分項記錄，請 EXT-015 核對代理的首包刷新及明選轉發。

CORE-033 Client9 上傳：[三平台完整性索引](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-033/client-uploads-handoff-final-01/verified-candidates.json)，SHA-256 4da11e14c4118f087c6535de7b973d60b195cb98eade2ed3a45b4d2404f2900c。未發布，待 EXT-015／016。

CORE-038 入站附件：來源 `b99b01a92443`，Windows、macOS、Linux 各通過 207 項。[候選包與逐平台 SHA-256](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-038/inbound-gateway-handoff-final-01/verified-candidates.json)，索引 SHA-256 `ad9f55a0434d54e3b8adc74c26f1e0a29d699621588eed9e668b5c48764585bb`。SDK alpha.13／a13、reader 49，供 EXT-004／006 重放原收件與過期案例；尚未發布。

## 缺少輸入與下一步

- 真擴展消費：EXT-023 需對已修的原 feed ACK 再測；EXT-022／037／038／039／049 需消費最新管理、權限與用量候選。未取得回條，不凍結穩定協議。
- 外部能力：EXT-004／006 附件授權與 fetch、EXT-031 vision slot/version/bytes、EXT-020 gVisor/execd/PTY 旋轉、EXT-015 agent_target/附件/SSE、SnowLuma 映像 digest 與雙向包仍待真實方法、版本或證據。
- 安裝與切換：CORE-043 等 EXT-041 與核心分發／移轉輸入；044 等 EXT-067 B0 實測；045 等 EXT-043 已發布 tuple 與 044；046 等 EXT-044／045 安裝證據；047、053、054 依各前置。CORE-041／042 等 CORE-040 等價切片判斷；不把未滿足前置的卡標完成。
- 下一個可獨立工作：有效期間乾淨候選已完成；`codex/core-033-connector-auth` 已實作 EXT-011 明確要求的未選 Work 裝置認證，三平台候選已驗，待 EXT-011 對拍。這是核心生產者應補的入口，不列為等待擴展提供的方法。Client9 明選入口／會話已產出候選；接續原 Social Outbound 斷線續讀、附件與 send／workspace 的本倉責任；CORE-040 仍需完整等功能切片。擴展倉保持只讀。

- 下一個可獨立工作：有效期間乾淨候選已完成；`codex/core-033-connector-auth` 已實作 EXT-011 明確要求的未選 Work 裝置認證，三平台候選已驗，待 EXT-011 對拍。這是核心生產者應補的入口，不列為等待擴展提供的方法。其後補原 Client9 明選入口／會話及 send／workspace 的本倉責任；CORE-040 仍需完整等功能切片。擴展倉保持只讀。

- 下一個可獨立工作：有效期間乾淨候選已完成；`codex/core-033-connector-auth` 已開始處理 EXT-011 明確要求的未選 Work 裝置認證，原版 8 項身份測試通過。這是核心生產者應補的入口，不列為等待擴展提供的方法。其後補原 Client9 明選入口／會話及 send／workspace 的本倉責任；CORE-040 仍需完整等功能切片。擴展倉保持只讀。

- 下一個可獨立工作：metadata 候選已完成；selector 候選亦已完成；在 `codex/core-026-memory-temporal` 補 valid_from／valid_until 的原版語義，再續其他狀態、send／workspace 參數的本倉責任；推進 CORE-040 完整原切片與冷暖對照。擴展倉保持只讀。

- 下一個可獨立工作：metadata 候選已完成，繼續補原 memory_change 的 selector／其他狀態、send／workspace 參數的本倉責任；推進 CORE-040 完整原切片與冷暖對照。擴展倉保持只讀。

- 下一個可獨立工作：繼續補原 memory_change 的 metadata／selector／其他狀態、send／workspace 參數的本倉責任；推進 CORE-040 完整原切片與冷暖對照。擴展倉保持只讀。

- 下一個可獨立工作：補原 memory_change、send／workspace 參數的本倉責任；推進 CORE-040 完整原切片與冷暖對照。擴展倉保持只讀。

- 下一個可獨立工作：完成 CORE-015 模型批內 schema 錯誤逐項續跑的回歸及三平台候選，再補原 memory_change、send／workspace 參數的本倉責任；推進 CORE-040 完整原切片與冷暖對照。擴展倉保持只讀。

- 下一個可獨立工作：完成 CORE-015 工具視圖三平台驗證與候選，再補模型批內 schema 錯誤逐項續跑及原 memory_change、send／workspace 參數的本倉責任；推進 CORE-040 完整原切片與冷暖對照。擴展倉保持只讀。

**CORE-032 開發候選，尚未發布。** 來源 `199dff095be0bc87c9713beaa0419400cb424a7c` 已推 [PR #24](https://github.com/Institute-of-Agent-Life/yuki-core/pull/24)。握手 revision 1，業務 revision 2，業務 schema SHA-256 `4822f19fb937eba524e475b35c0753a713587f48feee87ae6bd7b53ccab06cba`。雙 SDK、宿主執行檔及方法清單均在包內。兩端各通過 65 項及隔離 PATH 啟動，逐成員雜湊已複驗。
- HJW Windows x86_64：`/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-032/handoff-199dff095be0/windows-validation/handoff/yuki-host-preview-199dff095be0-windows-x86_64.zip`，SHA-256 `d3ef0f3ce8a7dedd175b6aa3392a22c744504cf6bea70eadc22d5408642e634c`。
- Mac arm64：`/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-032/handoff-199dff095be0/macos-arm64/yuki-host-preview-199dff095be0-darwin-arm64.zip`，SHA-256 `07592cb449433512dcaf55306bfddbc8a6b3f93d025f15d7f51da59e844a5e9d`。

CORE-020 已經合併 `00e1f9f`，見 [PR #21](https://github.com/Institute-of-Agent-Life/yuki-core/pull/21)。CORE-016 已推 [PR #26](https://github.com/Institute-of-Agent-Life/yuki-core/pull/26)；CORE-026 已推 [PR #27](https://github.com/Institute-of-Agent-Life/yuki-core/pull/27)。CORE-039 的候選尚未發布。

**CORE-039 簽署候選，尚未發布。** 來源 `093716ad01c0a0ce1f9364e7af26f97c7ad5bdc9`，握手 v1、Host 業務 schema v2，Rust SDK `1.0.0-alpha.2`、Python SDK `1.0.0a2`。三系統各 81 項；三個 release ZIP 及 Linux OCI 已在交接端獨立按下列 SHA、公鑰及來源重驗。source、reader、實含方法、逐 SDK digest 和驗證回條見各包 candidate.json；OCI 另有簽署的 oci-candidate.json。見 [PR #28](https://github.com/Institute-of-Agent-Life/yuki-core/pull/28)。

交接根目錄 `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-039/handoff-093716ad01c0`；fixture SHA-256 `2d986f7c2bf30d597fbf667f346d189a8aad9bf6194d1341fc13620e4907316e`。每包的 `signer.pub` 在其同一目錄，各公鑰檔雜湊獨立列出。

- `linux-evidence/release-host/yuki-core-093716ad01c0-linux-arm64.zip`，SHA-256 `526504d07c90cabff1c30373fed11465478b2ce451396d11ce1554c0801e1c2e`，公鑰檔 SHA-256 `d29f93317c1fb9d70f8b9c428f3f631ca8d3ab91864022dd195c261de6c8ebc4`。
- `macos-arm64/yuki-core-093716ad01c0-darwin-arm64.zip`，SHA-256 `e866d639de3b6127823df897ed200bfc3bcdd11883828fba5d54f5af029e2ff2`，公鑰檔 SHA-256 `6cac9d04cdb668e1ed385c27f2ff5e58753a49825f8e634765f4416543bfd3e0`。
- `windows-evidence/release-host/yuki-core-093716ad01c0-windows-x86_64.zip`，SHA-256 `03989033a13260cb863d1ff81e2407f0593d43c7a6c0779704ca9eb2c6b1a871`，公鑰檔 SHA-256 `86d9652f0bb90fcf0204869425d0313024c62c40ba3a49f170ce30a34207b7ef`。
- `linux-oci/yuki-core-093716ad01c0-linux-arm64.oci.tar`，SHA-256 `b5975cbc593594c6b55222e7ae1fd8ac9a2bbb3bd147ee1081438df04c708ec9`，公鑰檔 SHA-256 `dfc60f5213b47f244a9cdc673d65ddc2f98e940d9c67768a19400bed9ef77ecb`。

CORE-016 實作 `d4fcafc`、整合 head `4213703`，已推 [PR #26](https://github.com/Institute-of-Agent-Life/yuki-core/pull/26)。CORE-017 已通過 Windows 與 macOS 等待測試，CORE-018 可開始實作。CORE-027 可從本卡原事件證據與寫入合同開始。CORE-040 的本機三臂均逐筆通過 1,200 次 QQ 送達；原工具合同與群聊投影仍有差異，見 `artifacts/CORE-040/qq-1200-01/slice-decision.json`。
CORE-012 提示詞補核在 `codex/core-012-prompt-order`。原版 HTTP 錄製顯示角色身份、SOUL、執行程式的先後順序。修正後 Mac 與 HJW Windows 各通過 163 項，見 `artifacts/CORE-012/{prefix-mac-01,prefix-windows-01}/main-wire.json`。CORE-014 已合併 [PR #20](https://github.com/Institute-of-Agent-Life/yuki-core/pull/20)。

CORE-014 補核發現原版長 Work 在續接時呼叫 `find_contacts`。已於 `codex/core-014-contacts` 補原名稱、原參數 schema、單一人物的多帳號綁定、Person／Space keyset，以及分開的 `find_conversations`。讀取沿原 caller 和來源交集，發布前重查並綁真實來源；舊人物與會話目錄入口仍可用。固定 v4 八項對拍在 `artifacts/CORE-014/contacts-v4-01/`，正常 Main 的原 wire 在 `contacts-mac-04/original-contact-directory.json`。此修正供 CORE-040 原長 Work 消費，不以替代工具替換原劇本。

聯絡人補核最終來源樹 `62ca2f8c48e6cf303e74cb780ebf47c4dd608182b04acb3e77b81d97319e7a53`，Mac 與 HJW Windows 各 142 項通過，產物在 `artifacts/CORE-014/{contacts-mac-06,contacts-windows-02}/original-contact-directory.json`。CORE-005、007、015、052 的 `contacts-regression-02` 均通過，另留 13 張相鄰卡的 `contacts-regression-01`。包含原 UUID 大小寫別名升級、現有來源的發布前重查與目錄來源依賴。下一步將此版本供 CORE-040 原長 Work 使用。
CORE-024 在 `codex/core-024-search` 實作獨立原生搜索。Responses、Messages 及 Gemini 都用實際 loopback HTTP 驗證來源；一般 Main 透過自己的工具宣告派發，原聊天未送入獨立連線。冷重啟仍可命中完整且未過期的原收據，改 connection、Profile、金鑰域或憑據會重新派發。日期不明、孤立來源和未完成回應分開處理，原邏輯的未知嘗試重啟後維持一次。方法和來源證據在 `artifacts/CORE-024/{mac-07,windows-01}/search-proof-cache.json`；固定舊版橋接 24 項測試在 `artifacts/CORE-024/v4-oracle-01/junit.xml`。EXT-029 尚需以它的真實 Tavily owner 消費核心合同。
兩機各 377 項斷言，驗證來源 `de95a208c5a3464a46e50105baf3801f00f32cd86e2c2072b8350974a8243d80`；10 張相鄰卡的回歸位於各卡的 `native-search-regression-01/`。

CORE-024 PR #38 已以 `905e968c5a04187944bf8f0ff4a3667652221731` 合併 main。此分支同步凍結原生搜尋與聯絡人工具，整合來源另外重驗。

聯絡人分支同步 main 搜尋後，CORE-014 的 `search-main-integration-01` 142 項、CORE-024 的 `contacts-main-integration-01` 377 項、CORE-015 的 `search-contacts-integration-01` 均通過。此合併供 PR #41 的新來源 CI 使用。

CORE-017 實作 `8a41277`，HJW 與 Mac 各通過 106 項，已推 [PR #29](https://github.com/Institute-of-Agent-Life/yuki-core/pull/29)。CORE-018 與 CORE-023 可分別使用等待與公共整理的本地驗證結果。CORE-040 在 HJW Windows 另跑 A/B/C 各 1,200 筆；收件端、正文、回應原因及出站筆數相符。三臂各錄到 2,400 筆物理模型請求。中間報告 `artifacts/CORE-040/windows-1200-02/report/slice-decision.json` SHA-256 `22f5813d116dc164afcaf8e829e410a645a0d264b7e31d91d15d211f29c48cdf`，工具與群聊輸入仍有差異。

CORE-022 實作 `cc1a95e` 已推 [PR #30](https://github.com/Institute-of-Agent-Life/yuki-core/pull/30)。

CORE-018 實作 `49a3099` 已推 [PR #31](https://github.com/Institute-of-Agent-Life/yuki-core/pull/31)。Mac 與 HJW Windows 各通過 101 項。
CORE-021 的 [PR #23](https://github.com/Institute-of-Agent-Life/yuki-core/pull/23) head `23fc3b2` 三平台六項檢查通過，已合併 `7fd1002`。CORE-030 還需要排程保存與其他卡輸入，按其原前置核對。

CORE-023 實作 `755db0e`，[PR #32](https://github.com/Institute-of-Agent-Life/yuki-core/pull/32)。CORE-030 現已取得等待、排程及 E 的本地核心執行器。


**擴展對拍。** HJW 的以下回條以擴展倉固定提交重新下載並核對 SHA-256；前三項使用 CORE-032 來源 `199dff095be0`。SDK 與宿主均來自候選包。

| 消費者 | 固定回條 | SHA-256 |
|---|---|---|
| EXT-003 Telegram 遊標與重放 | [telegram-offset.json](https://github.com/Institute-of-Agent-Life/yuki-extensions/blob/03cabaa6f53db966888e3bf4d62a2113436482ec/artifacts/EXT-003/candidate-03/telegram-offset.json) | `a81f960527f3d5e959fef3003af13a8539f4174c9d35af91d1b5855e833f9338` |
| EXT-028 web-fetch 來源及工具分頁 | [fetch-source.json](https://github.com/Institute-of-Agent-Life/yuki-extensions/blob/f8aac847ba3c6807b7ff2574fc80ff4f6f98ad30/artifacts/EXT-028/candidate-05/fetch-source.json) | `e1f5569fd35045fadd0729b0222e229eef21bcbf798fb821ac086fbc539691d9` |
| EXT-005 飛書持久 ACK | [lark-frames.json](https://github.com/Institute-of-Agent-Life/yuki-extensions/blob/a9851426968903270e2f0c1ff87b704714f25d65/artifacts/EXT-005/candidate-01/lark-frames.json) | `55fb682c455fc5f43349d87d9c7c6c3c760d2136579d8c6e92738ae754dce9a3` |
| EXT-040 離線簽名安裝、啟動與選配 web-fetch；來源 `093716ad01c0` | [setup-install.json](https://github.com/Institute-of-Agent-Life/yuki-extensions/blob/df09742e211a08288172fc18b234b104e5334ab5/artifacts/EXT-040/candidate-06/setup-install.json) | `ec6d75176646ddcebae6a22792f4e0a95c22669ffd6e146dd17dd219cc75fbd8` |

CORE-025 提供 CORE-033、038 所需的本地版本合同。CORE-028 已開始測試原 embedding 的文件與查詢請求。穩定面仍待 EXT-067 公開包與方法級證據。
CORE-028 可消費已提交的事實版本；CORE-040 的舊版 A、B 各通過 20 組雙設備／兩次冷啟動的長 Work，每組六筆實際模型 HTTP 請求。原始回條在 `artifacts/CORE-040/{A,B}-long-20-01/`。C 的同一劇本仍需要 CORE-025 及 CORE-033。

CORE-027 實作 `b755907`，已推 [PR #33](https://github.com/Institute-of-Agent-Life/yuki-core/pull/33)。

CORE-028 的文件與查詢範本、OpenAI-compatible 與 Qwen 原始請求位於 `fixtures/embedding/v4-wire.json`。CORE-029 下一步按授權集合與固定舊版對拍排序及費用。
CORE-025 實作 `d40d4f5`，見 [PR #34](https://github.com/Institute-of-Agent-Life/yuki-core/pull/34)。在同一份測試來源上，HJW 另跑 CORE-013，見 `artifacts/CORE-013/media-windows-02/social-parts.json`。

CORE-028 實作 `1dd86c1` 見 [PR #35](https://github.com/Institute-of-Agent-Life/yuki-core/pull/35)。

CORE-029 在 `codex/core-029-retrieval` 完成授權集合內的詞法／向量排序及 Main 的 `search_memory`。Mac 與 HJW 各通過 71 項斷言，見 `artifacts/CORE-029/{mac-06,windows-03}/authorized-topk.json`，包含原來源在查詢途中被擦除及跨 Person 拒絕。相鄰 25 張卡通過 `retrieval-regression-{01,02}`；031 首次揭露主機 NOTICE 影響既有 SDK 封裝，修正後重驗通過。固定 v4 Vec1 對照同一 Rust owner 的精確與 Vec1 路徑，36/36 組排序一致，原始資料和摘要位於 `bench/retrieval/decision.json` 及同目錄的兩份壓縮證據。CORE-030 可沿該檢索路徑補 source 清除，CORE-035 可使用授權內的 fact 結果。此卡不新增公開協議版本。
最終 Mac `artifacts/CORE-029/mac-07/` 與 HJW `artifacts/CORE-029/windows-04/` 各核對兩份原始 DV 壓縮報告；兩機驗證來源 `63f05cc5555f426a0db8255a3e46929164c674e0c69bd7092b8c83a8d64aac15`。另以 Git Windows 換行設定核對 Vec1 原件逐位元相同。
CORE-033 的固定規格索引為 `protocol/personal/source.json`、`protocol/connector/4/source.json` 及 `protocol/plugin/personal/methods.json`。握手 1，Host 2，個人方法組 1，Rust SDK `1.0.0-alpha.3`、Python `1.0.0a3`。固定來源候選供 EXT-011／015 發展對拍，正式擴展驗收仍待其真包。


CORE-033 候選已驗證，尚未發布。來源 `994aee20ced762d4132186144e718d8946321347`，見 [PR #36](https://github.com/Institute-of-Agent-Life/yuki-core/pull/36)。每包均有實測執行檔、Client 9 及 Connector 4 固定規格、兩種 SDK、6 個個人方法與 20 條已驗 HTTP 路由清單。三平台各 187 項通過，解包後逐項核對 SHA-256。Linux 在斷網容器執行。本候選可驗個人與 Work 交接；已交付檔案的個人下載路由另行補驗。

| 平台 | 候選包 | ZIP SHA-256 |
|---|---|---|
| Windows x86_64 | `artifacts/CORE-033/handoff-994aee20ced7/windows-validation/handoff/yuki-personal-preview-994aee20ced7-windows-x86_64.zip` | `ddc2cf1c01dd9df9e0eecf6b1b85984176810b8c763a4b7bd493d924e40a53f8` |
| Mac arm64 | `artifacts/CORE-033/handoff-994aee20ced7/macos-arm64/yuki-personal-preview-994aee20ced7-darwin-arm64.zip` | `9b7a014334727466d14fe9add6e0d83fc17b0641eb120f6bf5e33afff2d51ea2` |
| Linux arm64 | `artifacts/CORE-033/handoff-994aee20ced7/linux-arm64/candidate/yuki-personal-preview-994aee20ced7-linux-arm64.zip` | `91da3ff307e1f908e82ee00472c41d1c74ce9d2da998001e17ecaed483f8ce8c` |

握手 revision 1、Host revision 2、個人方法組 revision 1。個人 schema SHA-256 `061b40a245ff7a75bc747684760d7964cae5fa7f685dab855ff96a5458682827`，組合 Host schema `9feab6196953ef66a04a745b801b2bf528564cfc0d9d0ff545a81e522624f42e`。Rust `1.0.0-alpha.3`、Python `1.0.0a3`。

CORE-033 已補上 Client9 原檔目錄、依 `artifact_id + version` 下載及 Work 結果引用。Mac 與 HJW 各 256 項通過，來源 `6701c44804cfebf01bff4a98463265a538fcfba6c00ce8a52abb6b0d8d53cc38`，原件在 `artifacts/CORE-033/{mac-download-05,windows-download-01}/personal-handoff.json`。同一 Work 經兩台設備、換執行及冷啟動後仍交付原始位元；另一個 Work、錯版、未派發、未收到 ACK 與 forget 後的請求均拒絕。17 張相鄰卡的回歸見各卡 `personal-download-regression-01/`。前列 `994aee20ced7` 候選未包含這兩個下載路由，更新包另以新來源識別。

CORE-030 在 `codex/core-030-forget` 增補同人族與歷史私聊清除、原管理路由、可續 GC，以及有待清義務時的獨立插件清除入口。Mac `artifacts/CORE-030/mac-11/forget-canaries.json` 已驗 115 項；先前 HJW `windows-02` 驗過同一核心清除劇本，後續 raw-method gate 補強會隨新來源重驗。固定 v4 七項測試在 `artifacts/CORE-030/v4-oracle-01/`。25 張相鄰卡通過各卡 `family-forget-01/`，包括 ASR、附件、兩種 Compact、提取、向量、歷史及 Host 的實際遲到來源斷言。原人物 ID、付費計數和防重放鍵保留；去內容及實體回收分欄。可繼續 CORE-041 的資料交接準備；其跨倉部分仍須對方 owner 的公開見證。啟動配置與外部邊界見 `docs/owner-erasure.md`。
最終 `artifacts/CORE-030/mac-12/` 與 `windows-04/` 各 115 項，來源 `f290cb757a66d82361fa610819f4a0338f00d3316e2d0a3cf9c348fba39a2b83`。清除世代的底層 raw method gate 也已反驗；CORE-032、033、052 重跑 `family-forget-03/` 通過。
### CORE-033 可交接原檔候選

三份未發布候選都固定在 `718cba6b1cf2b26ff2a26d33c554d4fa9452cdd1`，握手 1、Host 2、personal 1、Client 9、connector 4，Rust SDK 1.0.0-alpha.3 和 Python SDK 1.0.0a3。三機各 256 項斷言，已核對壓縮檔和每個成員。HTTP 及 SDK 方法名單見各自的 `candidate.json`。EXT-011／015 可依下列路徑讀同一原檔，正式跨倉回條仍待對方提供。

| 平台 | 本地 ZIP | SHA-256 |
| --- | --- | --- |
| Windows x86_64 | `artifacts/CORE-033/handoff-718cba6b1cf2/windows-validation/handoff/yuki-personal-preview-718cba6b1cf2-windows-x86_64.zip` | `5482c2628c57fdadb8a4ddc0f4680b73621a1307c476a582d341056a139e37f1` |
| macOS arm64 | `artifacts/CORE-033/handoff-718cba6b1cf2/macos-arm64/yuki-personal-preview-718cba6b1cf2-darwin-arm64.zip` | `4b0cb965d048e11063dc235756c104f83d22670cf02670ac3467e75dc72310ae` |
| Linux arm64 | `artifacts/CORE-033/handoff-718cba6b1cf2/linux-arm64-02/candidate/yuki-personal-preview-718cba6b1cf2-linux-arm64.zip` | `4427bd183ae5ea95566e65cb7dc7bb1df282eaf140f94e81c526399e932968bd` |

CORE-038 在 `codex/core-038-media` 完成有版本的 byte bridge 與原 Social 交接。Mac `artifacts/CORE-038/mac-09/`、HJW `windows-01/` 各 114 項；兩台實際 SDK 程序驗跨插件拒讀、分塊、source 清除途中 ASR 回覆、原操作冷恢復；兩個 Profile 驗兩張本插件圖片加外插件 handle，成功效果與呈現各自留回條。原 QQ 及受控外部 gateway 各自交原版本；篡改 plan 被拒。18 張相鄰卡在各卡 `media-broker-regression-01/` 通過。SDK Rust 1.0.0-alpha.4、Python 1.0.0a4，候選 `media.process/open/publish` revision 1；正式凍結待 EXT-021、030、031、032 真實消費。協議說明在 `protocol/plugin/media/README.md`。乾淨來源的候選包另按提交名封裝。
CORE-014 補核發現原版長 Work 在續接時呼叫 `find_contacts`。已於 `codex/core-014-contacts` 補原名稱、原參數 schema、單一人物的多帳號綁定、Person／Space keyset，以及分開的 `find_conversations`。讀取沿原 caller 和來源交集，發布前重查並綁真實來源；舊人物與會話目錄入口仍可用。固定 v4 八項對拍在 `artifacts/CORE-014/contacts-v4-01/`，正常 Main 的原 wire 在 `contacts-mac-04/original-contact-directory.json`。此修正供 CORE-040 原長 Work 消費，不以替代工具替換原劇本。

聯絡人補核最終來源樹 `62ca2f8c48e6cf303e74cb780ebf47c4dd608182b04acb3e77b81d97319e7a53`，Mac 與 HJW Windows 各 142 項通過，產物在 `artifacts/CORE-014/{contacts-mac-06,contacts-windows-02}/original-contact-directory.json`。CORE-005、007、015、052 的 `contacts-regression-02` 均通過，另留 13 張相鄰卡的 `contacts-regression-01`。包含原 UUID 大小寫別名升級、現有來源的發布前重查與目錄來源依賴。下一步將此版本供 CORE-040 原長 Work 使用。

CORE-016 分段續跑補核在 `codex/core-016-segments`，整合基底 `eb616e7` 包含已驗 CORE-033 文件下載及 CORE-014 原聯絡人工具。原 Work 到段額後在全批配對與完成判定後保存 queued，後續段沿原 request／operation／根預留接續。Mac、HJW Windows 各 184 項通過，來源樹 `6b09095e31c6de2f8b276cb6eec8de336456543ba4aa79b921491df845de6c2d`，見 `artifacts/CORE-016/{segments-mac-04,segments-windows-01}/work-segment-recovery.json`。兩個實際 kill 點、換段限額、根用盡、未知已付及 queued 取消均已驗。CORE-005、011、012、013、017、018、022、023、033、052 的 `work-segments-regression-01` 全過。

CORE-040 Rust 六請求長 Work 已由正常執行檔跑通兩次冷啟、原私人補答、QQ 逐片送達及原文件版本下載，初步回條 `artifacts/CORE-040/C-long-functional-05/`。此回條只驗該劇本功能。固定原版歷史工具名稱與本倉名稱仍有差異，完整 A/B/C 同口徑量測及 Compact 長分支待完成。下一步補全比較及 CORE-034 Code 子呼叫。
CORE-030 在 `codex/core-030-forget` 增補同人族與歷史私聊清除、原管理路由、可續 GC，以及有待清義務時的獨立插件清除入口。Mac `artifacts/CORE-030/mac-11/forget-canaries.json` 已驗 115 項；先前 HJW `windows-02` 驗過同一核心清除劇本，後續 raw-method gate 補強會隨新來源重驗。固定 v4 七項測試在 `artifacts/CORE-030/v4-oracle-01/`。25 張相鄰卡通過各卡 `family-forget-01/`，包括 ASR、附件、兩種 Compact、提取、向量、歷史及 Host 的實際遲到來源斷言。原人物 ID、付費計數和防重放鍵保留；去內容及實體回收分欄。可繼續 CORE-041 的資料交接準備；其跨倉部分仍須對方 owner 的公開見證。啟動配置與外部邊界見 `docs/owner-erasure.md`。
最終 `artifacts/CORE-030/mac-12/` 與 `windows-04/` 各 115 項，來源 `f290cb757a66d82361fa610819f4a0338f00d3316e2d0a3cf9c348fba39a2b83`。清除世代的底層 raw method gate 也已反驗；CORE-032、033、052 重跑 `family-forget-03/` 通過。

CORE-050 在 `codex/core-050-aux-model` 實作已配置的輔助模型路由。原 call ID、用途、來源／原 handle、Profile 與根預留一起固定，HTTP attempt 使用既有帳本。已驗並發同鍵、未知不重購、三個實際 kill 點、改 Profile、跨插件 handle、最後額度、取消晚返、插件關閉及重啟、來源擦除、六種文字協議、圖片／WAV／原 handle 下的轉換音訊。Work 段計數納入輔助及 embedding 請求。Mac `artifacts/CORE-050/mac-15/`、HJW `windows-01/` 同源通過，來源樹 `96f216fb9a8256cba304bfd9b79fcfd69e8de6074a476be2266b68991a10b02e`；23 張相鄰卡回歸見 `artifacts/CORE-050/regression-summary-01.json`。具名產物 `aux-attempts.json`，aux revision 1，Rust SDK `1.0.0-alpha.6`、Python `1.0.0a6`。待接入 Code 與最新 main 後，從乾淨提交生成三平台候選。真 EXT-025／030／032／033／035 消費尚待提供。
CORE-034 在 `codex/core-034-code` 完成核心候選實作。Mac `artifacts/CORE-034/mac-11/` 與 HJW Windows `artifacts/CORE-034/windows-04/` 各通過 381 項，來源 tree `40314f82d4e36d1463ea749e66811d7558514187305174be097f241e72e2ead8`。原子 VM checkpoint／子 intent、四個實際 kill 點、真 HTTP 未知、共用根額度、32 步、八層插件鏈、A→B→A 拒絕、來源清除、取消及 runtime 變更均有回條。24 張相鄰卡回歸見 `artifacts/CORE-034/regression-summary-01.json`。最終 Windows 曾因 D 槽滿而編譯失敗，回收本次已驗測試的編譯快取後，用原來源重跑通過。SDK Rust `1.0.0-alpha.5`、Python `1.0.0a5`，composition revision 1。具名產物為 `nested-boundary.json` 及 `vm-recovery-cuts.json`。真 Monty EXT-023／024 消費證據待提供，協議保持候選。下一步從乾淨 commit 產三平台包，等待 CI 後合併 main。

CORE-040 的 Mac `C-long-functional-20-01` 與 HJW Windows `C-long-windows-20-01` 各完成 20 個六請求長 Work、800 項功能斷言，原件在 `artifacts/CORE-040/`。來源為 CORE-016 分段修正的已驗執行檔。工具宣告差異、完整 Compact 長分支及同口徑 A/B/C 資源量測仍待完成。
CORE-024 在 `codex/core-024-search` 實作獨立原生搜索。Responses、Messages 及 Gemini 都用實際 loopback HTTP 驗證來源；一般 Main 透過自己的工具宣告派發，原聊天未送入獨立連線。冷重啟仍可命中完整且未過期的原收據，改 connection、Profile、金鑰域或憑據會重新派發。日期不明、孤立來源和未完成回應分開處理，原邏輯的未知嘗試重啟後維持一次。方法和來源證據在 `artifacts/CORE-024/{mac-07,windows-01}/search-proof-cache.json`；固定舊版橋接 24 項測試在 `artifacts/CORE-024/v4-oracle-01/junit.xml`。EXT-029 尚需以它的真實 Tavily owner 消費核心合同。
兩機各 377 項斷言，驗證來源 `de95a208c5a3464a46e50105baf3801f00f32cd86e2c2072b8350974a8243d80`；10 張相鄰卡的回歸位於各卡的 `native-search-regression-01/`。

CORE-034 實作 `40d0d43` 已推 [PR #43](https://github.com/Institute-of-Agent-Life/yuki-core/pull/43)。接入 main `905e968` 後，Code 的 381 項及原生搜尋、Work 分段、媒體、forget、工具 registry 回歸通過 `artifacts/CORE-034/code-search-integration-01/` 與其餘五卡的 `code-search-integration-02/`。搜尋離線 fixture 已明選 lexical-only。CORE-050 已開始原路由及付費身份實作，119 項正常／冷啟斷言通過，撤權與媒體邊界仍在驗證。

CORE-034 乾淨來源 `285240c` 的 Linux 候選已通過。Mac 在同時編譯多倉時，32 步 Code 仍為 running，而錄製器沿用 12 秒等待先行逾時。該情境改按已配置的 30 秒 Code watchdog 等待 35 秒，完成及原回執斷言保留；`artifacts/CORE-034/mac-12/` 的 381 項通過。原失敗回條保存在 `handoff-285240cb716d/macos-validation/`。

## CORE-034 未發布候選包

來源 `f3f3723aa5322c6a2a20e8cb3b04d845cd7773cc`，三機來源樹 `15239d4a9f4f1fc17ddf992d46e8f5bb293dfcbe96a97d5447e2a7e7e22214f8`。Mac、HJW Windows、Linux arm64 各通過 381 項與獨立目錄啟動。握手 1、Host 2、composition 1、Rust SDK `1.0.0-alpha.5`、Python `1.0.0a5`。八個方法包含目錄／schema、原子 invoke、原結果 resume、snapshot put/get、code.resume、code.sources。原件與成員雜湊核驗位於 `artifacts/CORE-034/handoff-f3f3723aa532/handoff-verification.json`。EXT-023／024 可消費本地候選，穩定凍結仍待真 VM 消費回條。

| 平台 | 路徑 | SHA-256 |
|---|---|---|
| macOS-26.6.2-arm64-arm-64bit | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-034/handoff-f3f3723aa532/macos-arm64/yuki-code-preview-f3f3723aa532-darwin-arm64.zip` | `a404610ee179994885f136417d195d7b13b55ee09fd43e3581bf1e648957e355` |
| Windows-11-10.0.22000-SP0 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-034/handoff-f3f3723aa532/windows-validation/handoff/yuki-code-preview-f3f3723aa532-windows-x86_64.zip` | `fc5f6f1cb8ddd2fe4c62b335705699b6fb8e181993b0a57fa920fe0798e67d04` |
| Linux-6.12.54-linuxkit-aarch64-with-glibc2.36 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-034/handoff-f3f3723aa532/linux-arm64/candidate/yuki-code-preview-f3f3723aa532-linux-arm64.zip` | `2e0704581efe0b0133c15e8e7700e4d4d40806a4e752d4b715790919257254db` |

CORE-038 三平台候選已由乾淨來源 `e25c2b8acce34858d5fd67ac408186e49a1da160` 建立。三端各通過 114 項斷言，解包啟動和每個 ZIP member 雜湊均已核對。共用來源樹 `27083f6d5273a689e8aa147edf551b97a70c4a5591b1d74db8c30e5f13bbd080`，握手 schema 1、Host 2、media 1、Rust SDK `1.0.0-alpha.4`、Python SDK `1.0.0a4`。實含 `media.process`、`media.open`、`media.publish`，使用受控 SDK 子程序驗證。下表均為未發布的開發候選，真實 EXT-021、030、031、032 消費證據待回傳。

交接根目錄 `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-038/handoff-e25c2b8acce3/`。每個 ZIP 旁附 `candidate.json`、`SHA256SUMS`，根目錄附 `handoff-verification.json`。

| 平台 | 根目錄內 ZIP 路徑 | SHA-256 |
|---|---|---|
| Windows x86_64 | `windows-validation/handoff/yuki-media-preview-e25c2b8acce3-windows-x86_64.zip` | `f7bff236ca0c518016a4bf40dafaddb29b9d86d6fa92722fdce179620d296310` |
| macOS ARM64 | `macos-arm64/yuki-media-preview-e25c2b8acce3-darwin-arm64.zip` | `8379af387c4080f3ce14bbeba898790cba9f06a0f03dff926b2ee30f0c01ee05` |
| Linux ARM64 | `linux-arm64/candidate/yuki-media-preview-e25c2b8acce3-linux-arm64.zip` | `d93dbe77128699e2f02948dc14929f264565b0f36e59641994db0fd87ceb4281` |

CORE-038 補核真 EXT-031 消費證據。擴展 checkout `0ec6f9351bc6f18f2cbe28863fb88ad03cc47fcd` 的 Windows candidate-01 原 SQLite 已含自動 `source_holds` 與 `owner_erasures`，原測試未等待非同步 ACK 就截取 wire。只讀診斷在 `artifacts/CORE-038/ext031-{readonly-diagnosis,diagnosis-summary}-01.json`。核心補強測試直接核原 handle/source、自動 forget 回調及清空義務，Mac／HJW 均通過 `artifacts/CORE-038/{automatic-owner-01,automatic-owner-windows-01}/`，來源 tree `04f76269fa4e655de9d788b823edd6c2e9832bb238ee5793fee03ccd633709ea`。EXT-031 candidate-02 含手動 acquire，正式對拍還需其正常自動來源流程及 Main 呈現證據。原 `e25c2b8` 三平台候選仍可讀。
CORE-014 補核發現原版長 Work 在續接時呼叫 `find_contacts`。已於 `codex/core-014-contacts` 補原名稱、原參數 schema、單一人物的多帳號綁定、Person／Space keyset，以及分開的 `find_conversations`。讀取沿原 caller 和來源交集，發布前重查並綁真實來源；舊人物與會話目錄入口仍可用。固定 v4 八項對拍在 `artifacts/CORE-014/contacts-v4-01/`，正常 Main 的原 wire 在 `contacts-mac-04/original-contact-directory.json`。此修正供 CORE-040 原長 Work 消費，不以替代工具替換原劇本。

聯絡人補核最終來源樹 `62ca2f8c48e6cf303e74cb780ebf47c4dd608182b04acb3e77b81d97319e7a53`，Mac 與 HJW Windows 各 142 項通過，產物在 `artifacts/CORE-014/{contacts-mac-06,contacts-windows-02}/original-contact-directory.json`。CORE-005、007、015、052 的 `contacts-regression-02` 均通過，另留 13 張相鄰卡的 `contacts-regression-01`。包含原 UUID 大小寫別名升級、現有來源的發布前重查與目錄來源依賴。下一步將此版本供 CORE-040 原長 Work 使用。

CORE-016 分段續跑補核在 `codex/core-016-segments`，整合基底 `eb616e7` 包含已驗 CORE-033 文件下載及 CORE-014 原聯絡人工具。原 Work 到段額後在全批配對與完成判定後保存 queued，後續段沿原 request／operation／根預留接續。Mac、HJW Windows 各 184 項通過，來源樹 `6b09095e31c6de2f8b276cb6eec8de336456543ba4aa79b921491df845de6c2d`，見 `artifacts/CORE-016/{segments-mac-04,segments-windows-01}/work-segment-recovery.json`。兩個實際 kill 點、換段限額、根用盡、未知已付及 queued 取消均已驗。CORE-005、011、012、013、017、018、022、023、033、052 的 `work-segments-regression-01` 全過。

CORE-040 Rust 六請求長 Work 已由正常執行檔跑通兩次冷啟、原私人補答、QQ 逐片送達及原文件版本下載，初步回條 `artifacts/CORE-040/C-long-functional-05/`。此回條只驗該劇本功能。固定原版歷史工具名稱與本倉名稱仍有差異，完整 A/B/C 同口徑量測及 Compact 長分支待完成。下一步補全比較及 CORE-034 Code 子呼叫。
CORE-017 實作 `8a41277`，[PR #29](https://github.com/Institute-of-Agent-Life/yuki-core/pull/29)，HJW 與 Mac 均通過 106 項。
CORE-012 提示詞補核 [PR #25](https://github.com/Institute-of-Agent-Life/yuki-core/pull/25) 的 head `1eb7be0` 通過六項檢查，已合併 `791e686`。
CORE-024 在 `codex/core-024-search` 實作獨立原生搜索。Responses、Messages 及 Gemini 都用實際 loopback HTTP 驗證來源；一般 Main 透過自己的工具宣告派發，原聊天未送入獨立連線。冷重啟仍可命中完整且未過期的原收據，改 connection、Profile、金鑰域或憑據會重新派發。日期不明、孤立來源和未完成回應分開處理，原邏輯的未知嘗試重啟後維持一次。方法和來源證據在 `artifacts/CORE-024/{mac-07,windows-01}/search-proof-cache.json`；固定舊版橋接 24 項測試在 `artifacts/CORE-024/v4-oracle-01/junit.xml`。EXT-029 尚需以它的真實 Tavily owner 消費核心合同。
兩機各 377 項斷言，驗證來源 `de95a208c5a3464a46e50105baf3801f00f32cd86e2c2072b8350974a8243d80`；10 張相鄰卡的回歸位於各卡的 `native-search-regression-01/`。

CORE-024 的 PR #38 已通過三平台 push／PR 六項檢查，於 main 合併 `905e968c5a04187944bf8f0ff4a3667652221731`。CORE-032 整合此版 main，同時保留原生搜尋與插件工具的凍結宣告；相關卡將以整合後來源重驗。

CORE-032 與 main 搜尋整合後，Mac CORE-032 的 `search-main-integration-01` 65 項、CORE-024 的 `host-main-integration-01` 377 項，以及 CORE-015 的 `search-host-integration-01` 均通過。原插件宣告與搜尋宣告一同經凍結／重名檢查。

CORE-039 已接入 Host `24d40cc` 及 main 的原生搜尋。原生分發與搜尋回歸通過，產物為 `artifacts/CORE-039/search-host-integration-01/`、`artifacts/CORE-024/dist-host-integration-01/`。PR #28 等最新三平台 CI 後進 main。原先候選包仍對應其固定來源 SHA。
CORE-016 已接入 Host `24d40cc` 及 main 的原生搜尋。Work 與搜尋回歸通過，產物為 `artifacts/CORE-016/search-host-integration-01/`、`artifacts/CORE-024/work-host-integration-01/`。PR #26 等最新三平台 CI 後進 main。

CORE-026 已接入 Work `04acd97` 的搜尋整合。CORE-026、024、016 回歸通過 `artifacts/CORE-{026,024,016}/facts-search-integration-01/`。PR #27 依最新來源等三平台 CI。

CORE-017 已接入 fact 分支 `bd52c95` 的搜尋整合。等待與搜尋回歸通過 `artifacts/CORE-{017,024}/wait-search-integration-01/`。PR #29 依最新來源等三平台 CI。

CORE-022 已接入 wait 分支 `03a52e5` 的搜尋整合。Compact 與搜尋回歸通過 `artifacts/CORE-{022,024}/compact-search-integration-01/`。PR #30 依最新來源等三平台 CI。

CORE-018 已接入 Compact `5d24062` 的搜尋整合。排程通過 `artifacts/CORE-018/schedule-search-integration-01/`。搜尋宣告情境的測試額度隨擴增 schema 調至 400,000，獨立超額拒絕情境保留，`artifacts/CORE-024/schedule-search-integration-02/` 通過。PR #31 等最新三平台 CI。

CORE-023 已接入排程 `72a93ae` 的搜尋整合。私人整理與搜尋回歸通過 `artifacts/CORE-{023,024}/private-search-integration-01/`。PR #32 等最新三平台 CI。

CORE-027 已接入私人整理 `18b4099` 的搜尋整合。提取與搜尋回歸通過 `artifacts/CORE-{027,024}/extract-search-integration-01/`。PR #33 等最新三平台 CI。

CORE-025 已接入提取 `dd3d4ff` 的搜尋整合。媒體與搜尋回歸通過 `artifacts/CORE-{025,024}/media-search-integration-01/`。PR #34 等最新三平台 CI。

CORE-028 已接入媒體 `894a27a` 的搜尋整合，原生搜尋離線 fixture 明選 lexical-only。向量與搜尋回歸通過 `artifacts/CORE-{028,024}/vectors-search-integration-01/`。PR #35 等最新三平台 CI。

CORE-029 已接入向量 `9e32244` 的搜尋整合。授權檢索與搜尋回歸通過 `artifacts/CORE-{029,024}/retrieval-search-integration-01/`。PR #37 等最新三平台 CI。

CORE-030 已接入檢索 `5c5a830` 的搜尋整合。原人物清除與搜尋回歸通過 `artifacts/CORE-{030,024}/forget-search-integration-01/`。PR #39 等最新三平台 CI。

CORE-028 計費補核統一 Embedding 寫入的 `input` 欄位。擦除保留器把舊 `input_tokens` 轉為共同欄位，狀態查詢同時相容兩種原紀錄。Mac 與 HJW 的 `embedding-usage-{regression,windows}-01` 通過實際晚返／來源擦除斷言，來源樹 `53f59231008eca8de8f82b0c0145d80fd4f319471739c5329379d9aace65fefe`。CORE-052、011 回歸通過，舊欄位單元測試回條在 `artifacts/CORE-028/embedding-usage-regression-01/legacy-usage-unit.txt`。既有已丟失的 usage 維持未知。

CORE-029 已接入向量 usage 補修 `5e0a59e`。檢索與向量回歸通過 `artifacts/CORE-029/embedding-usage-integration-01/`、`artifacts/CORE-028/retrieval-usage-integration-01/`。PR #37 依最新來源等待三平台 CI。

CORE-030 已接入檢索 `89a736f` 的 usage 補修。清除與向量回歸分別通過 `artifacts/CORE-030/embedding-usage-integration-01/`、`artifacts/CORE-028/forget-usage-integration-01/`。PR #39 從此最新來源重跑 CI。
CORE-033 已接入媒體 `894a27a` 的搜尋整合。個人入口與搜尋回歸通過 `artifacts/CORE-{033,024}/personal-search-integration-01/`。PR #36 等最新三平台 CI。

CORE-038 已接入個人入口 `c979a2e` 的搜尋整合。媒體自動 owner 與搜尋回歸通過 `artifacts/CORE-{038,024}/broker-search-integration-01/`。PR #40 等最新三平台 CI。

CORE-034 已接入媒體 `4004abf` 的自動 owner 回條及搜尋整合。Code、媒體、搜尋與工具 registry 通過 `artifacts/CORE-{034,038,024,015}/code-owner-integration-02/`。原 `f3f3723` 三平台候選保留固定來源，PR #43 等最新 CI。

CORE-050 實作 `8359146` 已推 [PR #44](https://github.com/Institute-of-Agent-Life/yuki-core/pull/44)。Code `d28d369` 接入後，CORE-050、034、016、024、038、031、011、030 八卡通過 `aux-code-integration-01/`。同一 SDK alpha.6 同時保留 Code、媒體及輔助模型方法。下一步封裝與補上下文多來源的擦除綁定。

CORE-050 多來源上下文補核通過 Mac `mac-18` 與 HJW `windows-02`，各 561 項，來源樹 `55f34d1393141e95115338b5ece6c965648e76860a3d0f3bc07d44961ab329fd`。`llm.complete.sources` 在原根額度預留前綁定實際依賴，僅接納原來源、原呼叫依賴或同入口已持有來源。額外來源擦除會封鎖晚返結果，原來源保持有效。未持有來源與跨入口來源拒絕，重送維持原請求身份。具名證據為兩目錄的 `aux-attempts.json`。下一步以乾淨提交製作三平台未發布候選。

## CORE-050 未發布候選包

來源 `952954d86166c6bb8ef1cec0683480df28787970`，來源樹 `b6eabdfbab78382368f8264d2e9d96bcf3e167209d64d8e38aee4af694c40a7a`。Mac、HJW Windows、Linux arm64 各 561 項與獨立路徑啟動通過。握手 1、Host 2、aux-model 1、Rust SDK `1.0.0-alpha.6`、Python `1.0.0a6`。實含 `llm.complete`、`llm.result`、六種文字 wire、Chat 圖片／音訊及多來源擦除。原根額度與物理 attempt 沿用共同帳本。真 EXT-025、030、032、033、035 消費待回傳。

Windows 首次封裝遇暫存執行檔清理 WinError 32，同一固定來源與驗證 manifest 在新 sandbox 重試成功。證據與 ZIP 成員核驗位於交接根目錄的 `windows-pack-recovery.json`、`handoff-verification.json`。

| 平台 | 路徑 | SHA-256 |
|---|---|---|
| Linux-6.12.54-linuxkit-aarch64-with-glibc2.36 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-050/handoff-952954d86166/linux-arm64/candidate/yuki-aux-model-preview-952954d86166-linux-arm64.zip` | `800bff7da32c81bb7173c9411c6edd898c32a2934dbf496a491dbca5d04023ee` |
| macOS-26.6.2-arm64-arm-64bit | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-050/handoff-952954d86166/macos-arm64/yuki-aux-model-preview-952954d86166-darwin-arm64.zip` | `a4b05689a35b4fec34fea2657c187507611e71e6d079f75ab629ddcff55e7fb8` |
| Windows-11-10.0.22000-SP0 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-050/handoff-952954d86166/windows-validation/handoff-retry-02/yuki-aux-model-preview-952954d86166-windows-x86_64.zip` | `ec0088c957caf21ca11b344cb69db539e22085f4aa8520c56feb19b272f29fe4` |

## CORE-036 群聊參與核心

`codex/core-036-participation` 完成原 event 提升、原場景 SELF、窄來源頁與實際 send／quiet 反饋。規則與兩次邀請競爭仍只有一個接納，提升可續原 Work；SELF 使用自己的 origin、root 與明確接收 Bot。QQ 啟動配置、當前安裝和原來源在派發前重核。外部網關共用原發送帳本，分段未知保留既有確認，冷啟不重發。

Mac `mac-06` 與 HJW `windows-01` 各通過 128 項，來源樹 `0c5c2a018ddf9356b274a196e453c477e3517a5f43bcdb2b659f1cf5676c1838`。兩端的 `promotion-vs-self.json` 包含原 Work、來源、SELF 身份與物理發送回條。11 張相鄰卡均通過，彙總為 `artifacts/CORE-036/regression-summary-01.json`。候選 schema 1、SDK alpha.7，真 EXT-025 消費待回傳。先前失敗 fixture／gateway 綁定案例保留於 `mac-01`、`mac-03`、`mac-04`。

兩端已驗開發封裝屬未提交工作樹來源。下一步從本卡乾淨提交重新封裝三平台候選。CORE-035 已建立正常與故障時序，將接續維護授權與審閱寫入。CORE-050 的來源補核另通過 CORE-011、016、030、052 四卡 `aux-sources-regression-01/`。

## CORE-036 未發布候選包

實作 `9f9c3d0eaace08c2abe59e0a6ca229c9293dd8e1` 已推 [PR #45](https://github.com/Institute-of-Agent-Life/yuki-core/pull/45)。Mac、HJW Windows、Linux arm64 各 128 項通過，來源樹 `7fc0d1a8f2a15257589776606608c86c18bab0f2a596650d610209e3764dcaa1`。三端均完成獨立路徑啟動與 ZIP 成員雜湊核驗，交接根目錄附 `handoff-verification.json`。握手 1、Host 2、participation 1、Rust SDK `1.0.0-alpha.7`、Python `1.0.0a7`。五個公開方法涵蓋來源頁、來源分段、真人提升、SELF 提案及實際回饋。真 EXT-025 消費待回傳，候選均未發布。

| 平台 | 路徑 | SHA-256 |
|---|---|---|
| Linux-6.12.54-linuxkit-aarch64-with-glibc2.36 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-036/handoff-9f9c3d0eaace/linux-arm64/candidate/yuki-participation-preview-9f9c3d0eaace-linux-arm64.zip` | `127847f6cab2d7cb8c3378068701ffd51d6610467a0d7ef4c42d0affb5d1174a` |
| macOS-26.6.2-arm64-arm-64bit | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-036/handoff-9f9c3d0eaace/macos-arm64/yuki-participation-preview-9f9c3d0eaace-darwin-arm64.zip` | `c438c7f23c4549210b13e9d77bd3ed5360e853fd47164854ec4d74cac0de4584` |
| Windows-11-10.0.22000-SP0 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-036/handoff-9f9c3d0eaace/windows-validation/handoff/yuki-participation-preview-9f9c3d0eaace-windows-x86_64.zip` | `6c2d5f22416836ac0c489b3fe1bb308e8f79b813e21c9ca321d4ec619eb59f1f` |

## CORE-035 維護授權與已付審閱

`codex/core-035-maintenance` 已完成十二個維護 RPC、加法遷移 33、SDK alpha.8／a8。包含原來源與 fact／工具回執版本綁定、已付 preview、核准子集與原鍵回放、歷史入口重建、限定 Reembed、來源審計及 FTS 重建。大內容與依賴以原 SHA-256 分頁。工具證據保留真實 Work 子任務與原人物來源，私聊上下文限原 SELF 範圍。

Mac `artifacts/CORE-035/tool-map-mac-06/maintenance-grants.json` 通過 303 項，來源樹 `02e537febf439d9e4f39754f421120d8334d2cc032d8847c1d297f2f2fb8e401`。實機 HJW Windows `windows-01` 同來源亦通過 303 項。CORE-007、011、016、026、027、028、029、030、033、036、050、052 十二卡 `maintenance-regression-01/` 全部通過。前幾輪 fixture、合同分支與 SDK 版本不一致的失敗回條保留。

CORE-028 已知 usage 擦除補修隨此工作樹驗過，獨立修正已推向量 PR #35。真 EXT-033、034、035、036、048 的候選消費證據仍待交接。實作 `0cdd9dffe07c` 已推 PR #46。CORE-051 已在獨立 `codex/core-051-sessions` 分支開始正常／故障時序與隔離資料模型，持續接續 Rust 實作。

## CORE-035 未發布候選包

來源 `0cdd9dffe07c18949990492fd2a9944919c455d3`，來源樹 `991bafb2f70c4571da5ae956be9cc903adc00d237670961ab23d99a456e9f1c5`。Mac、HJW Windows、Linux ARM64 各 303 項及乾淨 PATH 啟動通過。maintenance schema 1、Host 2、握手 1、Rust SDK `1.0.0-alpha.8`、Python `1.0.0a8`。十二個實含方法、全部 ZIP 成員與雜湊已核對，證據為交接目錄 `handoff-verification.json`。PR #46 等最新 CI 與前置合併，真擴展消費仍待回傳。

| 平台 | 可交接路徑 | SHA-256 |
|---|---|---|
| Linux-6.12.54-linuxkit-aarch64-with-glibc2.36 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-035/handoff-0cdd9dffe07c/linux-arm64/candidate/yuki-maintenance-preview-0cdd9dffe07c-linux-arm64.zip` | `04c13a21ea2d42a84d11b285d93c46dafa98f20270bf3b285dcd2d48afb8011c` |
| macOS-26.6.2-arm64-arm-64bit | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-035/handoff-0cdd9dffe07c/macos-arm64/yuki-maintenance-preview-0cdd9dffe07c-darwin-arm64.zip` | `3060058a12b5b10efc9fc78f1dd5d6ddcd8fe820dd72f353885c8248f067079c` |
| Windows-11-10.0.22000-SP0 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-035/handoff-0cdd9dffe07c/windows-validation/handoff/yuki-maintenance-preview-0cdd9dffe07c-windows-x86_64.zip` | `e2b964e87a30d5c230fbb0805a9de973cb91160b6e8b52606b363ca0fd77a152` |

## CORE-051 隔離插件會話

`codex/core-051-sessions` 已實作八個 Session RPC、遷移 34、雙 SDK alpha.9／a9，接入共同邏輯請求與物理 attempt 帳本。原插件 namespace、Person／群組範圍、已付重啟恢復、持久／暫存生命週期、來源分頁、擦除與晚返使用量均有實際 Host 路徑。

Mac `recovery-overlap-mac-01/` 通過 730 項，來源樹 `b1e1bd809386e297e3e77807a90aa6d866117cac89e15a8f8fd9add9e05aa674`。17 張相鄰卡回歸已通過，見 `artifacts/CORE-051/regression-summary-02.json`。先前遷移漏掉兩種既有 origin，現已保留七種並以已有執行與費用資料升級驗證。Windows 第二輪重啟測試失敗的原證據保留於 `windows-02/`。第三輪 `windows-03/` 同一來源通過 730 項。重播可先得到原 `prepared` 回條，測試沿原鍵等待持久結果；各 cut 的中間狀態另存 `session-cut-receipt-*.json`。Mac `recovery-overlap-mac-02/` 再加入確定性同鍵併發，735 項通過，來源樹 `3d249134c993533596bca6135503fd0c82d58855dcf58d016d7e97fc34528b96`。接著以乾淨提交完成三平台同源候選。真 EXT-047 消費及 CORE-054 舊會話導入另列後續。

## CORE-034 原生 feed 確認補修

真 EXT-023 回條 `core034-consumer-01/native-feed-ack.json`（SHA-256 `6b8fc8a5029880d68d7c15285de33a36e57d05003b58779740b9eae303060f11`）證實 Monty 同一 feed 可有兩個相繼呼叫。核心確認現在接受相同 feed，仍核原 child／engine call、checkpoint、snapshot 與已知回執，拒絕倒退和改寫重播。

Mac 與 HJW Windows 各 383 項通過，來源樹 `bf43b16e03c308a6cf43c6526664eec511a32d81810cf35ecfb0b90b8ee68f8f`，具名產物在 `artifacts/CORE-034/native-feed-ack-{mac,windows}-01/{nested-boundary,vm-recovery-cuts}.json`。四個 kill cut 均以同 feed 的兩個不同 call 驗證。CORE-011、016、032、052 回歸 460 項通過，彙總 `artifacts/CORE-034/feed-ack-regression-summary-01.json`。此補修將由乾淨提交重新封裝候選，真插件重跑結果待回傳。

### CORE-034 相同 feed 補修候選

來源 `f74ecdd86b732d0fb34fa12f0ca9defa377db6b8`，來源樹 `cc219a06edc93e62d87a960bbd54893d7ff9e31c7a9cc2a0a52327c63fd0f208`，三平台各 383 項與獨立 PATH 啟動通過。SDK alpha.5／a5、composition 1、Host 2。逐成員雜湊均已核驗，回條 `artifacts/CORE-034/handoff-f74ecdd86b73/handoff-verification.json`。真 EXT-023／024 須使用此補修版重跑原 feed 0／call 0→1；候選尚未發布。

- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-034/handoff-f74ecdd86b73/linux-arm64/candidate/yuki-code-preview-f74ecdd86b73-linux-arm64.zip`，SHA-256 `7a2a68e2e2be6009962948b2015b266ab613f233790111765f345163b6e05da2`。
- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-034/handoff-f74ecdd86b73/macos-arm64/yuki-code-preview-f74ecdd86b73-darwin-arm64.zip`，SHA-256 `4f65192f02e234e7c2146f8eebb3c472e3656b41fabeac89dde7d05c4a7a99b1`。
- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-034/handoff-f74ecdd86b73/windows-validation/handoff/yuki-code-preview-f74ecdd86b73-windows-x86_64.zip`，SHA-256 `4d073b6f15f5d10c0c8594064c568555671e90c49394f7da64ff1d9568b7ec2e`。

CORE-051 實作 `b176a7e` 已推 [PR #47](https://github.com/Institute-of-Agent-Life/yuki-core/pull/47)。現已整合 CORE-034 `befb666` 的原生 feed 確認補修及三平台交接記錄。整合後 CORE-034、051、050 分別通過 `sessions-feed-integration-01/`、`native-feed-integration-01/`、`sessions-feed-integration-01/`，接著製作同時含兩項能力的三平台乾淨候選。

### CORE-051 三平台未發布候選

來源 `9db8b3f073e34a6b86c8f93e878c2de09f0e25ba`，來源樹 `8bfe544d695eb64813fadd6d7ddba27a7e43b8da97edd940d6b7f8c3e260d785`。Mac、HJW Windows、Linux arm64 各 735 項與獨立 PATH 啟動通過，ZIP 成員雜湊核驗回條位於交接根目錄。Session 1、Host 2、握手 1、Rust SDK `1.0.0-alpha.9`、Python `1.0.0a9`。八個公開方法、六種文字 wire、原 Work 根額度與來源擦除均有實際重放，亦含 CORE-034 相同 feed 補修。真 EXT-047 消費待回傳，全部候選未發布。

- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-051/handoff-9db8b3f073e3/linux-arm64/candidate/yuki-session-preview-9db8b3f073e3-linux-arm64.zip`，SHA-256 `326325c842984e88ec80e94056b86afdab280e62968d9b22aceaa809ce281a99`。
- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-051/handoff-9db8b3f073e3/macos-arm64/yuki-session-preview-9db8b3f073e3-darwin-arm64.zip`，SHA-256 `3c2dab0738d434db26cd145e82d29a7e79773d4d9713649a1ef04b58949a32e9`。
- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-051/handoff-9db8b3f073e3/windows-validation/handoff/yuki-session-preview-9db8b3f073e3-windows-x86_64.zip`，SHA-256 `1e0d5856740899539c9f4adac2b4c6ce6d01aff6d7697597203d37f2d87d7f56`。

下一個已就緒核心任務是 CORE-058，工作分支 `codex/core-058-notifications`，已從本候選建立正常與故障時序。CORE-048、CORE-014 原歷史適配及 CORE-040 完整長 Work 對拍仍有本倉工作。

## CORE-058 通知與原目標授權

`codex/core-058-notifications` 已實作六個公開入口 `notification.publish/read/sources/grant_target/revoke_target/status`。migration 35、Host 2、notifications 1、SDK alpha.10／a10。Mac `artifacts/CORE-058/legacy-gateway-mac-02/notification-publication.json` 的 318 項通過，來源樹 `0cbb8333febc0bff62bc8832c640ffae5fec8c88e4df6c0979426bff95ea6705`。HJW Windows `artifacts/CORE-058/windows-01/notification-publication.json` 同樹 318 項通過。

同插件原鍵、UTC／manifest、雙插件隔離、媒體順序／過期／未知雜湊、commit 後失答、未知投遞、QQ／外部 gateway 實收、Main 靜默、真人及插件原 Work 喚醒、動態目標撤權、來源清除均有具名回條。真管理員從原 Host call 的執行鏈取得，固定 v4 訂閱插件所用的 `grant_target.conversation_id` 與 `status` 已對照；舊授權回條重投不撤銷後來的 revoke。舊 media 句柄案例使用明示降級 fixture，未聲稱已導入 v4 資料庫。

CORE-007、010、011、013、016、017、025、030、032、034、036、038、050、051、052 的 `notification-regression-01/` 共 2,948 項全部通過，彙總 `artifacts/CORE-058/regression-summary-01.json`。三平台候選位置見下表，真 EXT-027／047 消費待回傳。下一個已就緒任務為 CORE-048，工作分支 `codex/core-048-profiles` 位於 `.cache/worktrees/profile-config`。

CORE-014 聯絡人補核 PR #41 已合併 main `0ded0dd`。CORE-023 PR #32 最新 push／PR 六項 CI 通過後合併 `e01cd30`，核驗回條 `artifacts/CORE-023/main-merge-b0dab43835c2.json`。CORE-034 feed 修正已接入 CORE-050 並以 `94ee5c3` 推送，CORE-034／050 的 `aux-feed-integration-01/` 均通過。

### CORE-058 未發布候選包

來源 `cb02f74ea48b85c6b6932423b93e6332b97f2871`，來源樹 `fa1d772ed399ac5e2f98cb7649cced89dee4822f8745c881eb955ea1ba49199c`。Mac、HJW Windows、Linux arm64 各 318 項與獨立 PATH 啟動通過。notifications 1、Host 2、握手 1、SDK alpha.10／a10，六個通知方法均已驗。各 ZIP 的 13／16 個成員均逐項核 SHA-256，回條 `artifacts/CORE-058/handoff-cb02f74ea48b/handoff-verification.json`。真 EXT-027／047 可只讀消費這組候選並回傳方法與實收證據。候選尚未發布，PR #48 等最新 CI 及前置進入 main。

| 平台 | 只讀交接路徑 | SHA-256 |
|---|---|---|
| macos-arm64 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-058/handoff-cb02f74ea48b/macos-arm64/yuki-notification-preview-cb02f74ea48b-darwin-arm64.zip` | `eb4794b29db562b5fc12cde284474d61bf17ac9cba608d3f1ecd5c7dfed48f77` |
| linux-arm64 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-058/handoff-cb02f74ea48b/linux-arm64/candidate/yuki-notification-preview-cb02f74ea48b-linux-arm64.zip` | `34739495bb1bd35555a3016c1d01b8c49d9c1e335461329fbaf2a1c4ff226828` |
| windows-x86_64 | `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-058/handoff-cb02f74ea48b/windows-validation/handoff/yuki-notification-preview-cb02f74ea48b-windows-x86_64.zip` | `dea4b90f5008e466f6385ceea1d0833574dcf10a5a079f8ac639b7a4be4c542c` |

CORE-034 的真 Monty 修正版重跑尚待消費端提供可執行環境。此 SSH 身份 `hjw\hilbert` 執行既有 WSL 清單命令回「拒绝访问」，紀錄 `artifacts/CORE-034/native-consumer-wsl-access-01.json`，原 Runtime 未變更。核心三平台補修候選已齊備，其驗證不受此消費重跑阻塞。
CORE-050 已接入 CORE-034 的真 Monty 同 feed 確認補修 `befb666`。Mac CORE-034 的 383 項與 CORE-050 的 561 項通過，證據分別位於 `artifacts/CORE-{034,050}/aux-feed-integration-01/`。PR #44 以整合後 head 等待三平台 CI。

CORE-036 已接入 CORE-050 `94ee5c3` 的同 feed 修正。CORE-034、036 分別通過 `participation-feed-integration-01/`，保留原 SELF 與真人授權邊界。PR #45 以整合後 head 重驗三平台 CI。

CORE-035 已整合 CORE-036 `adaa111` 的同 feed 確認修正，CORE-034 與 CORE-035 的 `maintenance-feed-integration-01/` 通過。PR #46 按整合後 head 重驗三平台 CI。

CORE-051 已接入維護分支 `d772ed2` 的合併歷史，實作檔案與已驗版本相同，僅更新進度記錄。PR #47 等此最新 head 的 CI。

### CORE-058 CI 回條檔案交接修補

PR #48 head `58e53f5` 的原 CI 失敗 ZIP、雜湊與位置保留在 `artifacts/CORE-058/ci-failure-58e53f5.json`。Windows 命中回條檔案 PermissionError，Linux 的 CORE-050 原 media.process 呼叫超時。共享測試 peer 改為有界讀取同一回條，涵蓋半寫入 JSON 和 Windows 真實 sharing lock；持續畸形 JSON 仍失敗。CORE-050 超時時另留原 Host calls／operations 診斷。

Mac CORE-058 的 `file-receipts-mac-01/` 通過 321 項，HJW `file-receipts-windows-01/` 通過 324 項，含三個 Windows 鎖定檢查。同一來源樹 `32f477e6cc94d0cf870e9d282a97b93d20c2f4e8f8130cc61ca36b2f43e4ac65` 的 Linux ARM64 CORE-058／050／051 分別通過 321／561／735 項，位於 `artifacts/CORE-058/file-receipts-linux-01/`；Mac 的 CORE-050／051 `notification-file-receipts-01/` 亦通過。實際 Mac Host binary 與 `cb02f74` 候選逐位元相同，原候選仍按其固定來源與 318 項回條交接。

已接入 CORE-051 `aa0f52d` 的合併歷史，較上述驗證只改進度記錄。PR #48 等修補後最新 head 的三平台 CI。

PR #48 head `8007173` 的 Windows CI 已通過 CORE-058 324 項及 CORE-050 561 項，後於 CORE-034 結束後的新 child 拒絕探針等待檔案回條超時。原 ZIP SHA-256 `bf833847112c33ec61c7b32eeb63bb76cf498a395c5083dec02f6b4e96391763` 和失敗位置保存在 `artifacts/CORE-058/ci-failure-8007173.json`。Code peer 與探針讀者也沿同一檔案作有界讀取，新增 parent 結算斷言及執行緒／原 Host calls 診斷。Mac、HJW Windows 的 `artifacts/CORE-034/code-probe-files-{mac,windows}-01/` 同樹 `6708d16ffa66418132770a5cd96a39652090dea772cbf615529541e1e115d526` 均通過 384 項。核心 Runtime 與逾時、授權條件均沿用原值，等待修補後最新 CI。
CORE-035 最新 head `d772ed2` 的 macOS PR 工作 `127225319602` 在多個案例通過後被 45 分鐘整體 job 上限截斷。GitHub annotation 明確記錄超時，診斷為 `artifacts/CORE-035/macos-ci-timeout-01.json`。整體 job 窗口改為 90 分鐘，全部案例、斷言和各卡 timeout 保留，推送後按最新 head 重驗三平台。

CORE-051 的 macOS PR 工作 `127228529052` 同樣被 45 分鐘 job 上限截斷，沿用同一份 timeout 診斷。接入 CORE-035 `13e64dd` 後使用 90 分鐘整體窗口，全部檢查保留。

CORE-058 已接入 CORE-051 `fa1fddb` 的 90 分鐘 CI job 窗口。全部執行步驟及各卡驗證條件保留，原 runtime 未變；PR #48 按整合後 head 重驗。
PR #47 `fa1fddb` 的 Linux CORE-027 冷恢復案例提早送出 ingress：extraction_policy.enabled 在 operator IPC 綁定前就已提交。失敗 ZIP SHA-256 `84c71110f7e19aad2b614abfd64afe0e35562f748eb11974ac860f4e914700da`，診斷 `artifacts/CORE-051/ci-failure-fa1fddb.json`。fixture 改等同一子程序的正常 ready JSON，不重送命令、不延長 timeout；Mac CORE-027 `session-owner-readiness-01/` 通過，提交後重驗 CI。


## CORE-048 本地完成，PR #49 等待 CI 與前置合併

分支 `codex/core-048-profiles`，工作目錄 `.cache/worktrees/profile-config`，基底 `e913c81`。前置 CORE-024、033 已有本地驗證。新增精確依賴 `toml_edit =0.23.7`；v3 TOML／原 Admin DTO 保未知欄位、註釋、旁檔密鑰和聯合 revision。

已接入 OS／受限 bearer／真 chat 設備、原三個模型 HTTP 入口、兩階段保存和原回執恢復。Main／Work／child、提取及已批准輔助 task route 保原 Profile；獨立搜尋封存原連接，embedding 連接在重啟後生效。額度、purpose、來源與網路權限仍受原部署政策限制。原資格撤銷、同 revision 衝突、外部內容 hash 衝突、已付回應冷恢復和三個 rename 中止點都有具名測例。使用說明見 `docs/model-profiles.md`。

| 同一來源樹 `176707309b2a4015855b92139b8cef1b8cecac782a6003d0511d67fba041c4c2` | 驗證結果 | 產物位置 |
|---|---|---|
| Mac ARM64 | 624 項通過 | `artifacts/CORE-048/complete-mac-15/` |
| HJW Windows x86_64 | 621 項通過 | `artifacts/CORE-048/complete-windows-04/` |
| Linux ARM64，含真 readonly mount | 645 項通過 | `artifacts/CORE-048/complete-linux-03/` |
| Mac 18 張受影響核心卡 | 4283 項通過 | `artifacts/CORE-048/profile-regressions-02.json`、`regression-02-<卡號>/` |

Windows 首次實際 rename 發現 Win32 錯誤 87，已用同一根 handle 的原生 NtSetInformationFile 修正並在 HJW 重驗；證據 `windows-rename-probe-01/`、`windows-rename-02/`。固定 SHA 隔離舊版 oracle 的 operator／v3 profile 測試 17 項、退役 route 1 項通過；原 ZIP 未修改。全部測試均無付費模型呼叫。

實作提交 `d33ae32`、整合提交 `b30eec4d31c316e6307281ef0cef8ca02594bf38` 已推送，[PR #49](https://github.com/Institute-of-Agent-Life/yuki-core/pull/49) 以 sessions 為前置，等最新 head 的三平台 CI，僅合併 main。

乾淨提交 `b30eec4d31c316e6307281ef0cef8ca02594bf38`／來源樹 `73e4e2402f39b49aa4cfd0391f122aef1f4222f0bb2626c8b5cd59af36c72d2a` 的三平台候選已驗。Host schema 2、Profile schema 3、Python SDK `1.0.0a9`／Rust SDK `1.0.0-alpha.9`；實含三個 Admin 模型入口、六個 task route、獨立搜尋與重啟 embedding。Mac／Windows／Linux 分別通過 625/621/645 項；候選都從乾淨 PATH 啟動，ZIP 成員逐項 SHA-256 核對通過。所有候選仍為 unpublished-development-candidate；真實 EXT-049 消費證據尚缺。

- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-048/handoff-b30eec4/linux-arm64/candidate/yuki-profile-preview-b30eec4d31c3-linux-arm64.zip`，SHA-256 `eedb6a027d6b16caed8ec89135e5b22ebc374149ea028b208547800394e210bc`。
- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-048/handoff-b30eec4/macos-arm64/candidate/yuki-profile-preview-b30eec4d31c3-darwin-arm64.zip`，SHA-256 `90085f29f823f6056d521713718831c8d4ff221a757bc0204a5651b802651649`。
- `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-048/handoff-b30eec4/windows-x86_64/handoff/yuki-profile-preview-b30eec4d31c3-windows-x86_64.zip`，SHA-256 `1eec0bbfedb81e6bc4a47c1e5fabe9d2737faffe152b8a4e836f26cc63d9502e`。

完整交接索引為 `artifacts/CORE-048/handoff-b30eec4/verified-candidates.json`。較早 `development-package-01/` 的 dirty source ZIP 僅保留作開發證據。


CI 修正沿用已驗 `002a10d` 的 CORE-027 owner ready 回條，以及 `0f723ad` 的原檔案有界讀取；不重送 mutation、不改原 timeout 或接受條件。下一個已就緒核心任務是 CORE-037；真實 EXT-049 消費回條尚待對方，79 個固定 capability 宣告不等於全部管理方法已實作，候選不標為已發布。

CORE-037 已在 `codex/core-037-control`／`.cache/worktrees/control` 開始實作，先驗固定 Admin 角色及 SOUL 合同、原付費鏈和中斷保存；尚未完成或提交。CORE-014 原歷史 API 在隔離舊版取得 14 個實際查詢回條，`/Users/huangjiawei/.cache/yuki-core/original-history-oracle-01/original-history.json`，SHA-256 `75c6e389ddb5bfebab6ace552a14f882f7f612c69cf712b626cf936d8aaea6d0`；原歷史適配仍進行中。
CORE-058 接入 CORE-051 `002a10d` 的真 owner ready 同步修正。Mac CORE-027 `notifications-owner-readiness-01/` 通過；PR #48 按整合後 head 重驗三平台。
CORE-016 分段補核已接入個人入口 `c979a2e` 的搜尋整合。分段、搜尋、原聯絡人工具回歸通過 `artifacts/CORE-{016,024,014}/segments-search-integration-01/`。PR #42 等最新三平台 CI。
已接入 main `30a5ea7` 的 Host 合併提交。實作樹與上一輪驗證相同，PR 已轉向 main 並重新執行 CI。

CORE-039 已接入 main 的 Work `0261764`。Mac 分發與 Work 回歸分別通過，產物為 `artifacts/CORE-039/work-main-integration-01/`、`artifacts/CORE-016/dist-main-integration-01/`。PR #28 以此次整合的最新提交重新等待三平台 CI。

CORE-039 已整合 main `3867548` 的等待、Compact 與 facts。分發與 Compact 回歸通過 `artifacts/CORE-039/compact-main-integration-01/`、`artifacts/CORE-022/dist-main-integration-01/`。PR #28 等此版本三平台 CI，既有候選繼續對應原固定 SHA。

CORE-039 已整合 main 排程 `2c01cfa`，分發與排程分別通過 `artifacts/CORE-039/schedule-main-integration-01/`、`artifacts/CORE-018/dist-main-integration-01/`。PR #28 將按整合後 head 重驗三平台 CI。
聯絡／對話目錄已與 main 的 Work `0261764` 整合。原生宣告同時保留兩組入口，Mac CORE-014 與 CORE-016 回歸通過，見 `artifacts/CORE-014/work-main-integration-01/`、`artifacts/CORE-016/contacts-main-integration-01/`。PR #41 等最新三平台 CI。

CORE-014 聯絡人分支已整合 main `3867548` 的等待、Compact 與 facts。CORE-014、022、026 分別通過 `compact-main-integration-01/`、`contacts-main-integration-01/`、`contacts-main-integration-01/`。原聯絡人工具與 facts 宣告均保留，PR #41 等此整合版本 CI。

PR #41 整合來源 `2eba72c` 的 Linux CI 在 CORE-024 的 Claude 宣告 fixture 等不到 HTTP。沿用已在排程分支驗過的 400,000 輸入額度，使新增工具 schema 能進入宣告檢查；原額度耗盡反例維持並通過。Mac CORE-024 `contacts-declaration-budget-01/` 全部通過。原 CI ZIP SHA-256 `e0a39748bda9960361e363b2dd66616d092ddd6eea5db2397a9655b5272994d4` 保留於本地診斷。

CORE-027 已整合 main 的聯絡人／會話目錄 `0ded0dd`。提取與目錄回歸通過 `artifacts/CORE-027/contacts-main-integration-01/`、`artifacts/CORE-014/extract-contacts-integration-01/`。PR #33 轉向 main，按整合後 head 重驗三平台 CI。

CORE-023 的最新六項 CI 通過後已合併 main `e01cd30`。CORE-027 接入此提交僅產生進度記錄差異，沿用上述相同實作樹的整合驗證。

CORE-025 已接入提取 `9d1c64b` 的 main 與原聯絡人目錄。媒體、目錄、工具 registry 三卡分別通過 `artifacts/CORE-025/contacts-main-integration-01/`、`artifacts/CORE-{014,015}/media-contacts-integration-01/`。兩組原工具宣告一同保留，PR #34 等整合後 head 的三平台 CI。
CORE-039 已整合 main 的聯絡人／會話目錄 `0ded0dd`。分發與目錄回歸通過 `artifacts/CORE-039/contacts-main-integration-01/`、`artifacts/CORE-014/dist-contacts-integration-01/`。PR #28 按此整合 head 等最新三平台檢查。

CORE-027 已整合 main 的分發提交 `9a0b14a`。Mac CORE-039、027、032 全部通過，產物分別為 `artifacts/CORE-039/extract-dist-integration-01/`、`artifacts/CORE-027/dist-main-integration-01/`、`artifacts/CORE-032/extract-dist-integration-01/`。PR #33 按此整合版本重新等待三平台 CI。

CORE-027 六組 CI 通過並合併 main `3d7d12c`；CORE-025 接入此來源與分發入口。Mac CORE-025、039 通過 `artifacts/CORE-025/dist-main-integration-01/`、`artifacts/CORE-039/media-dist-integration-01/`。PR #34 改以 main 為基底，等待整合後 head 的三平台 CI 再合併。

CORE-028 已整合 main `692e323`。向量驗證 `artifacts/CORE-028/media-main-integration-01/` 通過；分發初次在 Host Rust 續接等待失敗，加入狀態診斷後該段通過，隨後顯示離線安裝案例未明選 lexical-only。案例補明配置，CORE-039 `artifacts/CORE-039/vectors-main-integration-03/` 全部通過。前兩份失敗回條保留，PR #35 以整合後 head 重驗三平台 CI。

CORE-029 已整合向量 `a5a9b35` 與 main `692e323` 的分發及目錄入口。Mac CORE-029、039 通過 `artifacts/CORE-029/media-main-integration-01/`、`artifacts/CORE-039/retrieval-main-integration-01/`。PR #37 以新 head 重驗，待 CORE-028 在 main 合併後再依 main CI 合併。
PR #35 的 `a5a9b35` 在 Linux／Windows CI 顯示新聯絡人 fixture 啟動時缺 Embedding 配置。保留原 CI ZIP `.cache/vector-ci-13434438838.zip`，SHA-256 `3c48f5a28cce9d5b6e8a5931db42e5d2ed5826945ab78c31b9d4a8799b00d5dd`；離線目錄案例明選 lexical-only 後，Mac CORE-014 `artifacts/CORE-014/vector-contacts-memory-mode-01/` 通過。原 hybrid 必填規則及 CORE-028 的故障案例保留。

CORE-029 同步向量 CI 修正 `c251510`。原目錄案例明選 lexical-only，Mac CORE-014 `artifacts/CORE-014/retrieval-contacts-memory-mode-01/` 通過；其餘實作沿用前述整合驗證。

CORE-029 合入新增工具後，CORE-012 的舊測試根額度在第三筆共享根請求前耗盡。CI `42070867425` 與本機 `retrieval-main-ci-01` 均保留失敗；診斷顯示兩筆已預留 68,206 units，第三筆尚未派發。測試沿用後續分支 `40d0d43` 的 1,000,000 units 設定，保留六筆請求上限及核心額度邏輯；`artifacts/CORE-012/retrieval-main-ci-02/` 已通過。超時現在附原執行、診斷和根預留，方便定位。

CORE-030 已整合檢索 `e9af0f5` 及 main 分發／個人入口。CORE-030 `forget-main-integration-01/` 與 CORE-039 `forget-main-final-01/` 通過；可攜包離線啟動明選 lexical-only。CORE-039 第三次整合遇到一次 Rust tool 後續請求未派發，保留 `forget-main-integration-03/host-rust-followup-diagnostic.json`；原因未定位。隨後 debug 三次、release 十二次診斷及移除臨時診斷後的最終檢查均通過，原故障回條保留，測試失敗時現在附原 turn／intent 狀態。PR #39 以整合 head 等 CI。
CORE-025 六項 CI 通過並合併 main `692e323`；CORE-033 已整合此來源及核心分發入口。Mac CORE-033、039 通過 `artifacts/CORE-033/media-main-integration-01/`、`artifacts/CORE-039/personal-main-integration-01/`。PR #36 以整合後提交重新等待三平台 CI。



CORE-038 已整合個人入口 `d576aee` 及 main `692e323` 的分發入口。Mac CORE-038、039 通過 `artifacts/CORE-038/dist-main-integration-01/`、`artifacts/CORE-039/media-broker-main-integration-01/`；整合用 development-package 實際打包及乾淨 PATH 啟動通過，該包標記 dirty 且未發布，並非新增正式交接包。PR #40 以新 head 重驗，等待個人入口在 main 完成。
CORE-033 已由六項 CI 合併 main `76cf762`。CORE-028 整合此來源，保留兩張卡的 CI；Mac CORE-028／033／012／039 全部通過 `vectors-personal-main-01/`。Main fixture 沿用檢索已驗的 1,000,000 units，不改核心額度邏輯。PR #35 按最新整合 head 重新驗證三平台。

CORE-029 整合向量 `e43339b` 與個人通道 main `76cf762`。Mac CORE-029、033、039 全部通過 `artifacts/CORE-{029,033,039}/retrieval-personal-main-01/`；兩組 SDK、個人協議與記憶檢索介面一併保留。PR #37 按此整合 head 等最新三平台 CI。

CORE-030 同步檢索 `87a07f9` 與個人入口 main `76cf762` 的提交祖先。此次整合只有 CI 和進度記錄差異，實作及測試來源均與 `62abb62` 相同，沿用 `forget-main-final-01/` 驗證；最新 head 仍須通過六項 CI 才合併。

CORE-034 接入清除 `b745a9d`、檢索及 main 的分發入口。Mac CORE-034 通過 `erasure-distribution-main-02/`，CORE-039／030 通過 `code-erasure-distribution-main-03/`、`code-distribution-main-03/`。整合時移除重複的 Tool provider 欄位，候選共用打包器按被驗的 debug／release profile 取二進位；兩次先前失敗回條保留。原 `f74ecdd` 三機候選仍按原來源供擴展讀取，PR #43 以最新整合 head 重驗六項 CI。
CORE-038 已整合個人入口 `d576aee` 及 main `692e323` 的分發入口。Mac CORE-038、039 通過 `artifacts/CORE-038/dist-main-integration-01/`、`artifacts/CORE-039/media-broker-main-integration-01/`；整合用 development-package 實際打包及乾淨 PATH 啟動通過，該包標記 dirty 且未發布，並非新增正式交接包。PR #40 以新 head 重驗，等待個人入口在 main 完成。

## CORE-028 CI 診斷補核

PR #35 原 head `e43339b` 的 Windows 重跑在 CORE-012 等候工具序列後的模型請求逾時，12 秒限制保留；下載證據 `/Volumes/huawei/項目實戰/yuki-core/.cache/vector-ci-13449371446.zip`。框架原先將 `queue.Empty` 記成空字串，已修正為非空例外名稱並增加真子程序負向案例；發生 wire 逾時會保留原 executions／operations／attempts 與本地實收資料。此修正不宣稱已找出該次平台逾時根因。

Mac `artifacts/CORE-001/empty-exception-01/`、`artifacts/CORE-012/wire-diagnostics-mac-01/` 通過；HJW 同一快照 `CORE-012-ba61fb93385f-415385e6` 的 CORE-001 與 CORE-012 亦通過，後者取回 `artifacts/CORE-012/wire-diagnostics-windows-01/`。PR 仍須最新 head 的六項 CI 全過才合併。

CORE-038 已整合個人入口 `d576aee` 及 main `692e323` 的分發入口。Mac CORE-038、039 通過 `artifacts/CORE-038/dist-main-integration-01/`、`artifacts/CORE-039/media-broker-main-integration-01/`；整合用 development-package 實際打包及乾淨 PATH 啟動通過，該包標記 dirty 且未發布，並非新增正式交接包。PR #40 以新 head 重驗，等待個人入口在 main 完成。
CORE-016 分段分支已整合個人入口 `d576aee` 及 main 分發。修正自動合併重複的 provider 欄位，保留原工具宣告入口。Mac CORE-016、014、039 全部通過 `artifacts/CORE-016/dist-main-segments-03/`、`artifacts/CORE-014/dist-segments-integration-02/`、`artifacts/CORE-039/work-segments-main-integration-02/`。PR #42 以整合後 head 等 CI。

CORE-034 同步媒體 `7cd4e42` 及 Work 分段 `e357ab6`。祖先整合保留 memory 配置驗證；Work 分段自動合併造成聯絡人註冊遺漏，已恢復並由 CORE-014／015 `code-segment-main-01/` 驗證。最新修正 head 通過 CI 前不合併。

CORE-050 接入 Code `6615b29`，包含 main 分發、清除、檢索、媒體及 Work 分段的已驗整合。Mac CORE-050、039 均通過 `artifacts/CORE-050/code-distribution-main-01/`、`artifacts/CORE-039/aux-code-distribution-main-01/`。共用候選打包器保留 auxiliary schema／SDK 能力與 debug／release 二進位選擇。PR #44 按新 head 重驗六項 CI，原候選仍按原來源標記。

CORE-036 整合輔助模型 `3de48dc` 與 main 分發鏈。Mac CORE-036、039 通過 `artifacts/CORE-036/aux-distribution-main-01/`、`artifacts/CORE-039/participation-aux-distribution-main-01/`。組合候選打包器保留 participation 方法與各卡驗證來源，PR #45 按新 head 重驗三平台。
CORE-035 整合參與邊界 `2290520` 與 main 分發鏈。Mac CORE-035、039 通過 `artifacts/CORE-035/participation-distribution-main-01/`、`artifacts/CORE-039/maintenance-participation-distribution-main-01/`；共用候選打包器保留 maintenance grants 與實際二進位 profile。PR #46 按新 head 重驗六項 CI，通過前不合併。

CORE-051 整合維護 `402a9aa` 與 main 分發鏈。Mac CORE-051 `maintenance-distribution-main-01/`、CORE-039 `sessions-maintenance-distribution-main-02/` 通過。第一次 CORE-039 的 Rust 工具結果缺少 data，原因尚未定位，失敗回條保留；補上失敗時完整結果與原執行狀態診斷後，release Host 另連續三次 65 項斷言通過於 `artifacts/CORE-032/sessions-release-diagnostic-{01,02,03}/`。未將重跑通過當成根因修復，PR #47 仍依最新 head 六項 CI 驗證。

CORE-048 接入隔離會話 `671d8c5` 與 main 分發鏈，Mac CORE-048 `sessions-distribution-main-02/`、CORE-039 `profiles-sessions-distribution-main-02/` 通過。候選器保留模型設定 metadata、四協議與獨立 Search／Embedding 的原能力記錄；PR #49 按整合後 head 等六項 CI。前一份 `sessions-distribution-main-01/` 在錯誤的 CORE-049 工作樹執行而缺卡入口，已撤銷該處本輪 merge，未修改 CORE-049。
CORE-058 接入隔離會話 `671d8c5` 與 main 分發鏈，Mac CORE-058 `sessions-distribution-main-01/`、CORE-039 `notifications-sessions-distribution-main-01/` 通過。候選共用打包保留通知合同、SDK 版本及驗證來源，PR #48 按整合後 head 重驗六項 CI。
## CORE-037：實作與三平台候選已驗，PR #51 待 CI

分支 `codex/core-037-control-integration`，實作提交 `1e7b89362b6f806aa11eed841b28071b22151e7e`，已推 [PR #51](https://github.com/Institute-of-Agent-Life/yuki-core/pull/51)。整合 CORE-048 與 CORE-058，Rust 共用控制服務涵蓋角色／SOUL、Person／Binding／Presence、Work、模型設定與單次探測、插件批准／啟停／發現／設定／觀察、trace、usage、原 outbox 安全重排及確定失敗回條。原 paid continuation 與未知效果不取得新重試權。

Mac ARM64、HJW Windows x86_64、Linux ARM64 在同一乾淨來源樹 `f58b344f4da4d4bf36c5a7a2dc20a67f6d752a373e8d362f3891d562a3bd22bf` 各通過 977 項。兩種實際安裝 SDK 執行 Host 管理呼叫；故障包含 SOUL／discovery 寫入後真 kill、CAS、撤權、冷啟、成功／失敗原回條及 UUID 大小寫重放。CORE-048 663 項、CORE-058 326 項與相鄰整合回歸亦通過。

**候選未發布。** 交接索引 `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-037/handoff-final-01/verified-candidates.json`，SHA-256 `8519f27ea2c10a9f692ac6b34e47e8a92bf3751b871b07dcaf02f9597e5b946b`；內含絕對路徑、來源提交、逐成員驗證、實含能力與各平台回條。握手 1、Host 2、control 1、reader 41，Rust SDK `1.0.0-alpha.11`／Python `1.0.0a11`。

| 平台 | ZIP SHA-256 |
|---|---|
| Mac ARM64 | `37945f2a6abda16c54fd024271471a0adaa78343b093fae3fac7313867ee9e9c` |
| Linux ARM64 | `673662bd73e0a89418c8aeb2740a00c34c491ea3da56c735b0bb7f6540f5b625` |
| HJW Windows x86_64 | `cfad9951aa18cb1e0a9677b8d6e042d84dafdc6e6b13ba95feb4ab943fd9a4e6` |

PR #51 等 #48／#49 進 main 後，按最新 head 的三平台 push／PR 六項 CI 合併。真 EXT-022／037／038／039／049 消費證據仍缺，穩定協議未凍結。下一個可獨立任務 CORE-055 已開始；CORE-014 原歷史工具與 CORE-040 原 wire 等價對拍仍待補齊。

## CORE-055：逐欄用量驗證

`codex/core-055-usage` 以 CORE-037 `1e7b893` 為前置。先設六次呼叫含失敗、零／缺欄位、47/110 同報分母、分離 attempt 欄位、紐約 DST、跨午夜重試與四種當前資格。`coverage-red-03/` 在跨 attempt 分母案例確實失敗；修正唯讀投影後 `coverage-mac-01/` 136 項通過；加入 SDK 包後 `coverage-mac-02/` 138 項、CORE-037 `usage-regression-01/` 977 項通過。原 provider/model/profile、邏輯呼叫計數與 DTO 保留，依實際 attempt 派發時間選欄位；無新增 writer／估價／重試權。

實作 `0efe249`，整合來源 `214197a1a8d3bebbc3364d3566a65ea81bab09a6`，已推 [PR #52](https://github.com/Institute-of-Agent-Life/yuki-core/pull/52)。Mac ARM64、HJW Windows x86_64、Linux ARM64 同一乾淨樹 `37472cf77eecc810a2ca5b4d3cf663cb26dc2accf79ae27ec410654aa3ead666` 各 138 項通過；原 DTO `protocol/control/usage.json`、兩種 SDK 包及空 PATH 候選啟動均已驗證。握手 1、Host 2、usage 1、reader 41，SDK Rust `1.0.0-alpha.11`／Python `1.0.0a11`。

**候選未發布。** 交接索引 `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-055/handoff-final-01/verified-candidates.json`，SHA-256 `cda5ff398b353784f013fef7d39e7b72d1d9a219a834c18846db8672a53b26f5`；內含絕對路徑、來源及逐成員雜湊驗證。ZIP 雜湊如下：

| 平台 | SHA-256 |
|---|---|
| Mac ARM64 | `b7ca70569b4fc6f536caad15134fd332a23aea5436c8b57ce3d9b1775c67dcb1` |
| Linux ARM64 | `d09aa148ba767db66019fb231fe2b6ce25d755e1eef74643b8a9d91819c51530` |
| HJW Windows x86_64 | `83e473a6ddb750b5b5de7e1febddffdeecec1bc0d4e344be8848c6f10c9cf1f8` |

PR #52 待 #51 進 main 後按最新 head 六項 CI 合併。缺 EXT-049 對當版候選的實際管理 HTTP 消費，穩定協議未凍結。初次 `coverage-red-01/` 為 Mac 系統碟滿、`-02/` 為 fixture ordinal 衝突，失敗產物保留。下一個可執行工作為 CORE-014 原歷史查詢 wire，供 CORE-040 原腳本等價對拍使用。

## CORE-057：Rust 離線模型設定移轉

分支 `codex/core-057-model-import`。新增 `import-models --inspect`、原 revision 的 apply 與 `--resume`。停寫副本以原 Application OS lock 互斥，固定 schema 0156、實際 TOML／sidecar／環境檔與 WAL 雜湊；依原來源優先序保存 null／false、停用 Search 與完整 ASR 憑證對。逐用途比較實際輸出 TOML 的有效參數，僅真差異分流共享 Profile，保留私有 headers、未知 operator 欄位及原檔權限。來源檔案不變；只在新 bundle 讀回確認後按原行 ID／鍵／scope／version／值雜湊做 SQL 原子清退。

先保留缺入口 `import-red-01/`；`artifacts/CORE-057/import-mac-05/` 功能檢查與 dirty 開發包的乾淨 PATH 啟動通過。固定 SHA 329ca7c 的隔離 oracle 22 項通過於 `/Users/huangjiawei/.cache/yuki-core/model-config-oracle-01/`；另驗原 CI 9138a528 的 20 份 model-legacy 證據。真 kill 覆蓋寫檔後／SQL commit 後，另驗非空分群拒絕、修改版本／目的檔拒絕、SQL 後筆失敗整筆回滾、舊 Application 鎖及 WAL-only 參數。首輪功能全過但缺 native SQLite build 回條，已補實際匯入連線查詢；未將該輪標為完成。

Windows／Linux 與乾淨來源候選待整合驗證；先前 dirty 包不作發布交接。此卡未啟動 owner／gateway／plugin／provider，不修改任意 .env，不修改舊倉。下一個可執行工作為 CORE-040；CORE-043 仍需 EXT-041 的實際安裝交接輸入。

## CORE-014 原歷史查詢入口補齊

分支 `codex/core-014-history-wire`。Rust `query_chat_history` 沿用固定 v4 宣告及共享 canonical event ID；recent/search/around、原 send receipt、source 交集、逐欄字面搜尋、當前 ASR、正數 keyset 分頁、上下文和公開外部事件已接實際 Main 工具 continuation。Reader 42 加可清除的逐來源結果快照；已刪除游標不重排，撤權／遺忘後不能重讀正文。舊 `history_query` 原入口保留。

先保留缺入口失敗 `original-wire-red-02/`，正常及故障檢查於 `artifacts/CORE-014/original-wire-mac-07/` 667 項通過；CORE-015、033、058 相鄰回歸均通過 `history-wire-regression-01/`。固定 SHA `329ca7c487414d2f54ddd98ff4727446ed2446cd` 隔離 oracle 的 14 個原查詢案例 SHA-256 `75c6e389ddb5bfebab6ace552a14f882f7f612c69cf712b626cf936d8aaea6d0`；大型分頁 201+29、原 provider call ID、通知 publisher、遺忘刪除結果正文均有回條。另兩次補充 fixture 因未設定 QQ 路由及用人類 erase 刪 plugin 事件被正確拒絕，失敗回條保留。

Windows／Linux 原生候選待本提交快照驗證；先前 dirty Mac 包僅打包自測，非交接發布。真擴展消費及 CORE-040 全量 wire 等價尚待；本補充不宣稱所有原 DTO 值已逐位相等。下一步依序產候選，繼續 CORE-057 離線模型設定移轉與 CORE-040。

CORE-037／030 CI 修復：Windows Python 缺少系統 tzdb，現以附來源版本、SHA-256 與授權的 IANA 2023c 固定 TZif 作獨立日曆測試輸入；不更動 Rust 時區計算。CORE-030 清除案例在改動 executable 宣告後，先驗證未審批不可執行，再由真 operator 按原 revision 批准，保留刪除後 ACK 前崩潰情境。Mac、Linux ARM64、HJW Windows 各自通過 CORE-030 120 項、CORE-037 977 項；回條位於 artifacts/CORE-030/approved-cleanup-{mac,windows}-01/、artifacts/CORE-037/pinned-calendar-{mac,windows}-01/ 與 artifacts/CORE-037/ci-recovery-linux-01/。源樹 e7d3d4982ab0679daead7202453e3f9a4801c74e17ed84027317425ead6870e2，PR #51 推新 head 後重新等六項 CI。

CORE-055 整合 CORE-037 CI 修復 `98816c2`，用相同固定 IANA fixture 驗 DST；schema 回條改讀實際 SQLite reader，避免後續可加遷移仍報 41。`artifacts/CORE-055/pinned-calendar-mac-01/` 138 項通過，原候選來源不變；PR #52 重驗最新 head。

CORE-014 三平台交接已驗：來源 `43b67d35d996ab6185507931a77e189677e9a1d6`，乾淨樹 `723429f67a5b67e8c498bba201fede6b21ec626c0e5842edd1e40d285cb66503`，Mac ARM64／Linux ARM64／HJW Windows x86_64 各 667 項與乾淨 PATH 啟動通過。reader 42、Host 2、SDK Rust `1.0.0-alpha.11`／Python `1.0.0a11`。原生 ZIP、manifest 及逐成員雜湊已核對，索引 `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-014/handoff-final-01/verified-candidates.json`，SHA-256 `c8d373992ec7e286337a4d79deb2caec81e79ee8fca5c22a3cd1f1330edc7c4c`。候選未發布，真擴展消費待補。

- macos ZIP SHA-256 `c5780e4c4346e1a378e6e5074d100cb184885126278acb9f9fb104b926a4142a`。
- linux ZIP SHA-256 `1cd0cabae64ea714c41cd11e443a0b200d01c563eb0de7522fc884716fbac233`。
- windows ZIP SHA-256 `18d65b272e547002ac980dc516f84c08ce9cdd2e9e83c185b58066dd99c30d7c`。

已推 PR #53；等待 #52 及最新 head 三平台 push／PR 六項 CI，通過後合併 main。

CORE-057 跨平台封裝補正：Linux `handoff-final-01/linux-validation-02/` 查出 provenance 誤收 ExFAT 的 AppleDouble `._` 中繼資料；移除非來源成員後，仍逐一驗 20 份原 CI JSON 與 defaults 的 SHA-256。`provenance-mac-01/` 通過，先前 Mac 212 項包含 21 項中繼資料檢查，修正後為 192 項（新增來源集合檢查）。原失敗及舊候選保留，未列入交接；將從本提交重做三平台候選。

CORE-005／040 量測補核：`database-info` 改從原 SQLite writer 佇列查詢，公開實際 synchronous、busy_timeout_ms 及 observed_connection，避免用 reader 的 PRAGMA 當 writer 設定。`artifacts/CORE-005/writer-info-mac-01/` 通過。CORE-040 最新 Rust release 的 `c-current-1200-02/` 逐筆 1200 輸入／1200 送達相符，20 組長 Work 正在跑；工具集合與提示差異仍在核對，未完成投資裁決。先前 C attempt 01 因未顯式選 lexical-only 啟動失敗，保留原紀錄。

CORE-020 整合回歸：PR #53 Linux run 42107381636 與 `history-pause-mac-01/02` 均在第 4 次 pause 前 held。原回條顯示 3 次預留共 154875 units；完整新工具宣告令下一次超過原測試 200000 units，尚未觸及待驗的 4-request 邊界。僅把該 fixture 的獨立 units 額度改為 1000000，新增「4 次全預留且 units 尚未耗盡」斷言；`history-pause-mac-03/` 通過。保留超時時的原 execution／request／budget 記錄，worker 對空字串異常保留類型，避免錯報缺 artifact。無產品預算或重試語義變更，Windows／Linux 由最新 CI 再驗。

CORE-012／040 提示補核：固定 v4 Main 與最新 Rust 的 system prefix 只差 Memory grounding 段。已從 `329ca7c` 的 `src/yuki/memory/context.py::MEMORY_GROUNDING_RULE` 提取原文並附來源雜湊，放在 SOUL 與原 Main 程式段之間。實際 HTTP 的 `CORE-012/original-memory-prompt-mac-01/` 及 CORE-020／022 同名回歸通過。CORE-040 `c-current-long-20-01/` 20 組、800 項功能斷言全過，原歷史名稱存在；此批 timing 仍不作跨臂比較。

CORE-057 三平台候選已驗：乾淨來源 `4dfb1c026a7f8e046c504484f6e7738494a866a5`，tree `e09b81dbe090adc713eb6500a2cf5d5287e87edf682facb086429888854e872b`。macOS ARM64、Linux ARM64、HJW Windows x86_64 各 192 項，均驗 inspect／匯入／原 revision resume、實際中斷、SQL 回滾、原鎖及 WAL。SDK Rust `1.0.0-alpha.11`／Python `1.0.0a11`，Host 2／握手 1，輸入 legacy schema `0156`；本卡未啟動 owner，新資料庫 schema 不冒記為已驗。

**未發布交接索引：** `/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-057/handoff-final-02/verified-candidates.json`，SHA-256 `6a1c274e4002cd0b96b43870a80c5a478af997143f0df31ab81fe2cf09bab2b7`。索引含絕對路徑、逐成員驗證及 native manifest。

- macOS ZIP `44ec44e6fa8875b95743ab226917cc2aaa06aff6cd030a229c8e28435ea13cf2`。
- Linux ZIP `a9f2da713fc6c96cb17523d01cb81ba58668c69a47854ff271748afe1b0486d6`。
- Windows ZIP `a60fc52ed9bbf2840244e23163a6ba0db7c4115cb42b5b37b96add9f05fcd5bb`。

原 failed／aborted 回條保留：第一輪缺舊 compiler cache；第二輪誤收 AppleDouble；`handoff-final-02/linux-validation/` 因發現並行共用編譯輸出而主動中止。最後 `linux-validation-02/` 在前一程序退出後順序重驗通過。PR #54 已加 CORE-057 三平台 CI，待 #53 與最新 head 六項全綠後合併 main；不發布或部署本候選。CORE-043 尚缺 EXT-041 真實安裝交接輸入，下一個本倉工作是 CORE-040 原切片差異補核。

CORE-037 整合 CI 時序修復：CORE-050 變更 aux Profile 宣告後先按實際 revision 重新批准；CORE-058 先確認離線通知已以 bot_unavailable 結算，再接上 QQ 發起另一個通知，不依賴偶然連線時序。Mac `approved-profile-mac-01`／`refused-receiver-mac-01` 與 Linux `CORE-037/ci-lifecycle-linux-02/{CORE-050,CORE-058}` 分別通過 566／330 項；HJW 的 `CORE-050/approved-profile-windows-01` 566 項通過，CORE-058 原生回歸續跑中。源樹 `9b08b55f460f89a3d6a280f52c655851dc9292eac25ab304b1a1051019b7abee`。保留原 CI 失敗；只修 fixture 的明確批准／派發時序，無放寬產品授權、通知狀態或等待上限。PR #51 最新 head 重新跑六項 CI 後才合併。

CORE-015 原 Artifact 工具補核（codex/core-015-artifact-wire）：新增原 handle／operation／陣列 path 的 inspect、get、search、text，JSON 的 image 保持明確拒絕。保留舊版顯式版本 operator 游標；讀取時核正文 SHA-256，解碼後核同版本及來源，無版本參數不能繞過晚返校驗。23 組固定 SHA 舊版 oracle 的輸入順序與回條已複驗；新 Main 工具宣告逐欄相同，既有逐項效果外層保留。Mac `artifacts/CORE-015/original-pages-mac-04` 117 項及 28 項 Rust 測試通過；mac-01 為測試尚未轉接 Snapshot 型別的編譯失敗，已保留並修正。新增候選打包入口，待乾淨 commit 三平台建包及回歸，不宣稱 CORE-040 整體等價或穩定跨倉驗收。

CORE-058 冷恢復補正（codex/core-037-control-integration）：Windows 原回條 `artifacts/CORE-058/refused-receiver-windows-01` 顯示恢復後 QQ 尚未在線，原投遞已安全拒絕，測試仍等待自動發送而逾時。改為先核持久 failed／bot_unavailable 與零物理投遞，再經 plugin.outbox.retry 的版本比較恢復同一原始操作；未知回執仍不重試。所有新 QQ 發布先等核心觀測 online。Mac `artifacts/CORE-058/cold-recovery-mac-02` 通過；首次 mac-01 把合法建立的物理子操作也誤當根身份變動，已改為核根操作，保留失敗回條。Windows cold-recovery-windows-02 及 Linux CORE-037/cold-recovery-linux-03 正在跑，未標通過。

CORE-015 三平台乾淨候選已驗：來源 `c61f45c6e831f69cd4dc7b9cefb93c778b7d2e1a`，tree `b83992480f2a9fa72f310476f8a1de6307a9b3cad3aaefd8e4c02d3a4b70b092`。Mac ARM64、Linux ARM64、HJW Windows x86_64 各 117 項及獨立乾淨 PATH 啟動通過；CORE-012／020／022／026 相鄰回歸全過（`artifacts/CORE-015/adjacent-regression-01/`）。Reader 42、Host 2、握手 1，SDK Rust `1.0.0-alpha.11`／Python `1.0.0a11`。實含 JSON／text Artifact 的 inspect、get、search、text 與 JSON image 拒絕；不宣稱 native image Artifact 驗收。

未發布交接索引：`/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-015/handoff-final-01/verified-candidates.json`，SHA-256 `da8b01c6762e7555622d63503c10822f2bd6deb7e977a802af75e43ed9cad67a`；內含三個 ZIP 絕對路徑、逐成員驗證與來源 manifest。

- Mac ZIP `49bafba2a09ec2bab6f237c8de2eed15d2ad7a344c7f2fa3c575af77d7cd0e81`。
- Linux ZIP `5517250b51c40c4acefce7d949f749de94f319a068ecbd27a10879b0750ea467`。
- Windows ZIP `b136a092ca206c53ab38cd48cea68589523dc4fa53d95c798df2ece3e51d8cf4`。

PR #56 等 #55 及最新 head 六項 CI；真擴展消費與 CORE-040 全切片等價待補。下一個獨立工作是 CORE-013 原 `get_group_members` 讀取接線。

CORE-058 冷恢復三平台完成：來源修正 `5081c5d`、tree `ca3a329a86cd1d483d38c51038a72201de1a95935ae9fb005a3e60907b579acb`，Mac `artifacts/CORE-058/cold-recovery-mac-02/` 341 項、Linux `artifacts/CORE-037/cold-recovery-linux-03/CORE-058/` 341 項及 HJW `artifacts/CORE-058/cold-recovery-windows-02/` 344 項通過。核原 failed／bot_unavailable 回條後，顯式按 revision 重試同一根操作；未知投遞仍不重試。

CORE-013 原群成員目錄補核（`codex/core-013-observed-members`）：Rust `get_group_members` 接原模型宣告、UUID keyset 分頁、同人多帳號、目前 assignment／Person／Presence、未知 mention 和來源撤銷。實際讀取只含原授權入口的已觀察帳號，群暫停或 QQ 離線仍可讀；不借發送路由、不查平台完整名冊。Mac `artifacts/CORE-013/observed-members-mac-07/` 175 項與 Rust 測試通過；CORE-009／014／015／036 四項相鄰回歸均通過 `adjacent-regression-01/`。舊版 oracle 三個原用例重跑通過，fixture SHA-256 `72a7d9c67f7e26a7c09897dc04d103d13eee45f68d6142508aece6c5c32a37a5`。

失敗保留：mac-01 在暫停前未先取得觀察；mac-03／04 發現 Main 派發漏接，已補實際原生呼叫。mac-02 誤在 root 舊 main 執行，結果不作本接線證據。mac-06 的 development-package 只供打包自測；待乾淨 commit 產 Windows／Mac／Linux 正式本地候選。reader 42、Host 2、SDK alpha.11／a11；尚未發布，未宣稱 CORE-040 全功能等價。下一個核心缺口為原 `recall_own_message`／權限目錄的 Main 接線。
CORE-048／039 CI 編譯診斷：PR #49 的兩個 Mac Intel run（42085724620、42085715264）均在 release 冷編譯 600 秒截止，尚未執行發布驗證案例。回移 CORE-037 已使用的編譯階段處理：只有 release build 使用 1200 秒編譯預算，超時仍失敗並保存部分編譯輸出和 stage 回條；其他編譯及所有功能、故障、啟動與分發驗收時間不變。原失敗保留。此處不因其他平台通過而跳過 Mac，需最新 head 三平台六項 CI 全通過才合併。

本地 Mac `artifacts/CORE-039/profile-compiler-mac-01/` 81 項分發驗證通過，source tree `a8a009014eb8f64a246d94279914c8611a5904f9443a60ae5e203837a94dc57f`；原功能驗收完整執行。
PR #43 Windows CI `42080468945` 的 CORE-039 在原命令通過後，清理臨時執行檔時遇到 WinError 32；失敗 ZIP `.cache/code-ci-13440917770.zip`，SHA-256 `4ca3d7b740a9a3c141ed87c4c2132f6c688d50dea830a08fbb424b893b15630f`。候選器僅對自建目錄的 sharing violation 最多重試五秒，持續鎖定及其他權限錯誤仍失敗，不重跑原命令。Mac CORE-039 `code-windows-cleanup-01/` 通過，HJW `code-windows-cleanup-native-01/` 正在驗證；最新提交六項 CI 完成前不合併。

CORE-050 同步 Code 的 Windows 臨時執行檔清理修正 `4448a06`。Mac CORE-039 `aux-windows-cleanup-01/` 通過；上游同份修正在 HJW 通過 `artifacts/CORE-039/code-windows-cleanup-native-01/`，該回條來源為 `6615b29` 工作樹 `4936d7b65e47d4579b9380b6436c6491d89425eaaebf0e7927df66dd9fc6671e`，不是新發布候選。PR #44 以新 head 重新檢查。

CORE-036 同步輔助模型 `c26e6c4` 與已驗 Windows 清理修正，Mac CORE-039 `participation-windows-cleanup-01/` 通過；實作未改參與授權或 SELF 額度。PR #45 以整合 head 重新驗證最新三平台 CI。

PR #42 macOS Intel CI `42071266030` 在 CORE-039 冷 release 編譯達 600 秒上限，尚未進入產品案例；原 artifact `.cache/segments-ci-13441839430.zip`，SHA-256 `6546648675809170fcb363d0797ccd2b86dc547bfcf6323e673b77ca35affdf5`。編譯器 release 階段獨立給 1200 秒，其他命令及案例期限不變，所有編譯超時現在保留 stdout／stderr 和 timed_out 回條。Mac CORE-039 `segments-release-build-budget-01/` 通過；注入超時 `segments-release-timeout-evidence-01/` 正確失敗並保留兩路輸出。PR #42 按新 head 重驗三平台，未把原失敗標為通過。

CORE-034 同步 Work 分段的冷 release 編譯預算及超時輸出保留 `e542c27`，Mac CORE-039 `code-release-build-budget-01/` 通過。既有 Windows 清理修正已在 HJW 通過 `code-windows-cleanup-native-01/`。本輪取消的 14 組 CI 均已由同分支的新 head 取代，核對與取消回條在 `.cache/superseded-ci-{candidates,cancellations}-01.json`；最新 head 的所有檢查仍須通過才合併。

PR #40 整合已合併的 Work 分段 main `ce66753`。Mac 回歸通過：CORE-038 118、CORE-016 184、CORE-012 163 項斷言，產物 `artifacts/CORE-038/segments-main-merge-01/`。推送整合提交後重新計算六項 CI。

PR #43 整合最新前置 `codex/core-038-media`，Mac 回歸 CORE-016、CORE-034、CORE-038 已通過，見 `artifacts/CORE-034/segments-parent-merge-01/`。

PR #43 的 Windows run 42147427157 在 crates.io HTTP/2 載入 siphasher 時出錯，PowerShell 接續準備 SDK，將第一個 cargo 的失敗隱藏，令離線 CORE-005 缺依賴。現在兩個鎖檔均須成功，準備階段使用非 multiplex HTTP，不改離線測試。Mac 驗證：`artifacts/CORE-005/code-locked-download-01/download-gate.json` 注入第一個錯誤後結束碼 16、第二個 fetch 沒執行；`code-locked-deps-owner-01/` 通過。原失敗 ZIP SHA-256 3fa1870cd09660b5102a44bdb740650563235251b9abd537e76b4a3cd8db1aa5，重等六項 CI。

PR #44 最新前置整合後，Mac CORE-034、CORE-050 通過，具名回條在 `artifacts/CORE-050/segments-parent-merge-01/`。

PR #45 已整合最新 CORE-050；參與與輔助模型兩張卡的 Mac 回歸通過，見 `artifacts/CORE-036/segments-parent-merge-01/`。

CORE-035 已合入目前 CORE-036 參與策略前置；正常與故障驗證見 `artifacts/CORE-035/segments-parent-merge-01/{CORE-035,CORE-036}/manifest.json`。兩套均通過。

PR #47 已接入維護分支 d658db3 的 Work 與分發修正。macOS CORE-051、035、039 均通過：051=735, 035=303, 039=84 項；見 `artifacts/CORE-051/maintenance-work-segments-01/` 及 `artifacts/CORE-{035,039}/sessions-work-segments-01/`。依新 head 等六項 CI。

PR #49 已接入 #47 的 Work 與分發修正 8a47f56。macOS 048=624, 051=735, 039=84 項均通過，詳見各卡 `sessions-048-work-main-01/`。等本 head 的六項 CI。

CORE-037／034 啟動修復：PR #51 的 Linux CI（42113062451／127545421287）在改動 Code runtime 後回 `plugin disabled`，導致整個 owner 尚未提供管理入口便退出。Host 啟動現在按目前 enabled 狀態 reconcile；未批准的 Code 只保持離線，原 Work 由原 runtime 校驗進 held，不啟動替代程序。顯式強制啟動的拒絕語義保留。Mac `CORE-034/unapproved-runtime-mac-01/` 387 項與 `CORE-037/unapproved-runtime-mac-01/` 977 項通過，tree `6f66f94254b9358dad90c66e56328c7792390f498df83102e5efb4ff63599ff2`。新增斷言核 discovered／disabled／offline、原 checkpoint、原模型預留及零派發；Windows／Linux 同源回歸正在跑。

CORE-013 群目錄三平台乾淨候選已驗：來源 `6d76210943e75a29c5bfa5f310ad378737fb9f06`，tree `a96812e8d50128f7db3f73b00860667a1d8e970a7ddea28b0b56ca2952262c6d`，Mac ARM64／Linux ARM64／HJW Windows x86_64 各 175 項通過，獨立 SDK 安裝與乾淨 PATH 啟動通過。Reader 42、Host 2、握手 1，Rust SDK `1.0.0-alpha.11`／Python `1.0.0a11`，本次驗證工具為 `get_group_members`，coverage 僅 observed。

未發布交接索引：`/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-013/handoff-final-01/verified-candidates.json`，SHA-256 `2c531868119a9b8d549e06820a16ee667f3667f668c1c221fbd24f2487460284`；索引附三個 ZIP 絕對路徑、逐成員雜湊與來源 manifest。

- Mac ZIP `ca0daa2585b185b7a1d2266f4cabb88be08445946083016b6a0fb7355a6db8da`。
- Linux ZIP `1cffea8b9ef0c9937b6a71133107abb5161f05fe068153a37d736c38ba176a84`。
- Windows ZIP `a46dcbb838a1d7fa4e724e76e22c0346be45ee919eb45bd511ddb0600cd10dff`。

PR #57 等 #56 與最新 head 六項 CI 通過；未發布、不標穩定擴展驗收。下一項原 `recall_own_message` 在 `codex/core-013-recall-wire` 進行，已核原 bot、未知結果與真實 writer 崩潰，待完整驗證及候選。

CORE-034 Code 啟動修復 `9bb02fc` 的同源三平台驗證完成：tree `6f66f94254b9358dad90c66e56328c7792390f498df83102e5efb4ff63599ff2`，`artifacts/CORE-034/unapproved-runtime-{mac,linux,windows}-01/` 各 387 項通過；Mac CORE-037 同名回歸 977 項通過。未批准的 Code 留在 discovered／disabled／offline，owner 管理入口可用；原 Work 不越過 runtime revision 或原預算。最新 PR CI 仍待完成。

CORE-013 原撤回模型工具補核（`codex/core-013-recall-wire`）：`recall_own_message(event_id)` 接實際 Rust Main 與認證 QQ socket。Reader 43 把目前工具操作綁到原出站 child／Presence／locator；群改派或暫停仍只能由原 bot 撤回。原 bot 離線拒絕，另一 bot 在線也不代撤。派發 claim 與工具效果同交易，原 session 不換代；未知、重複與崩潰重啟不重送，晚回只結算原鍵，來源清除不復活內容。

Mac `artifacts/CORE-013/recall-wire-mac-04/` 252 項與 Rust 測試通過，舊版五個原用例通過（`original-recall-oracle-01/02`），成功回條逐欄核對；writer commit 後真殺程序並冷啟動，三次物理撤回與四個已提交原鍵分開記錄。`recall-adjacent-01/` CORE-005／010／030 通過。CORE-034 發現測試 peer 仍送 Artifact 舊 version／字串 path；按原公開 schema 修正後，本分支 `artifacts/CORE-013/recall-code-regression-02/` 387 項通過，兩次業務扣額與 Artifact 子讀零扣額未變。

失敗保留：mac-01 測試雙 bot 共用憑證被啟動校驗拒絕；mac-02 把模型 call_id 誤當效果 operation_id，已經由原 intent 映射查核晚回；mac-03 成功後補完整結果對拍及包內證據，mac-04 通過。development-package 僅打包自測；待乾淨 commit 形成三平台未發布候選。未知／失敗 error_category 使用核心分類，不宣稱與舊 Python 例外類名完全相同。下一個核心補核是 `get_my_capabilities` 原主體權限目錄，穩定擴展消費及 CORE-040 全切片等價仍待補。

CORE-015／034 整合補核：Code 測試 peer 仍使用 Artifact 舊 version／字串 path，被現行公開模型 schema 拒絕。改為原 operation=get 與相對 logical_root 的陣列 path；仍核兩次業務扣額、Artifact 子讀零扣額、真派發／回執及原 128 字元。`artifacts/CORE-034/original-artifact-wire-mac-02/` 387 項通過。首次 mac-01 誤把既有核心效果外層當成 Python envelope，已修正斷言路徑並保留失敗；不改產品授權或放寬 schema。

CORE-013 撤回三平台乾淨候選已驗：來源 `266e9aa23e13990c156c6ed0bf8df3b985d6d810`，tree `8322f0c5be6285c8a62a64a1acbc91beeedfec7935cc8d99881c1348526a72eb`。Mac ARM64／Linux ARM64／HJW Windows x86_64 各 252 項、獨立 SDK 安裝及乾淨 PATH 啟動通過；Reader 43、Host 2、握手 1、Rust SDK `1.0.0-alpha.11`／Python `1.0.0a11`。同一包實驗證 `get_group_members` 與 `recall_own_message`，保留原 bot、未知、晚回及實際 writer 崩潰回條。

未發布交接索引：`/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-013/recall-handoff-final-01/verified-candidates.json`，SHA-256 `d7b86f0244ca8064e13e67cd78ad96256d9879bd35444bd4b68a2add1e73ebe2`，含逐成員驗證與三平台 manifest。

- Mac ZIP `f34dceb76bbf32f1af856b094fc70a17a3b1ec9261ce8c1ec183e31c44ae6178`。
- Linux ZIP `aa2c2b76e0d550333ebe0456fb41fc6e1f5901d7e2f07d2a5d3274c602195d80`。
- Windows ZIP `b72683a71ba2a8260f801bc6d85fd535dc871e3dd2ee366fff06b6222b569877`。

實作 `3dafd7a` 已推 PR #58，等 #57 及最新 head 六項 CI 再合併 main。候選未發布，未宣稱真擴展穩定驗收或 CORE-040 全切片等價。下一項 CORE-037 原主體權限目錄已在 `codex/core-037-permission-directory` 實作，Mac 整合初驗 1052 項通過，相鄰回歸及乾淨候選仍在進行。

CI 排隊修正（[PR #83](https://github.com/Institute-of-Agent-Life/yuki-core/pull/83)，`codex/ci-pr-queue`）：40 個開放 PR 的當時 head 有 58 個 Intel Mac job 在等。逐筆重查已過期的 45 個 run，GitHub 確認取消 43 個；2 個已有 job 執行，保留。以其他任務分支為基底的 PR 先跑 Linux 全套；以 main 為基底或轉向 main 的 PR 跑 Windows、Linux、Apple Silicon macOS 三平台完整合併檢查。同 PR 新提交取消舊任務。main 推送及手動完整檢查保留 Intel Mac。

`python ci/suite.py --out <新目錄>` 從 `ci/checks.json` 取完整清單。每台先跑含 release 的卡、fmt、clippy、writer tests、debug／release build；兩個分開的程序執行其餘案例，同一工作內依來源與 SHA-256 綁定編譯證據。快取只供 Cargo 依賴及編譯中間物，仍執行本倉的編譯與測試。卡片的斷言、期限及具名輸出保留。失敗或未跑完時，`suite.json` 列出缺口，整體回非零。SQLite 歷史測試庫分 migration 提交；reader 5、20、29 的 schema／資料相同，注入錯誤可回滾。原 Mac CORE-032 曾等不到 Rust 回呼；兩段獨立案例現在等前一回合完成，失敗時保留操作與預算診斷。佇列與測試紀錄在 [artifacts/ci](/Volumes/huawei/項目實戰/yuki-core/artifacts/ci/)。

PR #35 整合已合併的 Work 分段 main `ce66753`。Mac 回歸通過：CORE-028 94、CORE-016 184、CORE-012 163 項斷言，產物 `artifacts/CORE-028/segments-main-merge-01/`。推送整合提交後重新計算六項 CI。

PR #37 整合最新前置 `codex/core-028-vectors`，Mac 回歸 CORE-012、CORE-016、CORE-028、CORE-029 已通過，見 `artifacts/CORE-029/segments-parent-merge-01/`。

PR #39 最新前置整合後，Mac CORE-016、CORE-029、CORE-030 通過，具名回條在 `artifacts/CORE-030/segments-parent-merge-01/`。

另核對 17 筆重複 push，取消仍只排隊等待 Intel Mac 的 16 筆。#67 已開始執行並最終通過，全部 17 筆 PR run 保留。逐筆回執位於 `artifacts/ci/queue-triage-01/verified-duplicate-push-cancellations.json`。#35 合併後的 Mac 全套 35 卡見 `artifacts/ci/pr83-vectors-main-mac-01/suite.json`。#37 合併後以 `3c16aaf` 為基底重新驗證 36 卡，Mac 產物位於 `artifacts/ci/pr83-retrieval-main-mac-01/suite.json`。

PR #40 已整合 #83 的 CI 提交 `11e5a5b` 與 main 上的 CORE-028、029，同時保留 CORE-038 的媒體、ASR 和原路交付測試。Mac 37 卡通過，見 `artifacts/ci/pr40-current-main-mac-01/suite.json`。先以 #83 為基底跑 Linux，待 CI 修正合併後轉回 main 跑三平台。

PR #39 將 CORE-030 的遺忘測試接上 #83 工作流，Mac 37 卡全部通過，見 `artifacts/ci/pr39-current-main-mac-01/suite.json`。#83 提交 `11e5a5b` 的同一來源樹已在 HJW Windows、Mac 與 Linux 各驗過 36 卡，逐卡 SHA-256 見 `artifacts/ci/queue-triage-01/pr83-retrieval-native-matrix.json`。GitHub Windows、Linux 亦各通過 36 卡，見同目錄 `hosted-pr83-retrieval-main-windows-linux.json`。#39 先以 #83 為基底驗 Linux，之後轉回 main 驗三平台。

日常 PR 改用本地 Mac mini、同機 Linux ARM64 容器與 HJW Windows。三端均執行完整註冊套件。合併入口 `ci/local_matrix.py` 核對目前 PR 合併 SHA、來源樹、三個平台、全部斷言與具名產物雜湊，再寫入三個 GitHub commit status。#83 的首份本地合併回執位置為 `artifacts/ci/local-pr/83/receipt.json`。Intel Mac 候選驗證可手動執行。

#83 已合併至 main `b2cfe37`，日常驗證使用本地 Mac、Docker Linux ARM64 及 HJW Windows。三端各完成 36 卡，逐卡回執見 `artifacts/ci/local-pr/83/receipt.json`。
