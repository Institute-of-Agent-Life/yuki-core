# 核心實作進度

CORE-023 CI 補修：完整原工具宣告已超過舊 fixture 首輪上限；明示 64,000 容量並放大必要未觀察輪，保留硬上限拒絕並加驗恰好一次物理 attempt。Windows／Mac／Linux 全卡通過；詳見 [故障與修正證據](compact-capacity-fixture.md)。

任務書 v3.2 ZIP SHA-256：`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。舊倉只讀，行為基線 `329ca7c487414d2f54ddd98ff4727446ed2446cd`；K05 補正已套用工作副本，不增加 unknown 重試權。原提交、驗證、失敗診斷及完整交接雜湊見 [實作驗證記錄](implementation-evidence.md)。

CORE-037：修正歷史資料庫 fixture 的逐條落盤；[HJW、Mac、Linux 各 1052 項及超時診斷](control-fixture-timing.md)。

CORE-026 原證據合併（[PR #77](https://github.com/Institute-of-Agent-Life/yuki-core/pull/77)，`codex/core-026-memory-merge`，來源 `163a953734479df1bd3825dff595f94ffdf582fb`，reader 52）：[合同及故障時序](memory-merge.md)。固定 v4 十項原測及原狀態轉移通過；Mac 此卡 1250 項，CORE-027／029／030／035／003／039 各 172／73／120／304／20／148 項通過。驗證：`artifacts/CORE-*/memory-merge-proof-02/`。來源與保留事實需各自提供仍有效的讀證；原證據移轉、撤銷來源與第二筆寫入失敗均已實測。

未發布候選：`artifacts/CORE-026/memory-merge-handoff-final-01/verified-candidates.json`（SHA-256 `567fee374a56d902689db3f094d5f6a33d710fc43ef02c301d7bc102b266d968`）。Rust SDK 1.0.0-alpha.14／Python 1.0.0a14，HJW Windows、Mac、Linux 各 1250 項；索引列出每個 ZIP 的絕對路徑。macOS `dab154bc13bcb7eefb24f36a39391cceff5977845fa7c25932fd46afc66fb18a`；Linux `34ae512c53fbb63b8def8885f0497355c5a1f0dd1860c6a775ac9e523cdca86e`；Windows `f245dab6bc38b15ca99b341bce212916949fec69ee83c1d71bff424241679937`。

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
| CORE-038／025 原 gateway 取件 | codex/core-038-inbound-media | [原 Bot、app 與租戶取件](gateway-inbound-media.md)；SDK alpha.13／a13，reader 49。三平台各 207 項、固定原版 18 項及 7 張相鄰卡通過；索引見下，待 EXT-004／006。 |
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

CORE-033 Client9 上傳：[三平台完整性索引](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-033/client-uploads-handoff-final-01/verified-candidates.json)，SHA-256 4da11e14c4118f087c6535de7b973d60b195cb98eade2ed3a45b4d2404f2900c。未發布，待 EXT-015／016。

CORE-038 入站附件：來源 `b99b01a92443`，Windows、macOS、Linux 各通過 207 項。[候選包與逐平台 SHA-256](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-038/inbound-gateway-handoff-final-01/verified-candidates.json)，索引 SHA-256 `ad9f55a0434d54e3b8adc74c26f1e0a29d699621588eed9e668b5c48764585bb`。SDK alpha.13／a13、reader 49，供 EXT-004／006 重放原收件與過期案例；尚未發布。

CORE-038 Main 選讀：[PR #74](https://github.com/Institute-of-Agent-Life/yuki-core/pull/74)，來源 `0ef956a9c6d9`，Windows、macOS、Linux 各 318 項。[三平台候選索引](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-038/main-selected-handoff-final-01/verified-candidates.json)，SHA-256 `49437652505a0881fc14576502036101412042542d642f7eeea4f98bbdc7c459`。reader 50／SDK alpha.13、a13。原 PDF 頁文和兩張影格到達同一 Main，未額外派發模型請求。未發布。HJW 已用原 EXT-031 Rust 插件解出 PDF 兩頁與兩幀，並核對同一 Main 的實際 HTTP；[消費回條](../fixtures/media/ext031-windows-main.json)。

CORE-032 終端：`codex/core-032-terminal-work`，[原 Work／run 時序](terminal-work-scope.md)。SDK alpha.14／a14、reader 51。Mac CORE-032 127 項、CORE-039 release 146 項及 6 張相鄰卡通過；Linux 固定舊版 16 個原 Manager／Work 回歸全過。正式三平台包與 EXT-020 獨立消費分列。

CORE-013 目標：`codex/core-013-send-entry`；[原 QQ 目標、時序及界線](social-send-entry.md)。固定舊版四例、Rust Mac 013=296、014=667、032=129、015=363、023=225、026=1181 項通過。四條實際 QQ 動作、五個零派發拒絕（含委派 Work），原網關亦驗證錯誤選擇不抵達插件。Mac 及固定源回條在各卡 `canonical-send-delegation-01`、`send-entry-integration-01` 與 `artifacts/CORE-013/original-send-entry-oracle-01`。乾淨三平台封包另驗。

## 缺少輸入與下一步

- 真擴展消費：EXT-023 需對已修的原 feed ACK 再測；EXT-022／037／038／039／049 需消費最新管理、權限與用量候選。未取得回條，不凍結穩定協議。
- 外部能力：EXT-004／006 附件授權與 fetch、EXT-020 gVisor/execd/PTY 旋轉、EXT-015 agent_target/附件/SSE、SnowLuma 映像 digest 與雙向包仍待真實方法、版本或證據。
- 安裝與切換：CORE-043 等 EXT-041 與核心分發／移轉輸入；044 等 EXT-067 B0 實測；045 等 EXT-043 已發布 tuple 與 044；046 等 EXT-044／045 安裝證據；047、053、054 依各前置。CORE-041／042 等 CORE-040 等價切片判斷；不把未滿足前置的卡標完成。
- 下一個可獨立工作：有效期間乾淨候選已完成；`codex/core-033-connector-auth` 已實作 EXT-011 明確要求的未選 Work 裝置認證，三平台候選已驗，待 EXT-011 對拍。這是核心生產者應補的入口，不列為等待擴展提供的方法。Client9 明選入口／會話已產出候選；接續原 Social Outbound 斷線續讀、附件與 send／workspace 的本倉責任；CORE-040 仍需完整等功能切片。擴展倉保持只讀。

PR #77 的 Windows release 案例在 Manager 換代時讀 `ready.json` 遇到分享鎖。測試現在等原檔可讀再核對新 PID、原 Work 和請求。HJW 在同一檔案重現 errno 13 並驗證解鎖後讀到原值，見 `artifacts/ci/branch-failures-02/manager-sharing-witness.json`。Mac CORE-039 148 項通過，見 `artifacts/CORE-039/manager-ready-reader-01/`。

CI 排隊修正（[PR #83](https://github.com/Institute-of-Agent-Life/yuki-core/pull/83)，`codex/ci-pr-queue`）：40 個開放 PR 的當時 head 有 58 個 Intel Mac job 在等。逐筆重查已過期的 45 個 run，GitHub 確認取消 43 個；2 個已有 job 執行，保留。以其他任務分支為基底的 PR 先跑 Linux 全套；以 main 為基底或轉向 main 的 PR 跑 Windows、Linux、Apple Silicon macOS 三平台完整合併檢查。同 PR 新提交取消舊任務。main 推送及手動完整檢查保留 Intel Mac。

`python ci/suite.py --out <新目錄>` 從 `ci/checks.json` 取完整清單。每台先跑含 release 的卡、fmt、clippy、writer tests、debug／release build；兩個分開的程序執行其餘案例，同一工作內依來源與 SHA-256 綁定編譯證據。快取只供 Cargo 依賴及編譯中間物，仍執行本倉的編譯與測試。卡片的斷言、期限及具名輸出保留。失敗或未跑完時，`suite.json` 列出缺口，整體回非零。SQLite 歷史測試庫分 migration 提交；reader 5、20、29 的 schema／資料相同，注入錯誤可回滾。原 Mac CORE-032 曾等不到 Rust 回呼；兩段獨立案例現在等前一回合完成，失敗時保留操作與預算診斷。佇列與測試紀錄在 [artifacts/ci](/Volumes/huawei/項目實戰/yuki-core/artifacts/ci/)。
