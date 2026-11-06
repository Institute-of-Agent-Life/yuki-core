# 核心實作進度

截至 2026-11-06，工作目錄為 `yuki-core`，本次整合分支為 `codex/core-taskbook-local-handoff`。任務書 v3.2 使用補正後的 CORE-011 工作副本，原版行為固定在 `329ca7c487414d2f54ddd98ff4727446ed2446cd`。

| 已完成的本地驗證或整合 | 提交與原始回執 |
|---|---|
| #43 已合併 main，Mac arm64、Linux arm64、HJW Windows x64 各驗 39 卡 | main `82c14715e1bf`，`artifacts/ci/local-pr/43/receipt.json`，SHA-256 `8555243c6019263958d4f8a472ae46d72972315f133b9b038f815dcd426244c6` |
| #85 已合併 main。三端各驗 49 張已註冊卡，HJW 的終端與個人 SDK 檔案鎖故障均有獨立重放 | head `2385907a1b9a`，受測合併樹 `02a8bcf9d7c8`，main `9c17975ed11b`；`artifacts/ci/local-pr/85/receipt.json`，SHA-256 `85ed285396114ad52141fced3842096a66f411f5275ce9329cc0a2ee87de9b1f` |
| CORE-026 已修正帶既有 fact_id 的 create，Mac 1,398 項；CORE-013 已修正六段文字回條、撤回第三段後停止下一次派發 | #91 `8555755a439b`，`artifacts/CORE-026/create-identity-after-01/`；#87 `d344d7d51ff5`，`artifacts/CORE-013/automatic-split-check-01/` |
| CORE-040 已重放二十個雙裝置長 Work、兩種公開檢查點結果與拒絕後的四次模型請求。冷暖工具已綁定 CORE-039 執行檔 | #86–#90，`463acb44b754`、`5261421aeea5`。見 `artifacts/CORE-040/{long-work-matrix-C-02,private-fallback-check-01,native-trials-smoke-02}/` 與 `docs/benchmarks/slice.md` |

**任務範圍。** CORE-001–039、048–052、055–058 各有可執行驗收與具名產物。CORE-040 已註冊，目前仍是部分證據。逐卡分支、原始故障、來源與交接版本見[固定提交的逐卡記錄](https://github.com/Institute-of-Agent-Life/yuki-core/blob/8555755a439b0e67bdda6f00673eaed2ab7502a0/docs/implementation-progress.md)。原始證據路徑均相對於工作目錄。

**可交接候選。** #85 的檔案與說明文字候選來自 `04139ad30742`，資料庫 51、Rust SDK 1.0.0-alpha.14、Python SDK 1.0.0a14。三個未發布 ZIP 的絕對路徑、校驗和與逐卡清單在 `artifacts/CORE-013/file-caption-04139ad30742/verified-candidates.json`，索引 SHA-256 `561bdbfdf1bdc0bc1084803021496438b40a569123812002e14fc3de5ae6b708`。28 組共 84 個包的另一次逐檔校驗在 `artifacts/ci/core-handoffs-20261106/catalog-before-pr85.json`，SHA-256 `44512b7cda88674d79f13973de1c2ecd961281d08c4130b17aa74a6b524f8cfe`。可下載的[握手預發布](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/handshake-preview-dd694a0db605)只提供 initialize。

#85 受測合併樹另有三份 CORE-039 原生核心與 SDK 分發包。各端用獨立臨時金鑰驗簽，驗證原執行檔、能力清單、SDK 與依賴。索引在 `artifacts/CORE-039/pr85-02a8bcf9d7c8/verified-candidates.json`，SHA-256 `7e7666cc2674e05b6e8bcc6e1c200fd649d47bd1ca9a9a81becdabc7db2ffb64`。

**執行中的 CORE-040。** A/B 使用固定舊版和可重現的 B patch，C 走原生 QQ。來源已綁定的一輪冷暖測量共 24／24 個正確事件；舊十二輪重核共 14,400／14,400 個正確事件。完整來源綁定重放另存 `artifacts/CORE-040/native-cold-warm-bound-01/`。已安裝的真實 EXT-020 alpha.1 在 Linux 可回報原 Manager readiness，Mac 原 Manager 拒絕該請求。缺少 Manager 時兩端均續跑。原版的裝置授權及 Shell 查詢尚缺對應宣告。效果、工具集、成本分開記錄；完整等功能與 Rust 投資判斷仍開放。

| 待辦卡 | 下一步需要的輸入 |
|---|---|
| CORE-040 → 041 → 042 → 054 | 與 v4 可比的已安裝裝置／工作區檔案、終端、external_agent、網頁工具與消費回條，並對齊 core 的模型宣告。040 裁決後，041 才開始實際導入。 |
| CORE-044 → 045 → 053 | EXT-067 的 `base-consumer-witness.json`；EXT-043 的 `extension-release-lock.json`。交接需有原包版本、SHA-256 與方法回條。 |
| CORE-043 → 046 → 047 | EXT-041 的 `extension-import-diff.json`，以及 EXT-044 的 `desktop-package-matrix.json`、EXT-045 的 `install-restore-matrix.json` 與 EXT-042 的真實消費逐字稿。 |

Mac 最近一次回收三個已閒置的 incremental 目錄，核實釋出 11.03 GiB。路徑與保留產物的校驗在 `artifacts/ci/cleanup-20261106/three-pr-mac-incremental-verified.json`。
