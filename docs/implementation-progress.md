**CORE-038 逐段轉寫保留。** 分支 `codex/core-038-transcript-segments`。#122 原包重現同一訊息兩段語音相互覆蓋，回條 `artifacts/CORE-038/transcript-segments-red-01/case-result.json`。Host 依原 event／segment 保存逐段投影，原 owner 的整份修訂入口繼續沿用。重讀原操作保留回執，重複相同文字維持搜尋版本。Mac 預驗 `artifacts/CORE-038/transcript-segments-green-03/manifest.json` 通過 420 項斷言，SHA-256 `0880990f276e131e7d8befcd5d8e968054b6d3f679cea99a98a70a41d125999b`。待三端本地矩陣後交 EXT-030。

**CORE-038 Main 所選附件的擦除已合併。** 分支 `codex/core-038-selected-forget`，[#122](https://github.com/Institute-of-Agent-Life/yuki-core/pull/122) head `d56898e065f8`，main `43657d6df0cb`。資料 reader 58 允許清除可讀附件 handle 並保留原效果的事件與位置。Mac、Linux、HJW Windows 各通過 52 張卡與 200 項原生檢查，合併回條 `artifacts/ci/local-pr/122/main-confirmation.json`，SHA-256 `90cd1f72b1f8418ac32ff7313ac572f88a0da3ab64425b83a2defb715a8e7aa3`。三端未發布候選索引 `artifacts/CORE-038/pr122-selected-erasure-01/verified-candidates.json`，SHA-256 `e8925472c9af0804d4aaa040289bb44218657f349372125ab8f9122859b0a182`。擴展 #71 已用三端原包驗 PDF、影片影格與清除後遲到結果。Linux 回收 4,446,728,192 bytes，142,011 個保留檔案校驗一致，見 `artifacts/ci/cleanup-20261113/linux-pr122-01/receipt.json`。

**CORE-040 同安裝讀網頁。** #120 Mac 簽署 Host `fca3f22ce60d` 與擴展 #66 的 Rust web-fetch alpha.3 使用原 `read_webpage` 選裝宣告，配同一安裝的模型原生 `web_search`。四個固定 v4 QQ 輸入都送達，兩項宣告及 Chat 設定與 v4 相同。回條 `artifacts/CORE-040/installed-reader-pr120-01/installed-reader.json` 的 SHA-256 為 `b3e1c8107902ac91017bcd04c290e50f8e9ef7d04e64b38abd4b065c506e8fe7`。仍缺設備、external_agent、附件存入工作區與三項終端宣告，另有四項已公布工具的宣告差異。E13 全套耗時裁決待補齊同功能範圍。

**CORE-027 自動提取的回覆來源已合併。** [#121](https://github.com/Institute-of-Agent-Life/yuki-core/pull/121) head `cb20ef1b5b40`，main `ab1a5cb6064c`。 分支 `codex/core-027-source-erasure`。準備請求時綁定同入口、同接收階段的原回覆來源，涵蓋已移出上下文頁的原訊息。來源擦除使批次失效並清除私料，晚到回應只結算原 attempt 的用量。reader 57 補綁舊提取批次的可證來源。Mac 原卡通過 251 項斷言，回條 `artifacts/CORE-027/original-reply-source-04/manifest.json`（SHA-256 `adf9864abe1b340d9a0358ecc8c74c224ca4eca8f675f420a568819fcc6afb68`）。以 #120 的簽署 reader 56 原包實測先擦除後升級及先升級後擦除，回條 `artifacts/CORE-027/original-reply-upgrade-01/upgrade.json`（SHA-256 `81a290cfd0ee2b56f896b6eaf4ac7bd53faabf92104091501c34b68536941741`）。Mac、Linux、HJW Windows 各通過 52 張卡和 200 項原生檢查。三端回條 `artifacts/ci/local-pr/121/receipt.json`，SHA-256 `94a2e8fe3e538cb9744d110c74c1dfb97adf8a645ed7a452d19850bd74915864`。未發布候選見 `artifacts/CORE-039/pr121-extraction-reply/verified-candidates.json`。

**CORE-026 原回覆作者已合併。** [#120](https://github.com/Institute-of-Agent-Life/yuki-core/pull/120) head `6876dbfc043b`，main `a1e3609c88b0`。Main 的明確記憶操作以同入口、同接收帳號的先前原事件定位被回覆者。Mac、Linux、HJW Windows 各通過 52 張卡與 200 項原生檢查。三端回條 `artifacts/ci/local-pr/120/receipt.json`，SHA-256 `8511722ba5f3aa478af717d147c812de36d863d9b47aee56e728e61876dd961c`。來源 `fca3f22ce60d` 的三端未發布開發候選、SDK alpha.21 與完整 SHA-256 在 `artifacts/CORE-039/pr120-original-reply/verified-candidates.json`。

**CORE-053 1.1.0 已正式發布。** #118、#119 已分別合併 `2a25234e0a44`、`09bdd7382de4`。#119 在 Mac、Linux、HJW Windows 各通過 52 張卡與 200 項選面重放，回條 `artifacts/ci/local-pr/119/receipt.json`，SHA-256 `3e204aca8eb12cd81e3d30f72a0b2a2e5942779656ebb3f2f764d48f8889ea75`。以實際 main `09bdd7382de41343767960c0aa0e279db2b618dc` 重建三端原生 Host 和 Rust／Python SDK 1.1.0，資料 reader 56。飛書與 Lark、Telegram、web-fetch 在三端完成正常、拒權及冷啟重放。新增七個方法與原事件 `topic_root` 欄位。主 ZIP SHA-256 `90933f039d2aaa19231f03b7ea12dabd0ebfb09cac282d05e8385a8d33738845`。七個附件已對簽署清單，公開 tag 已核對到原 commit。發布回條 `artifacts/CORE-053/native-gateway-release-1.1-main-09bdd7382de4/github-assets-01/github-publication.json` 的 SHA-256 為 `f7255fddaa9e37101b799b271590b8867e7131f6972ffa0581d1aabebf0979d4`。Windows 首輪缺兩份鎖定 wheel，補齊後完整重跑。[plugin-base-v1.1.0](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/plugin-base-v1.1.0)

核心正式 51／58。CORE-040 的 QQ 同安裝比較仍需原 Work／終端與本機設備消費證據。擴展 #66 已交 Rust `read_webpage` 的三端宣告和原結果，可納入下一輪同安裝對拍。#120 後的 Linux 閒置 target 回收 4,451,426,304 bytes，136,095 個保留檔案雜湊一致，回條在 `artifacts/ci/cleanup-20261113/linux-pr120-01/receipt.json`。

CORE-053 清除結果回寫的正常與故障時序見 [清除回執](selected-forget-sequences.md)。已選 `forget.request` 依原請求 key、watermark 和當前程序授權保存清除 proof，冷啟補交未完成的義務。

**#118 補修通過預驗。** `codex/core-025-mention-paging` 在接納時整理相異的原身份。每頁重新核對當前授權，較早的呼叫從原註記推導身份。測試 peer 接上原命令領取機制，檔案解鎖後只領取一次。Mac 和 HJW Windows 各通過 13 個情境、172 個斷言。HJW 另以飛書、Lark、飛書三次重放原 11,000 個提及，逐次核對 323 頁及原回執。

預驗索引 `artifacts/CORE-032/mention-paging-authority-preflight/index.json` 的 SHA-256 為 `b2b69d9d268ed667396f5454cfee02c31b5fbef0d4f0221e7b63f4b924bc391c`。首輪 Windows 鎖檔失敗及修正證據均已保留。最新三端完整 CI 通過後再交付候選。正式完成數仍為 51／58。

**CORE-025／CORE-032 密集提及分頁修正已通過預驗。** 分支 `codex/core-025-mention-paging`，基於 main `77cec16d84817`。Windows 原飛書失敗案例的 11,000 個提及與 323 頁註記，已以原 alpha.8 插件在飛書／Lark／飛書三次重放通過。Mac debug／release 的 13 情境通過，Windows 核心 13 情境與 169 個斷言通過。Mac 另補摘要注入拒絕，debug 共 170 個斷言。格式、Clippy 與原 JSON bytes 回歸通過。

每頁重新驗證原授權與來源，複用接納時已計算的原摘要。12 秒期限、reader 56、SDK alpha.21 沿原合同。具名預驗 `artifacts/CORE-032/mention-paging-evidence/index.json`，SHA-256 `a731c6a0b82e7dbed798116f0bdeb2a3d3e8c906c84bfef43a7d603a9bad5963`。下一步推送本補修，執行最新合併樹的完整本地三端 CI，通過後合併及交付飛書候選。穩定公開合同與 EXT-006 完整驗收仍待後續，核心正式總數維持 51／58。

**#117 已合併。** main `77cec16d84817864567e687a208b13ffa600b5bd`，三端各 52 張與 42 項原生驗證通過。完整回條在本倉根目錄 `artifacts/ci/local-pr/117/receipt.json`，未發布三端候選在 `artifacts/CORE-039/pr117-native-mentions/verified-candidates.json`。

**#117 三端重驗中。** 首輪 Mac、Linux 各 52 張與 42 項原生驗證通過。Windows 的測試 mailbox 遇到暫時性 EACCES，已改用既有 `take_command`，三次隔離附件重放各 152 個斷言及真檔案鎖回歸通過。失敗與修正證據見 `artifacts/ci/native-mentions-windows-failure/`。本輪補丁仍須完成最新提交的三端完整 CI。

**CORE-025／CORE-038 原帳號提及準備完成。** 分支 `codex/core-025-native-mentions`，Rust／Python SDK alpha.21／a21，reader 56。同 app／namespace 的原 account、原事件名稱與 UTF-8 範圍交給 Host；`gateway.read_mentions` 以原 call handle 分頁。計畫完整覆蓋原文字且保留整個提及，派發前重核原階段與來源，來源持有同時登記 execution dependency，遺忘後清除 Host 材料。

Mac 正常 `CORE-032` 入口已通過 837 項斷言，回條 `artifacts/CORE-032/native-mentions-preflight-02/manifest.json`，SHA-256 `58e82e6e5eea44db16554c88566b41e76c456c248b751451af61b7794e2f32fe`。其後補私聊僅解析原參與者，連同同名、明確 account、未知文字、大段分頁、unknown、錯區間、偽 native、換階段與遺忘共 12 情境已在 `artifacts/CORE-032/native-mentions-focused-06/` 重驗。固定舊版文字 oracle 位於 `fixtures/social/v4-native-mentions.json`。較早的群來源篩選與測試命令／參數故障均保留原失敗回條。

下一步跑此提交的本地 Mac、Linux、HJW 完整矩陣與原生分發檢查，通過後普通合併並交三端未發布候選給飛書消費者。正式總數維持核心 51／58，擴展真消費及穩定合同發布仍待後續驗收。

**CORE-038 原檔名補齊進行中。** 分支 `codex/core-038-original-filename`。飛書附件上傳會消費原版本的檔名，Main 未指定名稱時由核心依原讀取權補齊，明確改名保留。Mac 的 CORE-032 預驗通過 693 項斷言，涵蓋缺欄位、原 payload 保留及原附件故障時序。回條 `artifacts/CORE-038/original-filename-02/manifest.json`，SHA-256 `0627653aa4fb7266513c47fd4f39d4fb0149d01a7e4a85a79030876b9df52fad`。下一步執行本地三端全量 CI 及候選包驗證。

**CORE-038 長文字已合併。** #115 head `3fa3e30116fe`，main `f9e027fcbb8e`。受測來源 `942a534763ba`，三端各 52 張檢查及 42 項原生協議斷言通過。完整回條位於本倉根目錄 `artifacts/ci/local-pr/115/receipt.json`，SHA-256 `21ef7352c3656945b32bd222aba1ca8e2f5ab72b014207ca06027efb4c0ba9f3`。未發布的 SDK alpha.20／reader 56 三端候選位於 `artifacts/CORE-039/pr115-bounded-text/verified-candidates.json`。

**空間清理已核對。** Mac 內置盤回收閒置 Docker build layers 和四輪合成 benchmark SQLite，Windows 回收 384 個已核同雜湊的重複 ZIP 及七個舊編譯 target。來源、原包及驗收記錄的保留校驗通過。根目錄回條 `artifacts/ci/cleanup-20261107/cleanup-index.json`，SHA-256 `552dde586f7994e795d600ed4474a80e302e8ffb18ad16e09a962e72bc498457`。

**CORE-038 原生附件準備已合併。** #114 head `d61567831216`，main `23d260e12559`。受測合併來源 `423a320beb33`，Mac／Linux／HJW Windows 各 52 張檢查與 42 項選面原生重放通過。驗收回條 `artifacts/ci/local-pr/114/receipt.json` 的 SHA-256 為 `7cffde61c7e92cea05e12ab42402d12b5450a84ecb435c0f5b8a05368aca05dd`。SDK alpha.19、reader 56 的三端原包已驗簽，未發布候選及完整雜湊位於 `artifacts/CORE-039/pr114-native-preparation/verified-candidates.json`。飛書已鎖定三端原包，真附件上傳消費仍在進行。

**CORE-053 受限原生 Host 已合併。** #112 head `76272a48fb77`，main `5b53f8b9a041`。Mac／Linux／HJW Windows 各 52 張檢查及 42 項選面原生重放通過，回條 `artifacts/ci/local-pr/112/receipt.json` 的 SHA-256 為 `b8b32f7136f4219afcfe8aaa5eee5504ac9cb922108f27c57a62b6249c6e6a68`。選面用例採合成消費者，真擴展 minor 見證仍待完成。

**CORE-038 原話題路由。** #113 head `fda690440380`，受測合併來源 `e6e4972a76911`。已合併 main `4cfc6b3542a1`，Mac／Linux／HJW Windows 各 52 張檢查及 42 項選面重放通過。回條 `artifacts/ci/local-pr/113/receipt.json` 的 SHA-256 為 `885fde9e72340c98507c819b735dc02092560339f298504a56f234c6096d38ee`。三端 CORE-039 原生候選已驗簽，SDK alpha.18、reader 56，索引位於本倉 `artifacts/CORE-039/pr113-original-topic/verified-candidates.json`。Mac `2ba57db7107492688a12a2713b7bce7773a902336748adc8948dbe5e534b7706`、Linux `127472d66b20897792990d8026ebb3cc97e57b7fb0be2c06a81b50240dbc3fe4`、Windows `afe39aef7726c29dbcc9ad0f0a272ccb5bcce34e81ad184a185d2d56be27b0d6`。三包均為未發布本地候選。

# 核心實作進度

**CORE-038 逐段語音轉寫。** 分支 `codex/core-038-transcript-segments`，[#123](https://github.com/Institute-of-Agent-Life/yuki-core/pull/123) head `7b8831fece7c`，main `69e036bba116`。同一訊息的兩段轉寫都保留，重讀原回執沿用原搜尋版本。Mac、Linux、HJW Windows 各通過 52 張卡及 200 項原生檢查。完整合併回條在 `artifacts/ci/local-pr/123/main-confirmation.json`，SHA-256 `908801718bc307cf09489a90cdde8e98798021238c4b5b71d06b92aad3c098a5`。CORE-038／050 同源未發布包索引分別在 `artifacts/CORE-038/pr123-transcript-segments-01/verified-candidates.json`（`14ecd35e1c5341a270cb912c23e3bf8256b538f87fbbc7680ac8f0fbaa50bdfb`）及 `artifacts/CORE-050/pr123-transcript-segments-01/verified-candidates.json`（`00fc592296a02bb11bcd7ae36ce4fab15d6856612a43ceec74e1fff23295345b`）。

**CORE-050 語音來源記帳。** 分支 `codex/core-050-original-audio`。同一個原事件及段位的已購請求保留在原根，另一個 Host handle 只讀原收據。改 route、Profile、付費內容或原根時拒絕。reader 59 由舊帳本回填最早原請求，保留後續舊記帳。EXT-030 下一步驗原 QQ 前景、雙 Bot 去重與首輪文字。

**CORE-038 原生話題路由正在驗證。** 分支 `codex/core-038-native-route`。SDK alpha.18 以可選 `topic_root` 保存原根，資料 migration 56 與原事件同筆接納。既有 `topic` 與防重鍵沿用。Mac 已用實際安裝 SDK 與 Rust Host 通過 88 項預驗，涵蓋舊訊息重投、缺證拒絕、兩租戶、冷啟、忘記與規劃後替換部署。原預驗位於本分支 `artifacts/CORE-032/native-topic-preflight-02/`。正常 CORE-032 入口已過 377 項斷言，CORE-039 release 入口過 454 項，含格式、Clippy 及 writer 測試。具名驗收包與完整 SHA-256 在 `artifacts/CORE-038/native-topic-preflight/index.json`。首輪 writer 測試揭示舊的無原根呼叫形狀需繼續接納，已修正並重跑通過。三端全量仍待驗證，後續交給飛書 alpha.5 真插件消費。

**CORE-053 受限構建入口正在驗證。** 分支 `codex/core-053-native-surface`，接續 #111 的本地發版器。原 B0 schema 隨來源保存，候選 Host 明確編入 plan digest 與所選方法聯集，普通啟動自動使用受限線路。Mac 真程序的正常、拒權、冷啟及兩套 SDK 內含面核對已過 42 項斷言，原 CORE-039 正常入口與 Rust 檢查共 367 項斷言通過，CORE-053 卡入口另過 42 項。後續本地三端會在全量 suite 完成後，獨佔同一端編譯快取，再執行 `ci/native_surface_check.py`。資料 reader 55 與 wire min_reader 1.0.0 分列。三份完整 preflight、原生執行檔、SDK 與逐檔 SHA-256 的可交接包在 `artifacts/CORE-053/native-builder-preflight/index.json`。追加驗收 `artifacts/CORE-053/native-profile-admission-03/` 以同一 Bot／namespace 真正接納原事件，冷啟重投回同一事件鍵，原生構建的 331 個輸入逐檔相符。當前選面為合成 activity 控制，真能力發布繼續需要原 consumer 證據。

**CORE-053 發版器已完成。** #111 head `060f9d0c1fcb`，合併 `f255b54970b6`。Mac／Linux／HJW Windows 各 52 張檢查通過，回條 `artifacts/ci/local-pr/111/receipt.json` 的 SHA-256 為 `d0f09aabebb2d050f2f7b7ecfbd8a9b89e58f073e3bb3a20df51b23284291071`。正常卡入口涵蓋並行競爭、中斷、重算、漏證拒絕和兩套安裝後 SDK。

2026-11-06。固定 v4 行為基線 `329ca7c487414d2f54ddd98ff4727446ed2446cd`；任務書工作副本的 CORE-011 已採用 K05 的邏輯請求預留規則。下列證據路徑相對於 yuki-core 工作目錄。

**CORE-045 已完成。** #110 head `b7f2fadcaaad`，合併 `19e60c86ef8a`。Mac／Linux／HJW Windows 各 51 卡通過，原 B0／候選 Host × 已發布 Telegram 0.1.0／alpha.9 四格正常與失回應重放全部通過，兩種不相容實例被獨立拒絕。完整回條 `artifacts/ci/local-pr/110/receipt.json`，SHA-256 `6a774e46c39392f39f938c5bd4adba99ba1e095aaa9f5eaac3130bcd0b7e6678`。Linux 原包要求 glibc 2.39 以上及 curl，已在本地 glibc 2.41 鏡像驗收。擴展正式發布回條已由 #59 合併 `69b6311d7277`。

**CORE-032 activity 與 CORE-025 撤回已合併。** #108 head `78df98498670` 合併 `427ffdf8a937`，#109 head `f5a488f46c16` 合併 `dc698f5a88dd`。兩個 PR 在 Mac／Linux／HJW 各通過 50 卡。三端簽署候選與完整 SHA-256 分別在 `artifacts/CORE-039/pr108-original-recall/verified-candidates.json`、`artifacts/CORE-039/pr109-original-activity/verified-candidates.json`。後者來源 `78394082d8ea`、SDK alpha.17。全部仍為未發布候選。

擴展 #57 撤回已合併 `f2bb0c977eaf`，#58 的 Telegram alpha.9 已合併 `e7dad7c8888c`。Mac／Linux／HJW 各以 #109 同平台原包通過 22 組附件、引用、撤回及 typing 重放。真消費回條在 `artifacts/CORE-032/ext004-activity-pr58/`，完整原線路與插件原包在擴展 `artifacts/EXT-004/original-activity-17/`。本輪清理三個已閒置 SDK 驗證 target，共 197,545,414 logical bytes，71 個原包／來源／回條雜湊保持一致，清單與結果在 `artifacts/ci/cleanup-20261106/completed-sdk-verified.json`。

**CORE-038 原事件引用已合併。** #107 head `a01fdc14cb2a`，main `ddd42728b760`，本地 Mac／Linux／HJW Windows 各 50 卡通過。候選來源 `7965392bd7de`，reader 55、握手 1、Host 2、SDK alpha.15，三端簽署原包及 SHA-256 在 `artifacts/CORE-039/pr107-original-reply/verified-candidates.json`，狀態為本地未發布。擴展 #56 head `372484a836a8`、main `5ba070ca10f8`，以同平台原包各重放 10 種情境，原 Bot、群話題和首段引用皆有見證。具名回條及逐檔雜湊在擴展倉 `artifacts/EXT-004/original-reply-13/telegram-delivery.json`。核心保留交接索引 `artifacts/CORE-038/ext004-original-reply-pr56/witness.json`。

**核心任務已完成 51／58 張。** CORE-001–039、044–045、048–053、055–058 已完成。CORE-040 已取得部分驗收證據，剩餘 7 張的交接條件見下表。[逐卡分支與原產物](https://github.com/Institute-of-Agent-Life/yuki-core/blob/8555755a439b0e67bdda6f00673eaed2ab7502a0/docs/implementation-progress.md)，[先前候選、回放與清理明細](https://github.com/Institute-of-Agent-Life/yuki-core/blob/e680fc23847c8460639294eef7448d27d465b79c/docs/implementation-progress.md)。

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
| #103 | `44f0a9c60785` | `d90190c5976e` | 各 50 卡 |
| #104 | `29d5225ce545` | `3fb011eb6337` | 各 50 卡 |
| #105 | `c94b97fa376c` | `0dab3504ee39` | 各 50 卡 |
| #106 | `952bbffec228` | `6ed7f10c62d1` | 各 50 卡 |
| #107 | `a01fdc14cb2a` | `ddd42728b760` | 各 50 卡 |

各 PR 的受測合併提交、三端逐卡雜湊在 `artifacts/ci/local-pr/<PR>/receipt.json`。#99 的回執 SHA-256 為 `5abbfe3d54fe3aea254dd96a9a9bc3ed2e61a108078d2c9ca204aea97a811969`。#100 的回執 SHA-256 為 `3da2d2ed7b847f79f437bf8aece3602dec77fd891dbf48140a9100c529a6feeb`。#101 的回執 SHA-256 為 `c40e5f62f60902adaf3d73b875bee00ae410644b9bfc1d1b608ba9f2452858ca`。#102 的回執 SHA-256 為 `71aadf32e4ba5e60c7faa1ac1f1b32d7f3be1d37e746bc53f7c033f3025a22da`。41 個已整合的堆疊 PR 均逐一核對 head 已進入 main，並核驗 147 份清單；見 `artifacts/ci/pr-reconciliation-20261106/closed-41-confirmed.json`。

**模型原生搜尋。** [核心 #105](https://github.com/Institute-of-Agent-Life/yuki-core/pull/105) 的 `codex/core-024-native-web-search`（`c94b97fa376c`）讓 Main 在自己的模型連線上啟用 DeepSeek Responses 的 `web_search`。成功的查詢和讀頁次數隨原物理請求記帳；429 或回應中斷後沒有自動重送。三端回放均使用本機模型端點。另以部署憑據查詢官方模型清單，確認列有 `deepseek-v4-pro`。[部署設定](native-web-search.md)

[擴展 #54](https://github.com/Institute-of-Agent-Life/yuki-extensions/pull/54) 使用已發布 1.0.0 Host 的**獨立模型搜尋連線**。已核對 HJW 原始回環請求、根帳本及 Windows Host 的執行檔雜湊，見 `artifacts/CORE-024/ext029-upstream-native-pr54-01/witness.json`。兩種配置都無需安裝 Tavily。#105 的 Main 直連候選已在核心三端驗收，Mac 簽署原包冷啟後另由公開命令讀回四筆原帳，見 `artifacts/CORE-024/installed-native-only-pr105-02/installed-ledger.json`。

**三端開發候選（本地未發布）。** 受測來源 `19cbc9bf21f9825545e587f0738144e4db1e23b4`，合入 main `0dab3504ee39`。資料庫 reader 55、握手 schema 1、Host schema 2，Rust／Python SDK 版本及逐檔雜湊隨索引提供。`artifacts/CORE-024/native-search-handoff/index.json` 的 SHA-256 為 `07e4d8d25cc3f946488c6a57c92467de6a092a4f6a0a244b7971a88a53cd11e8`。每個 ZIP 包含部署範例、原生搜尋回執及簽署清單。

| 平台 | ZIP 的 SHA-256 |
|---|---|
| darwin / arm64 | `7b803b0e6d0d15b16faef9a6574201b337c2ba419061467497bf2976e48bcc75` |
| linux / arm64 | `9f0ecdf4993e6869a66c804a8001cebbc31061941e5dd8acf143225208f41d2d` |
| windows / x86_64 | `17a001c95122a8ad889e61732f1ebcd4943fd27a46044f6903ff18048898361b` |

公開的[握手預發布](https://github.com/Institute-of-Agent-Life/yuki-core/releases/tag/handshake-preview-dd694a0db605)提供 `initialize`。

**CORE-038 真插件。** 三端均以同一平台的原生 Host 啟動 Telegram 與 web-fetch。Telegram alpha.2 收件後缺少發送權，Main 取得失敗回執，原生發送為零。alpha.3 把網頁讀回內容納入 8,197 字元回覆，三段均獲原生定位。第二段已送出但遺失回應時保留 unknown，第三段未派，重啟後零重送。平台及模型均使用回環測試端點。九例的原始逐幀記錄、版本與雜湊見 `artifacts/CORE-038/paired-consumers-pr97/consumer-receipts.json`（SHA-256 `a62089b8a5e2fc32ee69f78c11c8f513cc12572cfe47cfb142e9f7b061e57dd8`）。

分支 `codex/core-038-gateway-media-refusals`，實作 `e680fc23847c`（#98）。兩個真插件曾因媒體不支援或讀權不足令 Main 停在 held。修正後原模型能讀回拒絕，Mac 的 CORE-038 驗過 338 項斷言，亦涵蓋無權讀取版本和類型不符。見 `artifacts/CORE-038/preflight-media-refusals-isolated-01/manifest.json` 與 `artifacts/CORE-038/paired-media-{emoji-refusal,missing-reader}-after-01/`。三端已用 #98 候選完成六次重放，見 `artifacts/CORE-038/paired-media-pr98/consumer-receipts.json`（SHA-256 `bc9d9a597cb18be86363391d24d2eee63a9eae89ecb1aa3b91fb6c8289f91143`）。擴展倉 #43 亦已提交對應的媒體拒絕見證。

**CORE-026 群訊息的人物歸屬。** 分支 `codex/core-026-event-member`，提交 `681c22435609`（#100）。固定 v4 的 `SubjectContextBuilder`／`SubjectResolver` 可用 `member_<person_id>` 在同一事件提及的兩人中分別指名。Rust 在舊版重放對應輸入時曾以 `invalid_tool_arguments` 拒絕；修正後依原事件的人物及 Assignment 驗證目標，作者與第三方歸屬分別保存。同群另一條消息的提及、未提及的人與錯誤 scope 均不得落庫。原始回執在 `artifacts/CORE-026/selected-member-oracle-02/original-member-resolutions.json`，本卡實測在 `artifacts/CORE-026/selected-member-preflight-03/selected-group-member.json`。

**CORE-026 爭議記憶的合併。** 分支 `codex/core-026-contested-merge`，實作 `d101a2a12758`（#102）。固定 v4 與 Rust 均可將已讀的爭議記憶合入同一人的活躍記憶。狀態預期錯誤或寫入關聯時中斷，兩端版本與證據都保留。合併成功後，原先的矛盾關係與新合併關係皆可查，重開能讀回原回條。固定 v4 重放在 `artifacts/CORE-026/original-guarded-merge-01/`，Rust 的預驗在 `artifacts/CORE-026/guarded-merge-preflight-01/memory-contest.json`。本 PR 三端的驗收回執在 `artifacts/ci/local-pr/102/receipt.json`；CORE-039 候選及逐平台 SHA-256 由 `artifacts/CORE-039/pr102/verified-candidates.json` 交接。macOS 用前後兩個簽署包升級同一個資料卷的重放路徑為 `artifacts/CORE-026/guarded-merge-native-upgrade-01/native-upgrade.json`。

**CORE-026／040 當前群陳述。** 分支 `codex/core-026-current-group-report`。[PR #105](https://github.com/Institute-of-Agent-Life/yuki-core/pull/105) 實作 `addd6c2ae834`。Mac 的 `artifacts/CORE-026/current-group-preflight-03/manifest.json` 通過 1,515 個斷言。對應的三端回執與候選索引路徑為 `artifacts/ci/local-pr/105/receipt.json`、`artifacts/CORE-039/pr105-native/verified-candidates.json`。固定 v4 的原始群陳述、同鍵相同和不同內容都各自建立群事實；私聊及兩種主體錯配拒絕。隔離副本逐例證據在 `artifacts/CORE-040/original-group-create-03/original-group-create.json`。reader 55 將群事實綁到原群及發言來源；三端 CORE-026 的 `current-group-report.json` 記錄讀回、串群拒絕、重啟及來源清除。這縮小 E13 的 Memory 差異，完整同功能重放仍需下表所列的安裝。

#105 首輪的 Mac 與 Linux 各 50／50；HJW 在 CORE-039 搜尋預算用例確認了握手前提尚未成立，未合併該提交。Mac 用前後兩個簽署 Host 升級同一資料卷；舊拒絕和新群陳述均可冷重開讀回，見 `artifacts/CORE-026/current-group-native-upgrade-01/upgrade.json`。reader 55 的 E13 同輸入重放四筆均送達，八項工具缺少、四項有差異；見 `artifacts/CORE-040/installed-group-pr105-01/installed-search-replay.json`。舊版 A、B 在相同模型工具及 writer 設定下的 9,600 筆冷暖配對觀測見 `artifacts/CORE-040/original-ab-paired-01/original-ab.json`。

**EXT-029 握手及撤權。** 分支 `codex/core-024-search-readiness` 的修正 `926af0817dd3` 已納入 #105。用 #104 的簽署 Host 和包內 Rust SDK 復驗，握手後實發一次回環請求，入帳兩個 provider credits；重複 begin 沒有再派。逐幀回執、Host 和兩個編譯產物的雜湊見 `artifacts/CORE-024/ext029-rust-sdk-package-03/rust-sdk-wire.json`。原包在握手前曾把搜尋工具列給模型；故障重放見 `artifacts/CORE-024/readiness-red-01/assertions.json`。修正後，依當次實例與三項實際授權公布新輪次的工具。凍結後斷線或撤權不派發、不占根額度。Mac 的 CORE-032 通過 290 個斷言，見 `artifacts/CORE-032/ext029-readiness-01/manifest.json`。該可選介面的獨立交付已停止；本輪改取上列原生搜尋交接。

**CORE-024 搜尋宣告與上限。** 分支 `codex/core-024-original-search-limits`，實作 `c0dd72c38311`、解碼修正 `62c45c94b8f1`（#101）。舊版普通工具取部署上限的實測在 `artifacts/CORE-024/original-result-limit-04/original-search-policy.json`。Rust 獨立搜尋已對回原 E13 的完整模型工具宣告，原 wire SHA-256 為 `fb18802b8b4e2e5187ae8e853808ac085ad862e3ca9d4764dee65fd0c76a016e`。部署值 3 傳到獨立搜尋請求，模型覆寫 20 在派發前被拒。同批成功與拒絕兩份回條在 `artifacts/CORE-024/e13-bridge-preflight-02/search-tool-policy.json`。原簽署版已接納的 Work 跨版本重開後仍可依舊合同搜尋 7 筆，實際送出的獨立請求及原回條見 `artifacts/CORE-024/e13-original-paid-upgrade-04/previous-paid-search.json`。

**CORE-024／032 給 EXT-029 的開發接口。** 分支 `codex/core-024-ext029-search-accounting`。部署可將 Tavily 工具綁定到明選的 provider、目的及 endpoint。`search.begin` 以原 handle 在同一筆交易預留根額度並登記物理 attempt，`search.finish` 登記實際回應和可得費用，`search.status` 可在重啟後讀原回條。安裝配置與故障時序見 [external-search.md](external-search.md)。Rust SDK `1.0.0-alpha.15`、Python SDK `1.0.0a15`、Host schema 2。逐平台驗收檔為 CORE-032／039 的 `external-search-ledger.json`；[PR #104](https://github.com/Institute-of-Agent-Life/yuki-core/pull/104) 實作 `29d5225ce545`，已合併至 `3fb011eb6337`。Mac／Linux／HJW Windows 各 50 卡通過；Mac 首輪因兩個檢出共用 debug 產物失敗，重新隔離後全量通過。受測來源 `e74e468e0485`，根回執 SHA-256 `a5a176db18f3cec039418639e07c9bcbe12d79d8c921a583ffc9411e19580d58`。三端簽署候選、實含方法、SDK 成員與重放回執見 `artifacts/CORE-024/ext029-handoff/index.json`（SHA-256 `bab0b4e0dd36e0f49f82b173283154e6c1cd5fe1740e7de1bea152f6cc0a0947`）。這三個方法屬開發版，EXT-029 的真插件消費和業務結果由擴展倉驗收。

| EXT-029 本地未發布包 | ZIP SHA-256 |
|---|---|
| darwin / arm64 | `ff2c47baf9bf8f4f4cc47e0a2a9070afc3c22d5c650571e9d2bd159bcf21e518` |
| linux / arm64 | `9550f2bd4b7e28e9fdce6e141e880d19db257b80b08ff846605e0a427642a2c7` |
| windows / x86_64 | `bbf6d5380c384d923b4859917b359ef83b281ca571bb845328e93748dcf9f4d9` |


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
| CORE-040 → 041 → 042 → 054 | 取同一安裝的裝置／工作區、普通事件與 Work 終端、external_agent 及讀網頁方法和授權回執；A／B／C 的搜尋設定還需在同一安裝對齊。現有 `environment_status` 只交 Manager readiness；`web_fetch` 與 v4 `read_webpage` 的宣告不同。完整差異與原模型請求見 `artifacts/CORE-040/installed-inputs-pr95-02/consumer-requirements.json`，普通輪次的固定 v4 回執在 `artifacts/CORE-040/original-terminal-source-03/ordinary-terminal-source.json`。取得可比安裝後重跑 A／B／C。 |
| CORE-053 後續原生候選 | #112 的受限 Host 構建已通過三端全量及選面重放。真 minor 選面仍需同來源、逐方法及新增欄位的真擴展見證。 |
| CORE-043 → 046 → 047 | 待 EXT-041 的 `extension-import-diff.json`、EXT-044 的 `desktop-package-matrix.json`、EXT-045 的 `install-restore-matrix.json` 及 EXT-042 的實物逐字稿。 |

本輪 Mac 清理回執見 `artifacts/ci/cleanup-20261106/`。先前五個閒置 incremental 目錄合計回收 17.61 GiB，Telegram 建置後另清理約 130 MiB 的中間檔案。#98 的獨立 Mac 預驗再回收 4.39 GiB，保留核心、測試執行檔和回條並重驗 SHA-256（`pr98-mac-preflight-intermediates-verified.json`）。 #99 的專用編譯中間檔另回收 Mac 8.32 GiB、HJW 1.75 GiB；執行檔與保留記錄均已重驗（`core044-mac-build-verified.json` 和 `core044-windows-owned-build-verified.json`）。 #101 完成後另回收 0.57 GiB 已被替代的本機編譯快取，保留包及回條雜湊均相符（`core101-mac-superseded-verified.json`）。

Mac 的閒置受限 Host 預驗快取已清理 4,494,281,162 logical bytes，4,364 個來源與驗收檔雜湊相符。回執在 `artifacts/ci/cleanup-20261106/native-surface-cache.json`。

Mac 原話題預驗的閒置編譯快取已清理 6,896,683,013 logical bytes，4,462 個來源、原包和驗收檔雜湊一致，詳見 `artifacts/ci/cleanup-20261106/native-route-cache.json`。
