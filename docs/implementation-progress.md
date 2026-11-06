# 核心實作進度

截至 2026-11-06，工作目錄為 `yuki-core`，實作整合分支為 `codex/core-taskbook-local-handoff`，交接記錄分支為 `codex/core-validated-handoff`。任務書 v3.2 使用補正後的 CORE-011 工作副本，原版行為固定在 `329ca7c487414d2f54ddd98ff4727446ed2446cd`。

| 已完成的本地驗證或整合 | 提交與原始回執 |
|---|---|
| #43 已合併 main，Mac arm64、Linux arm64、HJW Windows x64 各驗 39 卡 | main `82c14715e1bf`，`artifacts/ci/local-pr/43/receipt.json`，SHA-256 `8555243c6019263958d4f8a472ae46d72972315f133b9b038f815dcd426244c6` |
| #85 已合併 main。三端各驗 49 張已註冊卡，HJW 的終端與個人 SDK 檔案鎖故障均有獨立重放 | head `2385907a1b9a`，受測合併樹 `02a8bcf9d7c8`，main `9c17975ed11b`；`artifacts/ci/local-pr/85/receipt.json`，SHA-256 `85ed285396114ad52141fced3842096a66f411f5275ce9329cc0a2ee87de9b1f` |
| #92 已合併 main，三端各驗 49 卡；含 Windows SSH 會話的驗簽修正 | head `808318b5de08`，受測合併樹 `eb0a7c49f3f3`，main `fb8806c5f718`，`artifacts/ci/local-pr/92/receipt.json`，SHA-256 `3396de1f14f9face9e576e6498d8fb099c0b525c7b2e7f76a9727be7337e1da7` |
| CORE-026 已修正帶既有 fact_id 的 create，Mac 1,398 項；CORE-013 已修正六段文字回條、撤回第三段後停止下一次派發 | #91 `8555755a439b`，`artifacts/CORE-026/create-identity-after-01/`；#87 `d344d7d51ff5`，`artifacts/CORE-013/automatic-split-check-01/` |
| CORE-040 已重放二十個雙裝置長 Work、兩種公開檢查點結果與拒絕後的四次模型請求。冷暖工具已綁定 CORE-039 執行檔 | #86–#90，`463acb44b754`、`5261421aeea5`。見 `artifacts/CORE-040/{long-work-matrix-C-02,private-fallback-check-01,native-trials-smoke-02}/` 與 `docs/benchmarks/slice.md` |

41 個已整合的堆疊 PR 已逐一重核，所有 head 均是 main `4c9aa971183f` 的祖先，重驗三端共 147 份清單後已關閉歸檔。核對記錄在 `artifacts/ci/pr-reconciliation-20261106/closed-41-confirmed.json`。

