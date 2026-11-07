# 原生附件準備與可見投遞

EXT-006 的飛書／Lark 先上傳 image/file 得到原應用資源鍵，再用此鍵發送訊息。兩個 HTTP 都有自己的效果邊界。原 `gateway.dispatch_part` 成功會寫入可見投遞，因此新增 `gateway.prepare_part` 保存上傳回條，沿用 Host 的 operation、handle、來源和未知結果處理。

## 正常時序

原 plan 可返回帶 `preparation=true`、`kind` 與原 `media` 的準備步，後續可見步以 `prepared_part` 指向先前的準備步索引。Host 每步使用原 parent 與 ordinal。準備只准讀 Main 明選的原版本，保存原 namespace、Bot、deployment 和 app／tenant。確認回條中的 resource 只供同一原 send 的後續一步使用。真正訊息送達後才寫 social_outbound 與附件投遞關聯。

## 故障時序

上傳成功後，訊息或附言失敗仍保留已確認上傳回條。未知上傳或未知可見結果停止後續步，冷啟讀原結果，禁止自動重送。前向索引、重複消費、換媒體、跨 parent、跨部署、撤權、忘記或原版本刪除均在新 HTTP 前拒絕。回應缺少原資源鍵保留 unknown。舊 plan 繼續走原 dispatch 路徑。

本輪屬未發布 SDK alpha.19。飛書真插件必須對拍原上傳與發送端點，再提供逐方法見證。完整 EXT-006 另外保留 owner-disposition 與現場驗證範圍。

## 已驗證入口

`python ci/check.py CORE-032 --out <new-directory>` 會執行 `tests/harness/native_preparation.py`。CORE-039 對同一入口使用發行構建。案例涵蓋純上傳回條、首個可見步的原引用、失回應、附言失敗、非法順序與重複消費，以及上傳後換部署、撤讀權、原媒體版本失效。缺原 app 證據的歷史事件不能靠重投補寫。Mac 最新預驗共 120 項斷言。

原路由同時交付 canonical entry 的 `scope`，插件依 private／group 選擇原生收件 ID 類型。SDK alpha.19 的新派發預驗已驗私聊與兩個租戶的群話題，發布構建通過 583 項斷言。

## 原檔名補齊

Main 未指定 `name` 時，規劃回呼從同一獲授權的不可變媒體版本取回 `filename`。明確指定的名稱沿原請求傳送。原 operation 的 payload 保留首次接納內容，補齊值只進原 plan 的已綁定參數。驗收同時核對缺欄位、明確改名，以及既有撤權、換版本和未知準備結果的停止時序。
