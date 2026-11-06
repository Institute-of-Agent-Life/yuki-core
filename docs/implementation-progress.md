# 核心實作進度

CORE-040（`codex/core-040-private-fallback`）的公開 Compact 提案遭拒後，Rust 在已拒同一公開前綴的狀態下，與固定 A/B 一致走四次模型請求。六段已發 QQ 回條、私人摘要、未覆蓋舊歷史一併進入最後一次正常請求。原版注入規劃失敗，Rust 的先行拒絕另存為 setup。Mac CORE-040 共 209 項通過，見 `artifacts/CORE-040/private-fallback-check-01/slice-decision.json`。完整功能集與效能裁決仍在進行。

CORE-013／040（`codex/core-040-automatic-replay`）已修復六段長文字的模型回條。舊版 A／B 的原始 Chat 請求與資料庫事實在 `artifacts/CORE-040/automatic-original-{A,B}-01/`。Rust 曾只返回派發摘要；修正後 96,000 token 門檻觸發公開 Compact，接續請求逐段帶回六個已確認事件。第三段在下一次預留時遭撤回，該預留的實體派發數為零。Mac 已驗 CORE-040（186 項）與 CORE-013（350 項），產物各在 `artifacts/CORE-{040,013}/automatic-split-check-01/`。精簡測試驅動後另跑兩條時序，32 項通過，見 `artifacts/CORE-040/automatic-native-09/`。原始 P 規劃拒絕後走 E 的 Rust 對拍仍待驗。

截至 2026-11-06 04:20（洛杉磯），main 已合併 #83、#39、#40（`5557e35`），三端逐次各驗 36、37、38 卡。回執在 `artifacts/ci/local-pr/{83,39,40}/receipt.json`。#43 的 Linux 回執已完成，仍待完整三端合併檢查。Mac 的舊原生編譯產物和內建 Docker 增量快取合計釋出約 85 GiB；驗證在 `artifacts/ci/cleanup-20261106/`。

CORE-013（#85，`codex/core-013-file-send-receipt`，來源 `04139ad30742`）的三端候選均已打包。每端 CORE-013 通過 349 項、CORE-040 通過 152 項。Windows 檔案傳送回條和重放分別保存在 `windows/manifest.json`、`windows/related-core040/manifest.json`。候選可供擴展倉唯讀對拍，狀態為未發布，schema 51，Rust SDK 1.0.0-alpha.14，Python SDK 1.0.0a14。各包位於 `artifacts/CORE-013/file-caption-04139ad30742/<平台>/handoff/`，實含方法以包中清單為準。

| 平台 | ZIP | SHA-256 |
| --- | --- | --- |
| macos | `yuki-social-preview-04139ad30742-darwin-arm64.zip` | `2e8064dcbb9cc77cf0640c49ba417905f5962b9bbc1273937e459f0e792041cf` |
| linux | `yuki-social-preview-04139ad30742-linux-arm64.zip` | `d5cf02c45034ac3e97f5a5067bd34a24b78b9e42038fd1483bcba85693f81a17` |
| windows | `yuki-social-preview-04139ad30742-windows-x86_64.zip` | `75f871ed9889c0ca13c4da2712448d0a277635d4f917bdf41dad886768af51b4` |

三端清單 `artifacts/CORE-013/file-caption-04139ad30742/verified-candidates.json` 的 SHA-256 為 `561bdbfdf1bdc0bc1084803021496438b40a569123812002e14fc3de5ae6b708`。

CORE-040 `a5b7fb71f55b`（`codex/core-040-long-work-matrix`），固定舊版 A／B 與 Rust C 各跑 20 個 Work。60 個 Work 共 360 次模型請求；20 組在初始 QQ 內容、原問題與手機回答的角色、檔案及文字回條等本輪觀察項目均相符。`artifacts/CORE-040/long-work-matrix-C-02/long-work-matrix.json` SHA-256 `78d37cc6547cb486fb3874c1dd92fe25869bab7efd377697d3ebe474ebc1188e`。這一組是 Mac 對照；CORE-040 全切片與效能裁決仍待驗。下一步為完成 #43 的本地合併檢查及 Windows 檔案回條候選。EXT-067 的 `base-consumer-witness.json` 仍是穩定協議凍結的必要輸入。

