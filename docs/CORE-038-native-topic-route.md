# 原生話題的原根訊息

EXT-006 的飛書／Lark 回覆端點使用原根 message ID。固定舊來源 `329ca7c487414d2f54ddd98ff4727446ed2446cd` 的 `feishu_ingress.py` 189–218 行與 `feishu_social.py` 165–170 行提供此行為。擴展 alpha.4 已用原生 thread ID 寫入 topic 與防重鍵，因此新增原根證據時沿用原 topic。

## 正常時序

同一接納 writer 先驗安裝的 app／tenant，再接納原事件並保存其可選 `topic_root`。原根不參與既有防重鍵。新事件保存原 plugin、namespace、Bot、deployment、app／tenant 和根訊息。Main 的原 send 只讀原執行 source 的此份證據，將 `route.topic_root` 交給原插件，每次派發重新驗原部署與來源。

## 故障時序

重投回同一事件，原根衝突拒絕整筆交易。舊事件沒有原根證據時不從重投補造。舊插件省略新增欄位的重投仍讀原事件。撤權、原部署替換、忘記來源或原來源與目的話題不符時不派發新投遞。兩個租戶可擁有同樣的 native thread 和 root 字串，原接收 namespace／Bot 始終參與身份核對。

B0 原投影維持其已見證 schema。新增欄位進入未發布 SDK alpha.18，穩定 minor 仍依真 consumer 逐欄位見證。資料 migration 56 保存這項來源，minimum_reader 為 56。完整 EXT-006 還要分離上傳準備與可見投遞的逐步效果。
