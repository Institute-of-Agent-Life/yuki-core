# QQ 冷暖重放與 Rust 投資判斷

CORE-040 的三個執行端共用同一份帶 SHA-256 的 QQ 封包。A 使用固定舊版 `329ca7c487414d2f54ddd98ff4727446ed2446cd`，B 在隔離副本套用 `bench/controls/b.patch.gz`，C 使用 CORE-039 已驗證的 Rust 執行檔。每個冷、暖條件都啟動新程序與資料目錄。暖條件先處理一組獨立事件，其後才開始計算測量事件。迭代時輪換三個執行端的順序。

正常時序從固定封包、模型請求、閘道確認追到原始持久事件。報告逐輪記錄正確交付數、CPU 秒、每 50 ms 抽樣的程序樹 RSS、DB／WAL 與延遲分布。少一段確認發送、替換候選二進位檔或移除來源回執都會被檢出。失敗的工作仍留在日誌和分母中。

以安裝了 psutil 的 Python 執行 `python bench/trials.py --source A --optimized B --python V4_PYTHON --library SQLITE_LIBRARY --core-manifest CORE039_MANIFEST --binary CORE_BINARY --out NEW_TRIALS`。預設兩次重複，每個條件 1,200 個測量事件。每次使用新的輸出目錄。再執行 `python bench/trial_report.py NEW_TRIALS NEW_REPORT.json`，由原始封包、日誌及各輪的回執重建報告。完整重放另需本地舊版依賴及相同的 SQLite library。

在 macOS 上，用已驗來源 `b74a91582f22` 完成六次冷暖程序重放，24 個測量事件全部正確。既有十二輪的原始記錄重核得到 14,400／14,400 個正確效果，那批舊記錄缺少 C 執行檔的來源回執。兩批的 SQLite writer 設定一致；實際模型工具宣告仍有差異，速度比較與投資裁決待完整等功能資料。回執在 `artifacts/CORE-040/native-trials-smoke-02/{trial-report,historical-report}.json`。長 Work 與 Compact 的驗證另見同目錄下的專文。