CORE-013 檔案加文字回條（`codex/core-013-file-send-receipt`）。已確認上傳和文字的原始事件分列，附件綁定原版本雜湊、檔名和大小。固定 v4 A/B 對照完整模型可見回條；先前 Rust 缺少 data，修正後 Mac CORE-040 152 項、CORE-013 348 項通過。預留下一模型請求之後移除原檔版本，實體派發次數維持六次；原三筆 QQ 效果無重送。產物 `artifacts/CORE-040/file-caption-wire-01/` 與 `artifacts/CORE-013/file-caption-withdrawal-01/`。

日常 CI 已於 #83 合併 main `b2cfe37`，Mac mini、同機 Linux ARM64 容器與 HJW Windows 各驗 36 卡；#39 於 main `d74e70e` 各驗 37 卡。合併回執在 `artifacts/ci/local-pr/{83,39}/receipt.json`。Mac 清理已合併分支的舊增量及依賴編譯檔約 64.1 GiB，詳見 `artifacts/ci/cleanup-20261106/`。

CORE-040 本地對拍交接包 `artifacts/CORE-040/work-question-b5bc5fd0a5a7/yuki-core-work-replay-evidence-b5bc5fd0a5a7.zip`，SHA-256 `a6c7245177c5543b728294b6fa581d26c8593b6fe81e9705477556e846164b70`。來源 `b5bc5fd0a5a7`，schema 51，SDK alpha.14；三端各 150 項。包內含原始雙臂的六次請求、三平台清單及修正結果，未發布。

CORE-040 跨裝置續接（`codex/core-040-work-replay`）。固定 v4 A/B 各重放同一個 Work 的六次實際模型請求。Mac 修正前連續三次遺失已確認的提問，修正後兩次冷啟動均保留原問題與手機答案，撤回原問題則停止下一次派發。Mac 150 項斷言見 `artifacts/CORE-040/work-question-fixed-01/manifest.json`。本卡仍待完整等功能與效能判定。正常與故障時序見 [Work replay](benchmarks/work-replay.md)。

CORE-023 CI 補修：完整原工具宣告已超過舊 fixture 首輪上限；明示 64,000 容量並放大必要未觀察輪，保留硬上限拒絕並加驗恰好一次物理 attempt。Windows／Mac／Linux 全卡通過；詳見 [故障與修正證據](compact-capacity-fixture.md)。

