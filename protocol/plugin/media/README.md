# Media preview 1

候選 SDK 為 Rust 1.0.0-alpha.4、Python 1.0.0a4。Host 仍協商 revision 2；媒體方法逐項 revision 1。這組方法供真實 env、media 與 gateway 消費者對拍後再凍結。

`media.process` 由核心發給明選的 processor，原參數固定 event/slot 或 artifact/version 及目的。只有獲准的原 call handle 能用 `media.open` 讀取每塊至多 16 KiB 的 base64；每塊帶同一 digest、總長和 offset。普通事件觀測不產生讀權。

`media.publish` 綁原 call、穩定 key、filename、MIME、類型、SHA-256 和總長。每塊重播核對原位元。相同 offset 改內容、不同版本、跳塊及越界文件名均拒絕。一個 call 至多八件、總計 16 MiB；完整內容通過 digest 後才形成不可變 artifact。持有媒體的插件需要 `forget.owner`。

普通工具回傳 `data.images`，元素採 `artifact_id/version`。Host 逐張核對本插件本 call 的原件；正文效果回條與每張圖的 ready/no_vision/unavailable 分別保存。Profile 具視覺能力時，原 Chat、Responses、Messages 或 Gemini wire 才附帶原始圖片。失敗呈現保留已發生效果，冷重啟可查同一回條。

Social 明選版本後，QQ 走核心受限 transfer root。外部 gateway 的原 plan 必須保留核心交付的 `media` 描述，dispatch part 才取得該版本的讀 handle。plan 改動 digest 時，在任何物理 part 前失敗。整條線路沿原 Person、Work、接收端、來源水位與操作鍵；只有實際已送出的版本進入 Client9 原檔目錄。

獨立候選的來源、方法清單、SDK／schema 雜湊及每個成員驗證記在 `candidate.json`。讀路徑與實際回條由進度檔交接，發布與跨倉驗收狀態另列。
