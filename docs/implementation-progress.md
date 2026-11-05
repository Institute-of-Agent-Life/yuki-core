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
CORE-012 提示詞補核在 `codex/core-012-prompt-order`。原版 HTTP 錄製顯示角色身份、SOUL、執行程式的先後順序。修正後 Mac 與 HJW Windows 各通過 163 項，見 `artifacts/CORE-012/{prefix-mac-01,prefix-windows-01}/main-wire.json`。CORE-014 已合併 [PR #20](https://github.com/Institute-of-Agent-Life/yuki-core/pull/20)。

CORE-012 提示詞補核 [PR #25](https://github.com/Institute-of-Agent-Life/yuki-core/pull/25) 的 head `1eb7be0` 通過六項檢查，已合併 `791e686`。
CORE-024 在 `codex/core-024-search` 實作獨立原生搜索。Responses、Messages 及 Gemini 都用實際 loopback HTTP 驗證來源；一般 Main 透過自己的工具宣告派發，原聊天未送入獨立連線。冷重啟仍可命中完整且未過期的原收據，改 connection、Profile、金鑰域或憑據會重新派發。日期不明、孤立來源和未完成回應分開處理，原邏輯的未知嘗試重啟後維持一次。方法和來源證據在 `artifacts/CORE-024/{mac-07,windows-01}/search-proof-cache.json`；固定舊版橋接 24 項測試在 `artifacts/CORE-024/v4-oracle-01/junit.xml`。EXT-029 尚需以它的真實 Tavily owner 消費核心合同。
兩機各 377 項斷言，驗證來源 `de95a208c5a3464a46e50105baf3801f00f32cd86e2c2072b8350974a8243d80`；10 張相鄰卡的回歸位於各卡的 `native-search-regression-01/`。

CORE-024 的 PR #38 已通過三平台 push／PR 六項檢查，於 main 合併 `905e968c5a04187944bf8f0ff4a3667652221731`。CORE-032 整合此版 main，同時保留原生搜尋與插件工具的凍結宣告；相關卡將以整合後來源重驗。

CORE-032 與 main 搜尋整合後，Mac CORE-032 的 `search-main-integration-01` 65 項、CORE-024 的 `host-main-integration-01` 377 項，以及 CORE-015 的 `search-host-integration-01` 均通過。原插件宣告與搜尋宣告一同經凍結／重名檢查。

CORE-039 已接入 Host `24d40cc` 及 main 的原生搜尋。原生分發與搜尋回歸通過，產物為 `artifacts/CORE-039/search-host-integration-01/`、`artifacts/CORE-024/dist-host-integration-01/`。PR #28 等最新三平台 CI 後進 main。原先候選包仍對應其固定來源 SHA。
CORE-016 已接入 Host `24d40cc` 及 main 的原生搜尋。Work 與搜尋回歸通過，產物為 `artifacts/CORE-016/search-host-integration-01/`、`artifacts/CORE-024/work-host-integration-01/`。PR #26 等最新三平台 CI 後進 main。

已接入 main `30a5ea7` 的 Host 合併提交。實作樹與上一輪驗證相同，PR 已轉向 main 並重新執行 CI。

CORE-039 已接入 main 的 Work `0261764`。Mac 分發與 Work 回歸分別通過，產物為 `artifacts/CORE-039/work-main-integration-01/`、`artifacts/CORE-016/dist-main-integration-01/`。PR #28 以此次整合的最新提交重新等待三平台 CI。
