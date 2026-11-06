# CORE-015 原工具讀取補核

固定行為來源為 329ca7c 的 ToolArtifactRepository 與分頁函式。23 個原始回條由 `baseline/replay/artifact_pages.py` 在唯讀隔離副本取得，存於 `fixtures/tool-results/v4-original-pages.json`。Rust 舊版分頁對拍、Main 原參數續讀與來源失效測試已在 Mac 通過；跨平台候選仍在驗證。

正常時序：原工具把受控不可變結果落庫並交句柄；模型按原 handle、operation、陣列 path 呼叫 inspect／get／search／text；讀取前保存實際版本與授權，離開 reader 執行解碼，發布前核同版本及來源資格。翻頁保完整成員、Unicode 字元座標及整體回執預算；原查詢游標與 Artifact offset 分開。

故障時序：不存在／失效／另一入口的句柄拒絕；解碼期間撤權或清除後不發布；第一個成員過大不切碎；search 回完整命中物件並去重；文字 casefold 展開不改原字元 offset。既有顯式 version／JSON Pointer 的 operator 呼叫維持可讀。JSON Artifact 的 image 操作回具體非圖片錯誤，不偽造圖片。

既有版本／JSON Pointer 格式保留在 operator 入口及原凍結清單；新 Main 使用原陣列 path 並由核心在讀取時固定 SHA-256，發布時核同一版本。讀取前也核正文雜湊；撤權或清除後不交晚返資料。原件只含 JSON；image 對這些句柄回 artifact_not_image，不宣稱提供原生圖片 Artifact。

對拍來源：23 組固定舊版 helper 回條，oracle fixture SHA-256 `dcb699b0bf7e99b2fbbe9503f7e5d90576da7171778410e8c8429c88ed928732`。重取原 oracle 的 JSON 欄位完全相同；原 ZIP 與隔離舊倉未改。原工具 DTO 與核心逐項效果回執仍分列，不宣稱整個 CORE-040 wire 已等價。

待完成：三平台乾淨來源候選、雜湊交接及當前 PR 的三平台 CI。
