# CORE-038：原 Main 選讀附件並呈現原處理器回條

EXT-031 的原生插件已能回傳 PDF 頁文與兩張影片幀。這條路徑使用既有 media.process/open/publish；固定模型工具仍是 inspect_conversation_attachment(event_id, attachment_index, question)。管理者在 Main 指定 attachment_processor 安裝 ID；建立原執行工具視圖時便固定該安裝的 revision 與 deployment，Work 子執行繼承同一選擇。

正常：模型只看到附件 metadata，明選 event/slot 後取得原件；PDF 交 inspect、影片交 frames。宿主以原工具的同一 operation 派發，將核准版本的影格按原順序加入同一 Main 正常續輪。不另建 vision 模型請求。未配置處理器、未選附件或無原生視覺的影片不觸發解碼。

故障：換 processor／deployment，插件斷線、來源到期、讀取時或解碼後撤權／forget、外來版本均不得發布私料。已派發結果未知不重做；已確認的效果保留原回條，呈現失敗不改寫派發事實。不替擴展實作 PDF 或影片編解碼。
