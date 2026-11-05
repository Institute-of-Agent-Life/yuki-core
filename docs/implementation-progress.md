# 核心實作進度

來源：任務書 v3.2 核心文檔補核版，ZIP SHA-256
`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。
舊行為基線：`329ca7c487414d2f54ddd98ff4727446ed2446cd`，舊倉只讀。
已依總書 K05 修正工作副本 CORE-011 第 2 步，不增加 unknown 重試權。

| 任務 | 狀態 | 分支／提交 | 驗證及產物 |
|---|---|---|---|
| CORE-004 | 完成，已進 main | `codex/core-004-qq-keys` | `artifacts/CORE-004/{mac-02,windows-01}/qq-key-decision.json`。提交 `aa2dca0`、[PR #5](https://github.com/Institute-of-Agent-Life/yuki-core/pull/5) 三平台 CI 通過，合併 `2bc490c`。現場僅見 QQ，指定唯一接收端；同秒同文、亂序、回聲與重連雙收 fixture 是合成對照。 |
| CORE-001 | 完成，已進 main | `c85bf8ee182d5075c1abaa8b1a56f51e16b0a196`；分支 `codex/rust-core-v3-2` | [三平台 CI 41818720821](https://github.com/Institute-of-Agent-Life/yuki-core/actions/runs/41818720821) 全過；`artifacts/CORE-001/` 保留正常、故障與修正證據 |
| CORE-005 | 完成，已進 main | `codex/core-005-owner`，PR head `f13b29d`，合併 `d98b02e` | Windows、macOS、Linux [CI 通過](https://github.com/Institute-of-Agent-Life/yuki-core/actions/runs/41825696747)。實機 `artifacts/CORE-005/windows-05/`、`windows-sid/`、`mac-sigterm-green/` 與 `linux-container-01/` |
| CORE-002 | 完成，已進 main | `codex/core-002-oracle` | `artifacts/CORE-002/capture-mac-01/{first,second}/` 各 5 個原版情境，Windows 實機重放兩次也一致。Windows 實機完整 A 線同樣通過，見 `artifacts/CORE-002/v4-oracle-979bac82/load/`。提交 `89f15db`，[PR #2](https://github.com/Institute-of-Agent-Life/yuki-core/pull/2) 三平台 CI 通過並合併 `2ea4477`。A 線 `load-mac-01/` 為 1,200 條逐筆送達與 20 個原 Work。三平台入口均為 `python ci/check.py CORE-002 --out <新目錄>` |
| CORE-003 | 完成，已進 main | `codex/core-003-inventory` | `artifacts/CORE-003/` 的 `mac-02/`、`windows-01/` 均通過，提交 `9c77e56`、[PR #3](https://github.com/Institute-of-Agent-Life/yuki-core/pull/3) 三平台全過並合併 `83d8583`，另留 red、WAL、缺 blob、拒絕路徑逸出證據 |
| CORE-006 | 完成，已進 main | `codex/core-006-person` | `artifacts/CORE-006/mac-02/`、`windows-01/` 中的 `person-before-after.json` 保留舊庫升級、原人物與停用兩端的授權結果。[PR #4](https://github.com/Institute-of-Agent-Life/yuki-core/pull/4) 三平台通過，head `541ee01`，合併 `69acc507`。 |
| CORE-007 | 完成，已進 main | `codex/core-007-assignments` | `artifacts/CORE-007/mac-02/` 與 `windows-01/` 的 `assignment-timeline.json`；原生時間可信區間、轉回、關閉私聊和重試在真 SQLite 上驗證。[PR #6](https://github.com/Institute-of-Agent-Life/yuki-core/pull/6) 三平台通過，head `9da1673`、合併 `0783298`。 |
| CORE-049 | 完成，已進 main | `codex/core-049-profile-evidence` | `artifacts/CORE-049/{mac-02,windows-01}/endpoint-evidence-grid.json` 與原 adapter 7 個通過的案例。外網模型呼叫 0，實際供應商能力待驗。提交 `b4de4e5`、[PR #7](https://github.com/Institute-of-Agent-Life/yuki-core/pull/7) 三平台通過，合併 `2a680e3`。 |
| CORE-008 | 完成，已進 main | `codex/core-008-ingress` | `artifacts/CORE-008/{mac-03,windows-02}/ingress-barriers.json` 留有實際程序在 commit 前、commit 後 ACK 前終止及冷啟重投的結果。[PR #8](https://github.com/Institute-of-Agent-Life/yuki-core/pull/8) 三平台通過，head `3b8b6c5`、合併 `ff7122a`。 |
| CORE-009 | 完成，已進 main | `codex/core-009-onebot` | `artifacts/CORE-009/{mac-07,windows-02}/qq-frames.json` 保留原提及、實收 locator、回聲及分段打字時序。合成雙端證據只適用該測試部署。[PR #9](https://github.com/Institute-of-Agent-Life/yuki-core/pull/9) 三平台通過，head `9f59267`、合併 `a2e9bfa`。 |
| CORE-010 | 完成，已進 main | `codex/core-010-effects` | `artifacts/CORE-010/{mac-03,windows-02}/effects-ledger.json` 記錄兩次本地 commit 中斷、原結果及三段外部實收。[PR #10](https://github.com/Institute-of-Agent-Life/yuki-core/pull/10) 三平台通過，head `4458777`、合併 `bf912a6`。 |
| CORE-031 | 完成，已進 main | `codex/core-031-handshake` | `artifacts/CORE-031/{mac-03,windows-02}/sdk-crosswire.json`。schema 1、Rust `1.0.0-alpha.1`、Python `1.0.0a1`；兩端的預覽 ZIP 逐位元相符。[PR #11](https://github.com/Institute-of-Agent-Life/yuki-core/pull/11) 三平台通過，head `cfbff5b`、合併 `dd694a0`。 |
| CORE-011 | 完成，已進 main | `codex/core-011-budget` | `artifacts/CORE-011/{mac-05,windows-03}/` 的 `paid-boundaries.json` 與 `usage-denominator.json`。根競爭、原回應保存、重試實收與 unknown 分母均以實際 SQLite／loopback 驗證。[PR #13](https://github.com/Institute-of-Agent-Life/yuki-core/pull/13) 三平台通過，head `208fc63`、合併 `ed971f0`。 |
| CORE-052 | 完成，已進 main | `codex/core-052-source-fence` | `artifacts/CORE-052/{mac-02,windows-01}/source-erasure-obligations.json`，各 94、94 項斷言。CORE-011 回歸 117 項通過。[PR #14](https://github.com/Institute-of-Agent-Life/yuki-core/pull/14) 三平台通過，head `555b6c9`、合併 `8493625`。 |
| CORE-056 | 完成，已進 main | `codex/core-056-group-recovery` | `artifacts/CORE-056/{mac-03,windows-02}/native-group-recovery.json`。兩端各 55 項斷言；CORE-009 的 49 項入站回歸通過。[PR #15](https://github.com/Institute-of-Agent-Life/yuki-core/pull/15) 三平台通過，head `3ae3b37`、合併 `fb57fcf`。`artifacts/CORE-056/{mac-proof-01,windows-proof-01}/` 另驗畸形群號及回應指向另一個群，均通過。 |
| CORE-012 | 完成，已進 main | `codex/core-012-main` | `artifacts/CORE-012/{mac-08,windows-03}/` 下的 `main-wire.json` 及 H01、H02、H03、H16、H20 回條。Mac／Windows 各 162 項斷言。以 HTTP recorder、合成工具接收程序及實際 writer 測試。CORE-008、010、011、052 回歸通過。[PR #17](https://github.com/Institute-of-Agent-Life/yuki-core/pull/17) 三平台通過，實作 `37a4f42`、最終 head `bce43bc`、合併 `dfaba17`。 |
| CORE-013 | 三平台 CI 通過，已合併 aa67bed | `codex/core-013-social` | `artifacts/CORE-013/{mac-09,windows-03}/social-parts.json`。[PR #18](https://github.com/Institute-of-Agent-Life/yuki-core/pull/18)，head `d2942ef`。兩端各 108 項斷言，另有固定舊版分段對拍。兩次 macOS ARM 託管作業未取得 runner；已改用標準 macOS Intel runner 執行同一組檢查，本機 Mac ARM 結果另列。三段送達、轉移後 ACK、原生定位撤回與配圖中斷均有回條。 |
| CORE-019 | 三平台 CI 通過，已合併 0687bb6 | `codex/core-019-responses` | `artifacts/CORE-019/{mac-03,windows-02}/responses-dialect.json`。[PR #19](https://github.com/Institute-of-Agent-Life/yuki-core/pull/19)，head `d15c942`。兩端各 154 項斷言；舊版兩個 adapter 的真 HTTP 回條與現行文件聲明分開，三種方言的 Rust 測試均用 loopback。 |
| CORE-014 | 三平台 CI 通過，已合併 ff3644a | `codex/core-014-history` | `artifacts/CORE-014/{mac-06,windows-02}/literal-search-gold.json`，兩端各 96 項斷言。固定舊版字面 gram、簡繁、ASR 邊界、舊庫升級、刪除後翻頁與模型下一次請求的原來源均有回條。`artifacts/CORE-014/linux-container-03/linux-history.json` 是 linux/arm64 斷網非 root 實測。提交 `40e008b`，[PR #20](https://github.com/Institute-of-Agent-Life/yuki-core/pull/20)。CORE-008、011、012、013、019、052 回歸通過。初次遠端打包查出 SDK 的 NOTICE 未同步；同步後 CORE-031 42 項通過，`linux-context-04/manifest.json` 記錄以本倉入口另一次構建的 Linux 映像。 |
| CORE-015 | 三平台 CI 通過，已合併 5c3b494 | `codex/core-015-tools` | `artifacts/CORE-015/{mac-04,windows-02}/{frozen-views,read-barriers,tool-result-pages}.json`，兩端各 105 項斷言。另以具名 Rust 測試驗證兩讀同時進入、隱藏 schema 改版、午夜、短槽到期及解碼後撤權。CORE-005、010、011、012、013、014、019、031、052 回歸均通過。 |
| CORE-020 | 三平台 CI 通過，已合併 00e1f9f | `codex/core-020-messages` | `artifacts/CORE-020/{mac-02,windows-02}/{messages-wire,messages-choice-pause}.json`，兩端各 163 項斷言。固定舊版四次 HTTP 對拍覆蓋 thinking 開關與具名／必選；Rust 驗過簽名原序、失敗工具原 ID、伺服器工具錯誤，以及 pause_turn 用盡根額度。CORE-011、012、014、019 回歸通過。 |
| CORE-032 | 三平台 CI 通過，已合併 `30a5ea7` | `codex/core-032-host` | `artifacts/CORE-032/{mac-08,windows-04}/host-delegation.json`。獨立安裝的 Rust、Python SDK 回呼同一宿主。涵蓋分段失答、離線命令與冷啟、撤權、觀察背壓、刪除回條。握手 v1、Host 業務 schema v2、Rust 1.0.0-alpha.2、Python 1.0.0a2。 |
| CORE-021 | 三平台 CI 通過，已合併 7fd1002 | `codex/core-021-gemini` | `artifacts/CORE-021/{mac-02,windows-02}/{gemini-wire,gemini-parts}.json`，各 100 項斷言。舊版兩次真 HTTP 錄製包含有／無原生 ID 的平行回條。Rust 驗過簽名位置、原生搜尋失敗、來源引用、媒體、拒絕混用及傳輸中斷；CORE-011、012、019、020 回歸通過。 |
| CORE-039 | Mac ARM64、HJW Windows x86_64、Linux ARM64 各通過 81 項，Linux 最小 OCI 亦通過 | `codex/core-039-dist` | `artifacts/CORE-039/{mac-04,windows-03,linux-arm64-03}/candidate-manifest.json` 和 `oci-dev-02/oci-candidate.json`。release 執行檔、兩 SDK、IDL、fixture、Cargo 依賴與授權按來源簽名。空目錄啟動，錯來源、錯架構、錯金鑰、錯 fixture、路徑逸出、連結及額外檔案均拒絕。 |
| CORE-016 | Mac 與 HJW Windows 各通過 56 項 | `codex/core-016-work` | `artifacts/CORE-016/{mac-06,windows-03}/{work-tree,settlement-tail,work-action-matrix}.json`。H04 在真收件後殺程序，重啟補原回條，維持一次送達、兩次模型請求。H10 核對中立前景、派生 worker 與跨樹權限。另驗私有問答、同批取消與遲到回條、空 final 及 unknown 後的新業務輸入。11 張相鄰卡回歸通過，見各卡 `work-regression-01/`。 |
| CORE-026 | Mac、HJW Windows 各通過 118 項；已驗 13 張相鄰卡 | `codex/core-026-facts` | `artifacts/CORE-026/{mac-04,windows-02}/fact-evidence-acl.json`。共同群可讀結構化事實而不可讀來源私聊；原 R1 重放不覆寫 R2，原證據換列後舊 proof 被拒。實測 QQ 本人命令、原模型 read_memory／memory_change、撤回、工具／run 原件及 forget。 |
| CORE-017 | macOS 與 HJW Windows 已驗，待三平台 CI | `codex/core-017-wait` | `artifacts/CORE-017/{mac-11,windows-02}/wait-{match,before-settle,parent-questions,plugin-events}.json`。原批結算後才掛起；原定位與身份命中後一次啟動，保留 all 的部分私有輸入。十次提示與重放均不增購；父工作終止後不復活。App／ACP 明確目標按核心入口驗證，實際擴展消費仍待交接。CORE-015 工具摘要及相鄰 13 張回歸通過。 |
| CORE-022 | macOS 與 HJW Windows 各通過 214 項，待 CI | `codex/core-022-compact` | `artifacts/CORE-022/{mac-07,windows-02}/{compact-public-next-wire,native-public-wires,compact-public-faults}.json`。送後跨 96k，只換公開前綴；保留原工具配對與未覆蓋尾。實際強殺後重用原已付回應；刪除、重置或撤管理權後舊 P 無法發布。三種原生協議均驗下一筆實際請求與 opaque 重放。 |
| CORE-018 | macOS、HJW Windows 實機已驗，待 CI | `codex/core-018-schedule` | `artifacts/CORE-018/{mac-03,windows-01}/schedule-occurrences.json`。H08 強殺接納後的 writer，重啟以原 goal 與原 QQ 私聊收件端完成；兩次到期有獨立根執行，重掃不重購。SELF 經指定群接收階段實際送達。R14 驗編輯、暫停、漏過多次到期、已完成次數，以及轉移後拒送。五種原規則、另加 cron 與舊時鐘對拍。18 張相鄰卡回歸通過。 |
| CORE-023 | Mac、HJW Windows 各通過 223 項，待 CI | `codex/core-023-private` | `artifacts/CORE-023/{mac-06,windows-01}/{compact-exec-next-wire,partial-native-next,native-E-P-next,next-normal-unknown}.json`。同一批在配對後與 E 回應落庫後各強殺一次，下一普通請求仍有原參數及效果鍵。原生 QQ 部分送達保留三段狀態、已送 locator 與出站 ID。另驗 E→P 換鏈、P 拒絕轉 E、來源失效、不可容納及 Work 喚醒。固定舊版 `v4-oracle-01/junit.xml` 的 21 項通過。 |
| CORE-025 | Mac 與 HJW Windows 各通過 83 項，待 CI | `codex/core-025-media` | `artifacts/CORE-025/{mac-03,windows-02}/media-versions.json`、`paid-image-wire.json`、`paid-messages-image.json`。原 QQ 段與引用、不可變交付、替換路徑、Windows junction、硬連結、忘記在途來源、TTL、無視覺能力均已實測。Mac 22 張卡回歸通過，見各卡 `media-regression-02/`。 |
| CORE-027 | HJW Windows、Mac 各通過 172 項，待 CI | `codex/core-027-extract` | `artifacts/CORE-027/{mac-04,windows-01}/{extraction-batches,extraction-wire}.json`。按原事件數、字數、等待年齡選批；上下文只消歧。真實前景執行先行，提取使用專屬 schema。原文／ASR、三種事實分域、同 key、零結果、強殺與寫庫故障均驗過。未保存回應、SELF 偽造、來源刪除都不能推進水位。 |
| CORE-028 | Mac 與 HJW Windows 各通過 93 項，待 CI | `codex/core-028-vectors` | `artifacts/CORE-028/{mac-03,windows-01}/{vector-coverage,physical-embedding-wire}.json`。標準 hybrid 要求有效配置；純詞法須明選。實測相同維度不同模型、部分批次只補缺項、在途改文和 forget、查詢共用與斷線未知不重購。25 張卡回歸通過，見各卡 `vector-regression-01/`。 |

**擴展交接。** EXT-007、EXT-019 可讀 CORE-002 執行產物
`v4-oracle-candidate.zip`；本地路徑 `artifacts/CORE-002/handoff-89f15db/v4-oracle-candidate.zip`，
SHA-256 `ccdefae10242b19d2f1a201fcd59f5567c08395fd0c051c3d4e92282f6434df0`。
同目錄 `v4-oracle-index.json` 記錄來源提交。
內容含 Client9、Admin OpenAPI、Connector 0.4.3 的 wire4 錄製、舊 Main 和 Work 行為及 A 線。
此包未發布。擴展提交 `9d1e8a8` 釘住相同 ZIP；EXT-007 對拍 124 個訊框、EXT-019 核對 Desktop 原始呼叫點。見[擴展來源鎖](https://github.com/Institute-of-Agent-Life/yuki-extensions/blob/9d1e8a8a8978b0da66a66e40a6a81c8da0116f75/compat/core-002/source.lock.json)、[使用端交接需求](https://github.com/Institute-of-Agent-Life/yuki-extensions/blob/9d1e8a8a8978b0da66a66e40a6a81c8da0116f75/docs/core-interface-requests.md)。
Rust 插件的 B0 尚缺同版宿主與真正插件的消費證據。

**EXT-002 握手交接。** 合併來源 `dd694a0db605bfe13b0d72663bef01599560230b`，
本地包 `artifacts/CORE-031/handoff-dd694a0db605/handshake-preview/yuki-handshake-preview.zip`，
SHA-256 `6e2ca1040f6d1eac2f70b0ff363887faf662782f72a7acd367d4a52a17b2caa2`。
Rust SDK 1.0.0-alpha.1、Python SDK 1.0.0a1、schema revision 1。兩個乾淨來源的 Mac／Windows 重建相同；
各檔雜湊及驗證在同目錄的 candidate.json 和 sdk-verification.json。僅提供 initialize、方法索引、
golden 和獨立 stdio peer。原包於 GitHub 預發布，重新下載的 ZIP 與回條均通過校驗。見[固定發布頁](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/handshake-preview-dd694a0db605)、[ZIP](https://github.com/Institute-of-Agent-Life/yuki-core/releases/download/handshake-preview-dd694a0db605/yuki-handshake-preview.zip)、[發布回條](https://github.com/Institute-of-Agent-Life/yuki-core/releases/download/handshake-preview-dd694a0db605/release-receipt.json)。擴展 `87bc88d` 的 Windows 預覽回條 `artifacts/EXT-002/preview-03/handshake-preview.json` SHA-256 `e34b86d7919a97524c4c737e0402bdeb6ef8575060e5551dcd0ce9c5d0b47f3b`，manifest `368556e4872e697a865983cc4a2fa0a051dfa572ba542a2cd4610433b6170469`。擴展 `2b961bd` 已用發布 URL 在 Windows 重新下載、驗 digest 與安裝。正式 `artifacts/EXT-002/01/published-sdk.json` SHA-256 `5c5d944f126b2866760e7684b7d5f0a8b512fb0bb54b20ee503bc6fb57daf6c4`，manifest `5c1137b8cf5a0a5d91243d33a0826da218f521cf72b870756e9875f46b5e22b8`，兩者 PASS。已合併為 `969a059`。見[擴展 PR #10](https://github.com/Institute-of-Agent-Life/yuki-extensions/pull/10)。

CORE-017 可從已驗 CORE-016 開始。CORE-032 正等正式 Telegram、web-fetch 的相同版本回條；EXT-047 尚缺具名、已核准的 QQ 原生動作。CORE-020 已合併 `00e1f9f`，見 [PR #21](https://github.com/Institute-of-Agent-Life/yuki-core/pull/21)。歷史搜尋的 DJ 校準尚缺目標語料。
缺目標 SnowLuma 映像 digest 和去識別雙端封包；部署預設採指定 ingress。

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

CORE-028 計費補核統一 Embedding 寫入的 `input` 欄位。擦除保留器把舊 `input_tokens` 轉為共同欄位，狀態查詢同時相容兩種原紀錄。Mac 與 HJW 的 `embedding-usage-{regression,windows}-01` 通過實際晚返／來源擦除斷言，來源樹 `53f59231008eca8de8f82b0c0145d80fd4f319471739c5329379d9aace65fefe`。CORE-052、011 回歸通過，舊欄位單元測試回條在 `artifacts/CORE-028/embedding-usage-regression-01/legacy-usage-unit.txt`。既有已丟失的 usage 維持未知。
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
