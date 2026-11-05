# 核心實作進度

CORE-023 CI 補修：完整原工具宣告已超過舊 fixture 首輪上限；明示 64,000 容量並放大必要未觀察輪，保留硬上限拒絕並加驗恰好一次物理 attempt。Mac／Linux 全卡通過，Windows 驗證中；詳見 [故障與修正證據](compact-capacity-fixture.md)。

任務書 v3.2 ZIP SHA-256：`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。舊倉只讀，行為基線 `329ca7c487414d2f54ddd98ff4727446ed2446cd`；K05 補正已套用工作副本，不增加 unknown 重試權。原提交、驗證、失敗診斷及完整交接雜湊見 [實作驗證記錄](implementation-evidence.md)。

## 完成與進行中

已合併 main：CORE-001～027、031～033、039、049、052、056 的原卡實作；目前 main `76cf762`。後續原工具相容性補核見下表；實作與離線驗證不替代真擴展消費或實際供應商驗收。

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
| CORE-026 metadata | `codex/core-026-memory-metadata`，本次提交 | Mac 主卡 370 項、9 張相鄰卡與原版 7 項通過；reader 44 升級保原回執，維護 Host 原投影已回歸，三平台乾淨候選待產。 |
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

CORE-037 權限目錄候選根目錄：`/Volumes/huawei/項目實戰/yuki-core/artifacts/CORE-037/permissions-handoff-final-01/`，來源 `a638fab4066b2913a2d4f00171ba0b4fe78a5e83`、tree `61a8622b8c3f84b88743b096e36804aed5a4f22ce2164ba16600654db2c2181f`；三平台 ZIP 與逐成員雜湊已驗，Host 2／reader 43／SDK alpha.11、a11；候選尚未發布。其他 Host／Code／媒體／Profile／分發候選的固定路徑與簽章見 [實作驗證記錄](implementation-evidence.md)。

唯一已授權並完成的發布：[handshake-preview-dd694a0db605](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/handshake-preview-dd694a0db605)，僅 initialize；ZIP SHA-256 `6e2ca1040f6d1eac2f70b0ff363887faf662782f72a7acd367d4a52a17b2caa2`。EXT-002 兩 SDK Windows 消費已通過。

## 缺少輸入與下一步

- 真擴展消費：EXT-023 需對已修的原 feed ACK 再測；EXT-022／037／038／039／049 需消費最新管理、權限與用量候選。未取得回條，不凍結穩定協議。
- 外部能力：EXT-004／006 附件授權與 fetch、EXT-031 vision slot/version/bytes、EXT-020 gVisor/execd/PTY 旋轉、EXT-011 Work 外設備撤權、EXT-015 agent_target/附件/SSE、SnowLuma 映像 digest 與雙向包仍待真實方法、版本或證據。
- 安裝與切換：CORE-043 等 EXT-041 與核心分發／移轉輸入；044 等 EXT-067 B0 實測；045 等 EXT-043 已發布 tuple 與 044；046 等 EXT-044／045 安裝證據；047、053、054 依各前置。CORE-041／042 等 CORE-040 等價切片判斷；不把未滿足前置的卡標完成。
- 下一個可獨立工作：完成 metadata 驗證候選後，繼續補原 memory_change 的 selector／其他狀態、send／workspace 參數的本倉責任；推進 CORE-040 完整原切片與冷暖對照。擴展倉保持只讀。
