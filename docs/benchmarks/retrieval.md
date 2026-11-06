# CORE-029 授權內檢索

正常 Main 的 `search_memory` 和本機 `operator memory retrieve` 都先按當次真人、原執行及日期取得可讀 fact，才從同一 SQLite snapshot 排名。兩次讀之間只重用查詢向量。資料缺失或陳舊、有限候選近似與分頁截斷在回條中分欄。日期使用 `valid_from` 的左閉右開範圍。

預設使用 Rust 精確餘弦；`--backend vec1` 保留固定的 Vec1 0.8 比較路徑。其原始碼、摘要及版權在 `native/vec1/NOTICE`，主機候選包也攜帶該檔。選用 Vec1 時分頁建立鏡像，每批 512 筆；大庫收到 `index_building` 可續同一請求，此時尚未派發 Embedding。`operator memory prepare-reference EXEC ACTOR` 可預先逐批準備。

固定 v4 比較程式在 `bench/retrieval/vec1_matrix.py`，實際核心 owner、權限、查詢向量與 CLI 的重現程式在 `bench/retrieval/core_matrix.py`。矩陣含 1,500、10,000、100,000 筆 1024 維向量，三種直接共同群可見比例、兩組日期及定量撤回與更新。每格各量四次。舊 Vec1 的 SQLite 原生計時與完整核心請求分別記錄。兩個 Rust 後端在同一資料庫交錯執行，資料庫大小包含比較用鏡像；RSS 為 owner 自啟動以來的採樣最高值。

`bench/retrieval/decision.json` 保存原始報告路徑與 SHA-256。36 組前 20 筆順序全部符合舊版；全範圍十萬筆的 98,592 筆可讀記錄，精確後端耗時 1.92–2.47 秒，Vec1 耗時 3.13–3.40 秒。兩者先校驗原事實版本與內容，再返回結果。這是本機 arm64 release 的觀測值。
