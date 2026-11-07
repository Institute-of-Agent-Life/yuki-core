# 核心實作進度

2026-11-06。固定 v4 行為基線 `329ca7c487414d2f54ddd98ff4727446ed2446cd`；任務書工作副本的 CORE-011 已採用 K05 的邏輯請求預留規則。下列證據路徑相對於 yuki-core 工作目錄。

**核心任務已完成 49／58 張。** CORE-001–039、044、048–052、055–058 已完成。CORE-040 已取得部分驗收證據，剩餘 9 張的交接條件見下表。[逐卡分支與原產物](https://github.com/Institute-of-Agent-Life/yuki-core/blob/8555755a439b0e67bdda6f00673eaed2ab7502a0/docs/implementation-progress.md)，[先前候選、回放與清理明細](https://github.com/Institute-of-Agent-Life/yuki-core/blob/e680fc23847c8460639294eef7448d27d465b79c/docs/implementation-progress.md)。

| 已合併 PR | head | main 合併提交 | 本地 Mac／Linux／HJW Windows |
|---|---|---|---|
| #43 | `19d29b959cb2` | `82c14715e1bf` | 各 39 卡 |
| #85 | `2385907a1b9a` | `9c17975ed11b` | 各 49 卡 |
| #92 | `808318b5de08` | `fb8806c5f718` | 各 49 卡 |
| #95 | `92dcccde08a6` | `f27749ef4b4f` | 各 49 卡 |
| #96 | `671b5274e7e9` | `7809b26ec3a1` | 各 49 卡 |
| #97 | `2f923d99c6c4` | `9c1c2eba971c` | 各 49 卡 |
| #98 | `17d5015008af` | `2f8785004584` | 各 49 卡 |
| #99 | `4860314c2e8e` | `c6bd0c75e387` | 各 50 卡 |
| #100 | `681c22435609` | `f948f2b1cc7a` | 各 50 卡 |
| #101 | `80de38b9183a` | `4c4ac2348e4d` | 各 50 卡 |
| #102 | `5cfff6988eb4` | `459f845146c0` | 各 50 卡 |

各 PR 的受測合併提交、三端逐卡雜湊在 `artifacts/ci/local-pr/<PR>/receipt.json`。#99 的回執 SHA-256 為 `5abbfe3d54fe3aea254dd96a9a9bc3ed2e61a108078d2c9ca204aea97a811969`。#100 的回執 SHA-256 為 `3da2d2ed7b847f79f437bf8aece3602dec77fd891dbf48140a9100c529a6feeb`。#101 的回執 SHA-256 為 `c40e5f62f60902adaf3d73b875bee00ae410644b9bfc1d1b608ba9f2452858ca`。#102 的回執 SHA-256 為 `71aadf32e4ba5e60c7faa1ac1f1b32d7f3be1d37e746bc53f7c033f3025a22da`。41 個已整合的堆疊 PR 均逐一核對 head 已進入 main，並核驗 147 份清單；見 `artifacts/ci/pr-reconciliation-20261106/closed-41-confirmed.json`。

**三端開發候選（本地未發布）。** #102 原生 Host 來源 `c942da9d39e0e237908cf3a5b9345c6fb1239a2c`，實際合併樹相同。資料庫 reader 54、握手 schema 1、Host schema 2、Rust SDK 1.0.0-alpha.14、Python SDK 1.0.0a14。共 14 個實作方法，完整包路徑與驗簽結果見 `artifacts/CORE-039/pr102/verified-candidates.json`（SHA-256 `2a8a60fe14110e14d83693fc1e2a60721568fa282da86c89be724021ac4a9f93`）。

| 平台 | 本地未發布 ZIP 的 SHA-256 |
|---|---|
| darwin / arm64 | `f198650d0a4ed3f48c8f63c209f1983e8917e9a9c430b689b2c2b7e69cee3fe6` |
| linux / arm64 | `e1b1a828c8f15ad71bf792581f362998a740e934cb7c7d09c5556a7b184a6299` |
| windows / x86_64 | `8b5d0fd40602296cb16c5a0a2b8b0166355b1cfcb96a2d8f51d92ac2f415c745` |

公開的[握手預發布](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/handshake-preview-dd694a0db605)提供 `initialize`。

**CORE-038 真插件。** 三端均以同一平台的原生 Host 啟動 Telegram 與 web-fetch。Telegram alpha.2 收件後缺少發送權，Main 取得失敗回執，原生發送為零。alpha.3 把網頁讀回內容納入 8,197 字元回覆，三段均獲原生定位。第二段已送出但遺失回應時保留 unknown，第三段未派，重啟後零重送。平台及模型均使用回環測試端點。九例的原始逐幀記錄、版本與雜湊見 `artifacts/CORE-038/paired-consumers-pr97/consumer-receipts.json`（SHA-256 `a62089b8a5e2fc32ee69f78c11c8f513cc12572cfe47cfb142e9f7b061e57dd8`）。

分支 `codex/core-038-gateway-media-refusals`，實作 `e680fc23847c`（#98）。兩個真插件曾因媒體不支援或讀權不足令 Main 停在 held。修正後原模型能讀回拒絕，Mac 的 CORE-038 驗過 338 項斷言，亦涵蓋無權讀取版本和類型不符。見 `artifacts/CORE-038/preflight-media-refusals-isolated-01/manifest.json` 與 `artifacts/CORE-038/paired-media-{emoji-refusal,missing-reader}-after-01/`。三端已用 #98 候選完成六次重放，見 `artifacts/CORE-038/paired-media-pr98/consumer-receipts.json`（SHA-256 `bc9d9a597cb18be86363391d24d2eee63a9eae89ecb1aa3b91fb6c8289f91143`）。擴展倉 #43 亦已提交對應的媒體拒絕見證。

**CORE-026 群訊息的人物歸屬。** 分支 `codex/core-026-event-member`，提交 `681c22435609`（#100）。固定 v4 的 `SubjectContextBuilder`／`SubjectResolver` 可用 `member_<person_id>` 在同一事件提及的兩人中分別指名。Rust 在舊版重放對應輸入時曾以 `invalid_tool_arguments` 拒絕；修正後依原事件的人物及 Assignment 驗證目標，作者與第三方歸屬分別保存。同群另一條消息的提及、未提及的人與錯誤 scope 均不得落庫。原始回執在 `artifacts/CORE-026/selected-member-oracle-02/original-member-resolutions.json`，本卡實測在 `artifacts/CORE-026/selected-member-preflight-03/selected-group-member.json`。

**CORE-026 爭議記憶的合併。** 分支 `codex/core-026-contested-merge`，實作 `d101a2a12758`（#102）。固定 v4 與 Rust 均可將已讀的爭議記憶合入同一人的活躍記憶。狀態預期錯誤或寫入關聯時中斷，兩端版本與證據都保留。合併成功後，原先的矛盾關係與新合併關係皆可查，重開能讀回原回條。固定 v4 重放在 `artifacts/CORE-026/original-guarded-merge-01/`，Rust 的預驗在 `artifacts/CORE-026/guarded-merge-preflight-01/memory-contest.json`。本 PR 三端的驗收回執在 `artifacts/ci/local-pr/102/receipt.json`；CORE-039 候選及逐平台 SHA-256 由 `artifacts/CORE-039/pr102/verified-candidates.json` 交接。macOS 用前後兩個簽署包升級同一個資料卷的重放路徑為 `artifacts/CORE-026/guarded-merge-native-upgrade-01/native-upgrade.json`。

**CORE-026／040 當前群陳述。** 分支 `codex/core-026-current-group-report`。固定 v4 的原始群陳述、同鍵相同和不同內容都各自建立群事實；私聊及兩種主體錯配拒絕。隔離副本逐例證據在 `artifacts/CORE-040/original-group-create-03/original-group-create.json`。reader 55 將群事實綁到原群及發言來源；三端 CORE-026 的 `current-group-report.json` 記錄讀回、串群拒絕、重啟及來源清除。這縮小 E13 的 Memory 差異，完整同功能重放仍需下表所列的安裝。

**CORE-024 搜尋宣告與上限。** 分支 `codex/core-024-original-search-limits`，實作 `c0dd72c38311`、解碼修正 `62c45c94b8f1`（#101）。舊版普通工具取部署上限的實測在 `artifacts/CORE-024/original-result-limit-04/original-search-policy.json`。Rust 獨立搜尋已對回原 E13 的完整模型工具宣告，原 wire SHA-256 為 `fb18802b8b4e2e5187ae8e853808ac085ad862e3ca9d4764dee65fd0c76a016e`。部署值 3 傳到獨立搜尋請求，模型覆寫 20 在派發前被拒。同批成功與拒絕兩份回條在 `artifacts/CORE-024/e13-bridge-preflight-02/search-tool-policy.json`。原簽署版已接納的 Work 跨版本重開後仍可依舊合同搜尋 7 筆，實際送出的獨立請求及原回條見 `artifacts/CORE-024/e13-original-paid-upgrade-04/previous-paid-search.json`。

**CORE-024／032 給 EXT-029 的開發接口。** 分支 `codex/core-024-ext029-search-accounting`。部署可將 Tavily 工具綁定到明選的 provider、目的及 endpoint。`search.begin` 以原 handle 在同一筆交易預留根額度並登記物理 attempt，`search.finish` 登記實際回應和可得費用，`search.status` 可在重啟後讀原回條。安裝配置與故障時序見 [external-search.md](external-search.md)。Rust SDK `1.0.0-alpha.15`、Python SDK `1.0.0a15`、Host schema 2。逐平台驗收檔為 CORE-032／039 的 `external-search-ledger.json`；三端完成後，簽署候選和雜湊從 `artifacts/CORE-024/ext029-handoff/index.json` 交接。這三個方法屬開發版，EXT-029 的真插件消費和業務結果由擴展倉驗收。

**CORE-044 插件基底 1.0.0。** 分支 `codex/core-044-witnessed-base`，實作 `4860314c2e8e`。以實際 main 合併提交 `c6bd0c75e38717abc71373201d31a77982428d5b` 再驗 CORE-039／044、打包。握手 revision 1、Host schema 2、資料庫 reader 54、Rust／Python SDK 1.0.0。凍結 `initialize`、`gateway.inbound`、`gateway.plan_send`、`gateway.dispatch_part`、`tool.call` 及 `tool.status`。

Mac arm64、Linux arm64 與 HJW Windows x64 各做 14 次真插件重放，七種情境皆於冷重開後零重送。Windows 用擴展倉 #42 的原包先重跑舊簽署宿主的原版 EXT-067，再用完全相同的 EXE 驗基底。EXT-067 最早兩個本地 EXE 目前未找到；原清單及雜湊獨立保存在 `fixtures/compat/ext-067/`。

GitHub 已正式發布 `plugin-base-v1.0.0`。九個附件均已重新下載，逐檔大小與 SHA-256 相符，雜湊清單的 Ed25519 簽名通過驗證。驗簽公鑰 `signer.pub` 的 SHA-256 為 `b94f861a2d7a4a53ae689cea4990cbff6387ae9f019bba1780a1f832fadb8541`。[正式版下載](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/plugin-base-v1.0.0) · [簽署包清單](https://github.com/Institute-of-Agent-Life/yuki-core/releases/download/plugin-base-v1.0.0/base-release.json) · [逐檔雜湊](https://github.com/Institute-of-Agent-Life/yuki-core/releases/download/plugin-base-v1.0.0/SHA256SUMS)。

本地發布回執在 `artifacts/CORE-044/core044-main-c6bd0c75e387-1d0322/distribution/published-receipt.json`。42 例逐原包與原回條核對見 `artifacts/CORE-044/core044-main-c6bd0c75e387-1d0322/qualification/acceptance-index.json`（SHA-256 `33be23dca6127f85b129821fff1c14f2871fd774d7948e7517331e8ab785a604`）。三端逐檔簽署回執在 `artifacts/CORE-044/core044-main-c6bd0c75e387-1d0322/distribution/base-release.json`，SHA-256 `a98fb1e4d36f895f02db8614b88638ed73339f21d03bd42c76719ab54274dabc`。

| 交接包 | SHA-256 |
|---|---|
| macos | `ed0aef8a02d216133a21dc784c67d2c344f5bd667f168884ae5475ea1465e533` |
| linux | `737d5d0ff9ec8e77adc898f5a6112f742c7785bc395730b65a1e4b13310eb69f` |
| windows | `287167dae7b2b1222efd22fa5a0aa58e1c39f7129c7112e5b81cebb6278a7ecd` |
| 共用 SDK／schema／golden | `f9ffec757b415a745a9bec5c45897c771f22237584e7f78a4513fd0ebd28e317` |

**CORE-040 進行中。** Mac 上 A（固定 v4）／B（可復現 patch）／C（Rust）各做兩輪冷暖，14,400／14,400 個 QQ 輸入正確。六輪混合故障重放也保留退出碼與已耗資源。見 `artifacts/CORE-040/native-cold-warm-bound-01/trial-report.json` 與 `artifacts/CORE-040/failed-trial-accounting-01/mixed-report.json`。固定 E13 安裝的模型工具相對 v4 缺 9 項、4 項不同、另有 5 項；速度與成本裁決待等功能比較。#101 的 macOS 簽署候選另以實際配置搜尋重放相同四筆 QQ 輸入，四筆送達。`web_search` 的模型宣告與 v4 完全相同，剩 8 項缺少、4 項不同、5 項新增。回執在 `artifacts/CORE-040/installed-search-pr101-01/installed-search-replay.json`。

| 待辦 | 下一步與交接物 |
|---|---|
| CORE-040 → 041 → 042 → 054 | 取同一安裝的裝置／工作區、普通事件與 Work 終端、external_agent 及讀網頁方法和授權回執；另需共同部署的搜尋設定。現有 `environment_status` 只交 Manager readiness；`web_fetch` 與 v4 `read_webpage` 的宣告不同。完整差異與原模型請求見 `artifacts/CORE-040/installed-inputs-pr95-02/consumer-requirements.json`，普通輪次的固定 v4 回執在 `artifacts/CORE-040/original-terminal-source-03/ordinary-terminal-source.json`。取得可比安裝後重跑 A／B／C。 |
| CORE-045 → 053 | 核心基底 1.0.0 已可供 EXT-043 下載。擴展倉 main `16f92e1f9af3` 已交 `artifacts/EXT-043/01/extension-release-lock.json`，其中 `extension_published=false`。取得對應的正式 Telegram 包與發布回執後，執行 CORE-045 版本組合驗收。 |
| CORE-043 → 046 → 047 | 待 EXT-041 的 `extension-import-diff.json`、EXT-044 的 `desktop-package-matrix.json`、EXT-045 的 `install-restore-matrix.json` 及 EXT-042 的實物逐字稿。 |

本輪 Mac 清理回執見 `artifacts/ci/cleanup-20261106/`。先前五個閒置 incremental 目錄合計回收 17.61 GiB，Telegram 建置後另清理約 130 MiB 的中間檔案。#98 的獨立 Mac 預驗再回收 4.39 GiB，保留核心、測試執行檔和回條並重驗 SHA-256（`pr98-mac-preflight-intermediates-verified.json`）。 #99 的專用編譯中間檔另回收 Mac 8.32 GiB、HJW 1.75 GiB；執行檔與保留記錄均已重驗（`core044-mac-build-verified.json` 和 `core044-windows-owned-build-verified.json`）。 #101 完成後另回收 0.57 GiB 已被替代的本機編譯快取，保留包及回條雜湊均相符（`core101-mac-superseded-verified.json`）。