任務書 v3.2 ZIP SHA-256：`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。舊倉只讀，行為基線 `329ca7c487414d2f54ddd98ff4727446ed2446cd`；K05 補正已套用工作副本，不增加 unknown 重試權。原提交、驗證、失敗診斷及完整交接雜湊見 [實作驗證記錄](implementation-evidence.md)。

CORE-037：修正歷史資料庫 fixture 的逐條落盤；[HJW、Mac、Linux 各 1052 項及超時診斷](control-fixture-timing.md)。

CORE-040 同封包對拍（`codex/core-040-native-arrivals`）：[重現與故障時序](benchmarks/native-arrivals.md)。兩份固定原版與 Rust 各 4 筆輸入／8 次物理 Chat；同一 QQ 訊息的發言者、原始時區、提及、正文、靜態／runtime 提示與模型參數已對齊，修正 Rust 把自身提及顯示為未知成員。Mac 實測 `artifacts/CORE-040/native-arrival-check-03/slice-decision.json`；原 A/B 在 `artifacts/CORE-040/native-arrivals-04/`。尚缺 9 個工具、5 個額外工具、4 個宣告差異，以及原 send 回條；未作完整 E13 投資判斷。下一步補核模型實收的 Social 回條與真擴展工具視圖。

[PR #78](https://github.com/Institute-of-Agent-Life/yuki-core/pull/78) 固定來源 `8dfa2ae520dba4063ab80fb4d6e907b6eae369c5`：乾淨 HJW Windows、Mac、Linux 各 9 項；`artifacts/CORE-040/native-arrival-clean-01/verified-platforms.json` SHA-256 `786f796b6b9719c116ae39f9561d03f3c1ca1cc03f0826781e60df779d2231e5`。另以相同封包重跑完整 E13：A、B、C 各 1200 筆、2400 次實體 Chat、1200 段送達和 1200 筆原出站記錄。逐筆外部效果索引 `artifacts/CORE-040/native-e13-effects-01/external-effects.json` SHA-256 `fb3e6163cc2aca637cb03a90e6cb731021ac9fbe4f0f9865dfd1c778ac3c7c8b`；同時執行，只核效果。

CORE-013 單段文字送達回條（`codex/core-013-model-send-receipt`）：[模型可見字段與撤回競態](model-send-receipt.md)。原版 14 項通過。Mac CORE-013 通過 314 項；CORE-040 的 10 項包含真實第二次 Chat 與舊版實收欄位逐項對拍。CORE-011／012／016／023／014／038／039 各 117／166／184／225／667／318／148 項通過。證據 `artifacts/CORE-*/model-send-receipt-check-05/`。實測在正常預留後撤回正文，原邏輯請求零物理 attempt，沒有重發 QQ。多段和未知送達沿用原有物理狀態。

CORE-013 未發布候選（PR #79，來源 `9597513f82d3f42a4ababcb750362bf15326ac98`；schema 51，SDK alpha.14）：HJW Windows、macOS、Linux 各通過 314 項。交接清單 `artifacts/CORE-013/send-receipt-handoff-final-01/verified-candidates.json`，SHA-256 `e0fb3ed1562270cf8e75b8b796653b900e0063ee689105475c13f9aaed290bdc`；內含各 ZIP 的絕對路徑、SHA-256 及逐檔驗證記錄。

CORE-040 連續兩次公開 Compact（`codex/core-040-compact-replay`）：[共同輸入及重現方式](benchmarks/compact-replay.md)。正常／第二份摘要帶禁用工具兩條時序，A／B／Rust 各 30 次實體 Chat；逐次核對哪些原文、摘要及當前問題真正進入模型。第二份失敗時第一份摘要仍有效，沒有送出 QQ，沒有補買。舊版原始回條 `artifacts/CORE-040/compact-two-{a,b}-03/`；Rust `artifacts/CORE-040/compact-replay-check-01/slice-decision.json`。此處只驗公開手動壓縮，E13 的自動 P/E、長 Work 工具與等資源比較仍待核對。

[PR #81](https://github.com/Institute-of-Agent-Life/yuki-core/pull/81) 固定來源 `d8a10ba67270b8d2ff2c936ade3e267c0d9e9fbe`，HJW Windows、Linux、macOS 各 84 項通過；三平台實際模型時序、公開摘要及費用帳本索引 `artifacts/CORE-040/compact-replay-clean-01/verified-platforms.json`，SHA-256 `bb518eec24eea3f7c8126b69fe4edaf41214161e11c0f8e06c6fd394f9710b53`。

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

CORE-013 目標：`codex/core-013-send-entry`；[原 QQ 目標、時序及界線](social-send-entry.md)。固定舊版四例、Rust Mac 013=296、014=667、032=129、015=363、023=225、026=1181 項通過。四條實際 QQ 動作、五個零派發拒絕（含委派 Work），原網關亦驗證錯誤選擇不抵達插件。Mac 及固定源回條在各卡 `canonical-send-delegation-01`、`send-entry-integration-01` 與 `artifacts/CORE-013/original-send-entry-oracle-01`。來源 `fece2511985036e6e349c43e1b437562dd4fac42`，Mac、Linux、HJW 各 296 項；[未發布候選及各包 SHA-256](/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-013/send-entry-handoff-final-01/verified-candidates.json)，索引 `79f6db0b19318987d2a300dc0ec789e35830feba5e6226cb4384d20464d85c9f`。

## 缺少輸入與下一步

- 真擴展消費：EXT-023 需對已修的原 feed ACK 再測；EXT-022／037／038／039／049 需消費最新管理、權限與用量候選。未取得回條，不凍結穩定協議。
- 外部能力：EXT-004／006 附件授權與 fetch、EXT-020 gVisor/execd/PTY 旋轉、EXT-015 agent_target/附件/SSE、SnowLuma 映像 digest 與雙向包仍待真實方法、版本或證據。
- 安裝與切換：CORE-043 等 EXT-041 與核心分發／移轉輸入；044 等 EXT-067 B0 實測；045 等 EXT-043 已發布 tuple 與 044；046 等 EXT-044／045 安裝證據；047、053、054 依各前置。CORE-041／042 等 CORE-040 等價切片判斷；不把未滿足前置的卡標完成。
- 下一個可獨立工作：有效期間乾淨候選已完成；`codex/core-033-connector-auth` 已實作 EXT-011 明確要求的未選 Work 裝置認證，三平台候選已驗，待 EXT-011 對拍。這是核心生產者應補的入口，不列為等待擴展提供的方法。Client9 明選入口／會話已產出候選；接續原 Social Outbound 斷線續讀、附件與 send／workspace 的本倉責任；CORE-040 仍需完整等功能切片。擴展倉保持只讀。

CORE-018 與 CORE-013 交叉驗證：排程私聊、SELF 群聊的文字送達，均對照原 QQ ACK 的操作 ID、事件、對話、Presence 及目的類型。PR #79 與 #81 原 CI 讀取舊回條欄位而失敗；Mac 已驗 103 項，見 `artifacts/CORE-018/schedule-model-receipt-01/manifest.json`。

CI 排隊修正（[PR #83](https://github.com/Institute-of-Agent-Life/yuki-core/pull/83)，`codex/ci-pr-queue`）：40 個開放 PR 的當時 head 有 58 個 Intel Mac job 在等。逐筆重查已過期的 45 個 run，GitHub 確認取消 43 個；2 個已有 job 執行，保留。以其他任務分支為基底的 PR 先跑 Linux 全套；以 main 為基底或轉向 main 的 PR 跑 Windows、Linux、Apple Silicon macOS 三平台完整合併檢查。同 PR 新提交取消舊任務。main 推送及手動完整檢查保留 Intel Mac。

`python ci/suite.py --out <新目錄>` 從 `ci/checks.json` 取完整清單。每台先跑含 release 的卡、fmt、clippy、writer tests、debug／release build；兩個分開的程序執行其餘案例，同一工作內依來源與 SHA-256 綁定編譯證據。快取只供 Cargo 依賴及編譯中間物，仍執行本倉的編譯與測試。卡片的斷言、期限及具名輸出保留。失敗或未跑完時，`suite.json` 列出缺口，整體回非零。SQLite 歷史測試庫分 migration 提交；reader 5、20、29 的 schema／資料相同，注入錯誤可回滾。原 Mac CORE-032 曾等不到 Rust 回呼；兩段獨立案例現在等前一回合完成，失敗時保留操作與預算診斷。佇列與測試紀錄在 [artifacts/ci](/Volumes/huawei/項目實戰/yuki-core/artifacts/ci/)。

PR #79 整合新的 CI 入口。Mac 完整驗證通過 49 張卡，見 `artifacts/ci/pr79-suite-mac-02/suite.json`。第一次複測時 Rust 消費者離線，未取得當次退出原因。CORE-032 現在等待該消費者完成 SDK 握手，並保存握手狀態或退出錯誤。四次獨立啟動均完成實際工具呼叫，見 `artifacts/CORE-032/pr79-startup-probe-01/` 至 `04/`。同步採用 #77 驗過的 Windows Manager 回條讀取方式，HJW 原生 CORE-039 的 150 項驗證見 `artifacts/CORE-039/manager-ready-reader-windows-01/manifest.json`。

PR #81 同步 #79 的發送回條與 CI 修正，保留兩次 Compact 的原始重播。Mac 49 張卡全部通過，包含 CORE-018、032、039 與 040，見 `artifacts/ci/pr81-suite-mac-01/suite.json`。HJW 原生 Windows 另完成 CORE-018 共 103 項斷言，見 `artifacts/CORE-018/schedule-model-receipt-windows-02/manifest.json`。