**任務範圍。** CORE-001–039、048–052、055–058 各有可執行驗收與具名產物。CORE-040 已註冊，目前仍是部分證據。逐卡分支、原始故障、來源與交接版本見[固定提交的逐卡記錄](https://github.com/Institute-of-Agent-Life/yuki-core/blob/8555755a439b0e67bdda6f00673eaed2ab7502a0/docs/implementation-progress.md)。原始證據路徑均相對於工作目錄。

**可交接候選。** #85 的檔案與說明文字候選來自 `04139ad30742`，資料庫 51、Rust SDK 1.0.0-alpha.14、Python SDK 1.0.0a14。三個未發布 ZIP 的絕對路徑、校驗和與逐卡清單在 `artifacts/CORE-013/file-caption-04139ad30742/verified-candidates.json`，索引 SHA-256 `561bdbfdf1bdc0bc1084803021496438b40a569123812002e14fc3de5ae6b708`。可下載的[握手預發布](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/handshake-preview-dd694a0db605)只提供 initialize。

#85 受測合併樹另有三份 CORE-039 原生核心與 SDK 分發包。各端用獨立臨時金鑰驗簽，驗證原執行檔、能力清單、SDK 與依賴。索引在 `artifacts/CORE-039/pr85-02a8bcf9d7c8/verified-candidates.json`，SHA-256 `7e7666cc2674e05b6e8bcc6e1c200fd649d47bd1ca9a9a81becdabc7db2ffb64`。

**最新三端候選。** 擴展倉可從 `artifacts/CORE-039/pr92-eb0a7c49f3f3/verified-candidates.json` 取得三個未發布 ZIP 的絕對路徑、逐包雜湊及實含方法。索引 SHA-256 `095b946d540bc197a9ccafbf5bd957c2fbc14384e79d05ab0fead1ee6e84a25b`。來源為 #92 的受測合併樹，資料庫 reader 54、握手 schema 1、Host schema 2，Rust／Python SDK 分別為 1.0.0-alpha.14／1.0.0a14。歷次 31 組、93 個候選包的逐檔校驗總表在 `artifacts/ci/core-handoffs-20261106/catalog-after-pr92-packaging.json`，SHA-256 `2cb9873c76b4930ae8bd950cf7b4f62a34f6510a5f378db8eaf4affcd1f0d6a5`。

CORE-039 的獨立驗簽器在 HJW 的 Windows SSH 會話曾停住。已重現 OpenSSH 繼承的舊檔案描述符狀態與新 stdin pipe 衝突，並以原簽名和變造輸入驗證修正。回執在 `artifacts/CORE-039/pr85-02a8bcf9d7c8/{openssh-native-environment,signature-regression-windows}.json`。

**執行中的 CORE-040。** 固定舊版 A、套用可重現 patch 的 B，以及 Rust 核心 C 在 Mac 上各跑兩輪冷、暖 QQ 離線重放。12 個獨立程序共驗證 14,400／14,400 個事件，失敗輪次為零。報告綁定 C 的執行檔與來源 `b74a91582f22`，見 `artifacts/CORE-040/native-cold-warm-bound-01/trial-report.json`，SHA-256 `f8ef9111edf8b00849ee9fdfdf3ab74affecc20bf29cf3c3d2b44f3a7ad21385`。SQLite 寫入設定一致，模型可見的工具仍有差異，速度及成本裁決待等功能驗收。

CORE-026／040 的 `codex/core-040-memory-create-contract` 以固定 v4 實際寫入與拒絕回執核對七種記憶建立參數。Rust 拒絕缺少目標、內容、鍵或類別的請求；人物目標收到合法 SELF visibility 提示時仍只寫入原人物範圍。原版投影 SHA-256 `52f4d2657d902f34a7d03d2a92dbcfd817e926b43fc844146357350bfa65921f`，實跑對照在 `artifacts/CORE-026/{create-arguments-v4-01,create-arguments-before-01,create-arguments-after-02}/`。

EXT-020 alpha.1 在 Linux 可交付原 Manager 的 readiness，Mac 原 Manager 拒絕同一請求。兩端均有 Manager 離線時的回條。CORE-040 下一輪需要的已安裝方法、原授權範圍、版本與消費證據列在 `artifacts/CORE-040/installed-inputs-01/consumer-requirements.json`，SHA-256 `6b897f10dc111907a67bd6952ef6fbad74defe156ae525c3d4fbd37dde6f99d0`。

| 待辦卡 | 下一步需要的輸入 |
|---|---|
| CORE-040 → 041 → 042 → 054 | 與 v4 可比的已安裝裝置／工作區檔案、終端、external_agent、網頁工具與消費回條，並對齊 core 的模型宣告。040 裁決後，041 才開始實際導入。 |
| CORE-044 → 045 → 053 | EXT-067 的 `base-consumer-witness.json`；EXT-043 的 `extension-release-lock.json`。交接需有原包版本、SHA-256 與方法回條。 |
| CORE-043 → 046 → 047 | EXT-041 的 `extension-import-diff.json`，以及 EXT-044 的 `desktop-package-matrix.json`、EXT-045 的 `install-restore-matrix.json` 與 EXT-042 的真實消費逐字稿。 |

Mac 兩次共回收五個閒置的 Rust incremental 目錄，合計 17.61 GiB。回收路徑及保留產物的雜湊記錄在 `artifacts/ci/cleanup-20261106/{three-pr-mac-incremental-verified,two-finished-debug-incremental-verified}.json`。
