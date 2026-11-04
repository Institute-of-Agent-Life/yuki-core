# 核心實作進度

來源：任務書 v3.2 核心文檔補核版，ZIP SHA-256
`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。
舊行為基線：`329ca7c487414d2f54ddd98ff4727446ed2446cd`，舊倉只讀。
已依總書 K05 修正工作副本 CORE-011 第 2 步，不增加 unknown 重試權。

| 任務 | 狀態 | 分支／提交 | 驗證及產物 |
|---|---|---|---|
| CORE-001 | 完成，已進 main | `c85bf8ee182d5075c1abaa8b1a56f51e16b0a196`；分支 `codex/rust-core-v3-2` | [三平台 CI 41818720821](https://github.com/Institute-of-Agent-Life/yuki-core/actions/runs/41818720821) 全過；`artifacts/CORE-001/` 保留正常、故障與修正證據 |
| CORE-005 | 完成，已進 main | `codex/core-005-owner`，PR head `f13b29d`，合併 `d98b02e` | Windows、macOS、Linux [CI 通過](https://github.com/Institute-of-Agent-Life/yuki-core/actions/runs/41825696747)。實機 `artifacts/CORE-005/windows-05/`、`windows-sid/`、`mac-sigterm-green/` 與 `linux-container-01/` |
| CORE-002 | 固定基線與負載已驗，正在交付 | `codex/core-002-oracle` | `artifacts/CORE-002/capture-mac-01/{first,second}/` 各 5 個原版情境；A 線 `load-mac-01/` 為 1,200 條逐筆送達與 20 個原 Work。三平台入口均為 `python ci/check.py CORE-002 --out <新目錄>` |

**擴展交接。** EXT-007、EXT-019 可讀 CORE-002 執行產物
`v4-oracle-candidate.zip`；同目錄 `v4-oracle-index.json` 記錄內容雜湊與來源提交。
內容含 Client9、Admin OpenAPI、Connector 0.4.3 的 wire4 錄製、舊 Main 和 Work 行為及 A 線。
此包未發布；Rust 宿主／插件 SDK 與穩定協議仍待後續卡及真實消費者。

CORE-002 交付後可做 CORE-003 的唯讀盤點；CORE-006 已具備另一先決 CORE-005。
雙 Bot 的部署映像 digest 和去識別雙端封包尚缺，CORE-004 須按 K02 的保守分支裁決。
