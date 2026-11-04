# 核心實作進度

來源：任務書 v3.2 核心文檔補核版，ZIP SHA-256
`79dfd2ba5a8b96f22dea4c5f72a94ca761b15f2e354e33b36c7ecdbedc3ec3ee`。
舊行為基線：`329ca7c487414d2f54ddd98ff4727446ed2446cd`，舊倉只讀。
已依總書 K05 修正工作副本 CORE-011 第 2 步，不增加 unknown 重試權。

| 任務 | 狀態 | 分支／提交 | 驗證及產物 |
|---|---|---|---|
| CORE-001 | 完成，已進 main | `c85bf8ee182d5075c1abaa8b1a56f51e16b0a196`；分支 `codex/rust-core-v3-2` | [三平台 CI 41818720821](https://github.com/Institute-of-Agent-Life/yuki-core/actions/runs/41818720821) 全過；`artifacts/CORE-001/` 保留正常、故障與修正證據 |
| CORE-005 | 完成，已進 main | `codex/core-005-owner`，PR head `f13b29d`，合併 `d98b02e` | Windows、macOS、Linux [CI 通過](https://github.com/Institute-of-Agent-Life/yuki-core/actions/runs/41825696747)。實機 `artifacts/CORE-005/windows-05/`、`windows-sid/`、`mac-sigterm-green/` 與 `linux-container-01/` |
| CORE-002 | 完成，已進 main | `codex/core-002-oracle` | `artifacts/CORE-002/capture-mac-01/{first,second}/` 各 5 個原版情境，Windows 實機重放兩次也一致。Windows 實機完整 A 線同樣通過，見 `artifacts/CORE-002/v4-oracle-979bac82/load/`。提交 `89f15db`，[PR #2](https://github.com/Institute-of-Agent-Life/yuki-core/pull/2) 三平台 CI 通過並合併 `2ea4477`。A 線 `load-mac-01/` 為 1,200 條逐筆送達與 20 個原 Work。三平台入口均為 `python ci/check.py CORE-002 --out <新目錄>` |
| CORE-003 | 完成，已進 main | `codex/core-003-inventory` | `artifacts/CORE-003/` 的 `mac-02/`、`windows-01/` 均通過，提交 `9c77e56`、[PR #3](https://github.com/Institute-of-Agent-Life/yuki-core/pull/3) 三平台全過並合併 `83d8583`，另留 red、WAL、缺 blob、拒絕路徑逸出證據 |
| CORE-006 | Mac 和 Windows 實機通過，待三平台 CI | `codex/core-006-person` | `artifacts/CORE-006/mac-02/`、`windows-01/` 中的 `person-before-after.json` 保留舊庫升級、原人物與停用兩端的授權結果。 |

**擴展交接。** EXT-007、EXT-019 可讀 CORE-002 執行產物
`v4-oracle-candidate.zip`；本地路徑 `artifacts/CORE-002/handoff-89f15db/v4-oracle-candidate.zip`，
SHA-256 `ccdefae10242b19d2f1a201fcd59f5567c08395fd0c051c3d4e92282f6434df0`。
同目錄 `v4-oracle-index.json` 記錄來源提交。
內容含 Client9、Admin OpenAPI、Connector 0.4.3 的 wire4 錄製、舊 Main 和 Work 行為及 A 線。
此包未發布；Rust 宿主／插件 SDK 與穩定協議仍待後續卡及真實消費者。

CORE-006 合併後，CORE-007 可使用此身份模型與 CORE-003 盤點。
雙 Bot 的部署映像 digest 和去識別雙端封包尚缺，CORE-004 須按 K02 的保守分支裁決。
