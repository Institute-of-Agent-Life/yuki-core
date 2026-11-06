# 核心實作進度

2026-11-06。固定 v4 行為基線 `329ca7c487414d2f54ddd98ff4727446ed2446cd`；任務書工作副本的 CORE-011 已採用 K05 的邏輯請求預留規則。下列證據路徑相對於 yuki-core 工作目錄。

**48 張卡已有驗收。** CORE-001–039、048–052、055–058。CORE-040 有可執行的部分驗收，另 9 張的前置輸入見下表。[逐卡分支與原產物](https://github.com/Institute-of-Agent-Life/yuki-core/blob/8555755a439b0e67bdda6f00673eaed2ab7502a0/docs/implementation-progress.md)，[先前候選、回放與清理明細](https://github.com/Institute-of-Agent-Life/yuki-core/blob/e680fc23847c8460639294eef7448d27d465b79c/docs/implementation-progress.md)。

| 已合併 PR | head | main 合併提交 | 本地 Mac／Linux／HJW Windows |
|---|---|---|---|
| #43 | `19d29b959cb2` | `82c14715e1bf` | 各 39 卡 |
| #85 | `2385907a1b9a` | `9c17975ed11b` | 各 49 卡 |
| #92 | `808318b5de08` | `fb8806c5f718` | 各 49 卡 |
| #95 | `92dcccde08a6` | `f27749ef4b4f` | 各 49 卡 |
| #96 | `671b5274e7e9` | `7809b26ec3a1` | 各 49 卡 |
| #97 | `2f923d99c6c4` | `9c1c2eba971c` | 各 49 卡 |
| #98 | `17d5015008af` | `2f8785004584` | 各 49 卡 |

各 PR 的受測合併提交、三端逐卡雜湊在 `artifacts/ci/local-pr/<PR>/receipt.json`。#98 的回執 SHA-256 為 `d20376ba58605827a1387c40b0f6d222c4829f9a90eb6694f7651abb15e9a1ce`。41 個已整合的堆疊 PR 均逐一核對 head 已進入 main，並核驗 147 份清單；見 `artifacts/ci/pr-reconciliation-20261106/closed-41-confirmed.json`。

**三端開發候選。** #98 原生 Host 來源 `dce1a6862000c312dc93dcfd7e9b3b2f8ba06168`，實際合併樹相同。資料庫 reader 54、握手 schema 1、Host schema 2、Rust SDK 1.0.0-alpha.14、Python SDK 1.0.0a14。共 14 個實作方法，完整包路徑與驗簽結果見 `artifacts/CORE-039/pr98/verified-candidates.json`（SHA-256 `d7aa2e2a7e584bf8f10ea499bdbff6cc8c6f70fdb26e3ad438288930593c1c3c`）。

| 平台 | 本地未發布 ZIP 的 SHA-256 |
|---|---|
| darwin / arm64 | `6993a481ab6bdac290c6c5789f14c4e9b813ed76483a52e1ed7b7c92ef57d7a8` |
| linux / arm64 | `0d73ed3abaaaf17949f0b88e736ecaf1aa7bb90ada09e10a24834534abeb45ae` |
| windows / x86_64 | `6f25d5274c118bf26dbf101062ed1edc333d0ac0615f92e0c0b719e9c413968b` |

公開的[握手預發布](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/handshake-preview-dd694a0db605)提供 `initialize`。

**CORE-038 真插件。** 三端均以同一平台的原生 Host 啟動 Telegram 與 web-fetch。Telegram alpha.2 收件後缺少發送權，Main 取得失敗回執，原生發送為零。alpha.3 把網頁讀回內容納入 8,197 字元回覆，三段均獲原生定位。第二段已送出但遺失回應時保留 unknown，第三段未派，重啟後零重送。平台及模型均使用回環測試端點。九例的原始逐幀記錄、版本與雜湊見 `artifacts/CORE-038/paired-consumers-pr97/consumer-receipts.json`（SHA-256 `a62089b8a5e2fc32ee69f78c11c8f513cc12572cfe47cfb142e9f7b061e57dd8`）。

分支 `codex/core-038-gateway-media-refusals`，實作 `e680fc23847c`（#98）。兩個真插件曾因媒體不支援或讀權不足令 Main 停在 held。修正後原模型能讀回拒絕，Mac 的 CORE-038 驗過 338 項斷言，亦涵蓋無權讀取版本和類型不符。見 `artifacts/CORE-038/preflight-media-refusals-isolated-01/manifest.json` 與 `artifacts/CORE-038/paired-media-{emoji-refusal,missing-reader}-after-01/`。三端已用 #98 候選完成六次重放，見 `artifacts/CORE-038/paired-media-pr98/consumer-receipts.json`（SHA-256 `bc9d9a597cb18be86363391d24d2eee63a9eae89ecb1aa3b91fb6c8289f91143`）。擴展倉 #43 亦已提交對應的媒體拒絕見證。

**CORE-026 群訊息的人物歸屬。** 分支 `codex/core-026-event-member`。固定 v4 的 `SubjectContextBuilder`／`SubjectResolver` 可用 `member_<person_id>` 在同一事件提及的兩人中分別指名。Rust 在舊版重放對應輸入時曾以 `invalid_tool_arguments` 拒絕；修正後依原事件的人物及 Assignment 驗證目標，作者與第三方歸屬分別保存。同群另一條消息的提及、未提及的人與錯誤 scope 均不得落庫。原始回執在 `artifacts/CORE-026/selected-member-oracle-02/original-member-resolutions.json`，本卡實測在 `artifacts/CORE-026/selected-member-preflight-03/selected-group-member.json`。

**CORE-024 搜尋宣告與上限。** 分支 `codex/core-024-original-search-limits`。舊版普通工具取部署上限的實測在 `artifacts/CORE-024/original-result-limit-04/original-search-policy.json`。Rust 獨立搜尋已對回原 E13 的完整模型工具宣告，原 wire SHA-256 為 `fb18802b8b4e2e5187ae8e853808ac085ad862e3ca9d4764dee65fd0c76a016e`。部署值 3 傳到獨立搜尋請求，模型覆寫 20 在派發前被拒。同批成功與拒絕兩份回條在 `artifacts/CORE-024/e13-bridge-preflight-01/search-tool-policy.json`。

**CORE-040 進行中。** Mac 上 A（固定 v4）／B（可復現 patch）／C（Rust）各做兩輪冷暖，14,400／14,400 個 QQ 輸入正確。六輪混合故障重放也保留退出碼與已耗資源。見 `artifacts/CORE-040/native-cold-warm-bound-01/trial-report.json` 與 `artifacts/CORE-040/failed-trial-accounting-01/mixed-report.json`。固定 E13 安裝的模型工具相對 v4 缺 9 項、4 項不同、另有 5 項；速度與成本裁決待等功能比較。

| 待辦 | 下一步與交接物 |
|---|---|
| CORE-040 → 041 → 042 → 054 | 取同一安裝的裝置／工作區、普通事件與 Work 終端、external_agent 及搜尋／網頁方法和授權回執。現有 `environment_status` 只交 Manager readiness；`web_fetch` 與 v4 `read_webpage` 的宣告不同。完整差異與原模型請求見 `artifacts/CORE-040/installed-inputs-pr95-02/consumer-requirements.json`，普通輪次的固定 v4 回執在 `artifacts/CORE-040/original-terminal-source-03/ordinary-terminal-source.json`。取得可比安裝後重跑 A／B／C。 |
| CORE-044 → 045 → 053 | EXT-067 與 EXT-028 已取得（擴展倉 `bc58ec7e703e`）。CORE-044 分支 `codex/core-044-witnessed-base` 匯出六種已見證 RPC、兩個 1.0.0 SDK。Mac、HJW 各用兩組真插件重放七例，見 `artifacts/CORE-044/base-real-{macos,windows}-03/`。HJW 的歷史本地 EXE 尚未找回；另取擴展倉 #42 的 GitHub 原包，在原簽署宿主同跑未修改的 EXT-067 七例（`artifacts/CORE-044/pinned-gha-consumers-windows-01/base-consumer-witness.json`），再以同一對 EXE 驗新宿主。Mac release 預驗通過，回執 `artifacts/CORE-044/preflight-release-01/manifest.json`。最終原生包及簽名將由受測來源和三端實物重放產生；公開發布另需授權。EXT-043 的 `extension-release-lock.json` 仍是 045／053 所需交接物。 |
| CORE-043 → 046 → 047 | 待 EXT-041 的 `extension-import-diff.json`、EXT-044 的 `desktop-package-matrix.json`、EXT-045 的 `install-restore-matrix.json` 及 EXT-042 的實物逐字稿。 |

本輪 Mac 清理回執見 `artifacts/ci/cleanup-20261106/`。先前五個閒置 incremental 目錄合計回收 17.61 GiB，Telegram 建置後另清理約 130 MiB 的中間檔案。#98 的獨立 Mac 預驗再回收 4.39 GiB，保留核心、測試執行檔和回條並重驗 SHA-256（`pr98-mac-preflight-intermediates-verified.json`）。
