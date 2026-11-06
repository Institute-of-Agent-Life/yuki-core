# CORE-038：原 Main 選讀附件並呈現原處理器回條

EXT-031 的原生插件已能回傳 PDF 頁文與兩張影片幀。這條路徑使用既有 media.process/open/publish；固定模型工具仍是 inspect_conversation_attachment(event_id, attachment_index, question)。管理者在 Main 指定 attachment_processor 安裝 ID；建立原執行工具視圖時便固定該安裝的 revision 與 deployment，Work 子執行繼承同一選擇。

正常：模型只看到附件 metadata，明選 event/slot 後取得原件；PDF 交 inspect、影片交 frames。宿主以原工具的同一 operation 派發，將核准版本的影格按原順序加入同一 Main 正常續輪。不另建 vision 模型請求。未配置處理器、未選附件或無原生視覺的影片不觸發解碼。

故障：換 processor／deployment，插件斷線、來源到期、讀取時或解碼後撤權／forget、外來版本均不得發布私料。已派發結果未知不重做；已確認的效果保留原回條，呈現失敗不改寫派發事實。不替擴展實作 PDF 或影片編解碼。

HJW 的實際消費者來自 EXT-031 candidate-03，程式 SHA-256 `2d0e7e9b910789bf34b8d263475c71a720a2ee650628fc88a3292a64e26d454c`。原核心候選 `0ef956a` 與這個未改動的插件一起運行：讀取入站 slot 1 的原 PDF 並呈現兩頁，影片返回 0 秒與 5 秒的兩幀；Main 實際收到的兩份影格 bytes 雜湊與 FFmpeg 輸出相同。PDF 與影片各派發一次處理器、兩次本地模型 HTTP。這是 Windows 選讀路徑的真擴展消費證據，不涵蓋 macOS／Linux 上的擴展編解碼、付費模型或協議正式發布。

重現入口：`python ci/consume-ext031.py --help`。提供 Windows 候選 ZIP 與 SHA、同提交的凍結原始碼、原擴展 evidence 目錄及其 package index；輸出到全新工作目錄。[可讀回條](../fixtures/media/ext031-windows-main.json)。Windows 原始回條 SHA-256 `3e43a514cee1abce0873da4393c610a02c48e65956a1ec359367ae244c09dcd7`，完整 HTTP 位於 `artifacts/CORE-038/main-selected-real-ext031-01/`。
