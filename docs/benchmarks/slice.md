# QQ 冷暖重放與 Rust 投資判斷

CORE-040 的三個執行端共用同一份帶 SHA-256 的 QQ 封包。A 使用固定舊版 `329ca7c487414d2f54ddd98ff4727446ed2446cd`，B 在隔離副本套用 `bench/controls/b.patch.gz`，C 使用 CORE-039 已驗證的 Rust 執行檔。每個冷、暖條件都啟動新程序與資料目錄。暖條件先處理一組獨立事件，其後才開始計算測量事件。迭代時輪換三個執行端的順序。

正常時序從固定封包、模型請求、閘道確認追到原始持久事件。報告逐輪記錄正確交付數、CPU 秒、每 50 ms 抽樣的程序樹 RSS、DB／WAL 與延遲分布。少一段確認發送、替換候選二進位檔或移除來源回執都會被檢出。失敗的工作仍留在日誌和分母中。

以安裝了 psutil 的 Python 執行 `python bench/trials.py --source A --optimized B --python V4_PYTHON --library SQLITE_LIBRARY --core-manifest CORE039_MANIFEST --binary CORE_BINARY --out NEW_TRIALS`。預設兩次重複，每個條件 1,200 個測量事件。每次使用新的輸出目錄。再執行 `python bench/trial_report.py NEW_TRIALS NEW_REPORT.json`，由原始封包、日誌及各輪的回執重建報告。完整重放另需本地舊版依賴及相同的 SQLite library。

在 macOS arm64 上，A、B、C 各以冷、暖條件重複兩輪，每個新程序測量 1,200 個事件，14,400 個事件全部交付正確。C 的執行檔已綁定提交 `b74a91582f22a027c7c23d6dcafa8a7336800ba9` 與 CORE-039 manifest。逐輪資源用量及原始記錄雜湊在 `artifacts/CORE-040/native-cold-warm-bound-01/trial-report.json`，SHA-256 `f8ef9111edf8b00849ee9fdfdf3ab74affecc20bf29cf3c3d2b44f3a7ad21385`。A、B、C 的 SQLite writer 設定一致，模型的工具宣告不同，供應商帳單尚缺，速度比較及成本裁決仍待等功能資料。長 Work 與 Compact 的驗證另見同目錄下的專文。

## 失敗輪次的計量

報告按每次啟動列出狀態、原定輸入數、退出碼及實測資源。程序退出前已量到的 CPU、RSS 和時間仍計入記錄，尚未取得的值保留為 null。效果不符時保留第一個差異。新失敗輪次在寫入日誌清單時一併記下 process.log 的 SHA-256；從舊記錄重建時會標明雜湊是在重建時取得。

固定在提交 `eb0a7c49f3f3` 的 Rust 候選與兩個 v4 隔離副本進行了故障重放。為 A、B 選用系統 Python 後，兩者均因 SQLite 3.51.0 與釘選版 3.46.0 不符退出。冷暖合計六個程序、預定十二個測量輸入，Rust 的四個輸入交付正確，另外四個程序皆有退出碼、耗時、CPU 和原日誌雜湊。報告見 `artifacts/CORE-040/failed-trial-accounting-01/mixed-report.json`。同目錄的 `failed-history.json` 收錄另一次歷史實跑的全部十二個失敗輪次；`source-bound-replay.json` 由逐輪明細再次核實 14,400／14,400 個正確輸入。

## 既有環境插件的實際回條

使用擴展倉的固定提交 `dfe8eb46d64ac12e68596a457fe5cc3554ce6acf` 在隔離副本建置 `env 0.1.0-alpha.1`。使用它選定的 Rust SDK `1.0.0-alpha.2`，crate SHA-256 為 `9408022bdf1322a8d6e07914685869771a5738280e36145e46da5c36facbfce0`。原版 Manager 的來源 ZIP 為 `c21901e0eca498be372507d3b600281ed2e43e221bc90744f762b8470cbdfab6`。用該插件的 manifest 啟動 Host，讓模型從實際宣告呼叫 `environment_status`。每次均錄得一筆派發、兩次同 handle 的 `tool.status` 與下一輪模型回條。

| 實跑環境 | 原版 Manager 已連線 | 沒有 Manager | 回執及 SHA-256 |
|---|---|---|---|
| Linux arm64，核心 `02a8bcf9d7c8` | Manager 回傳 `ready: false`，插件成功交付這個狀態 | `manager_unavailable` | `artifacts/CORE-040/real-env-dfe8eb4-linux-02/environment-status-replay.json`，`6db866c2320cd2cf5efdea3fe26cc3319c4c4a14fb711e436a5e84eceeb2b17f` |
| macOS arm64，核心 `b74a91582f22` | 已到原 socket，Manager 回傳 `invalid_request`，模型收到 `invalid_manager_response` | `manager_unavailable` | `artifacts/CORE-040/real-env-dfe8eb4-mac-01/environment-status-replay.json`，`c1cf1cc45180a54472fa5072ff8e5551b5790bd868fb640bed2bfbf2857e20b8` |

原 v4 的同名工具接受 `device_id`，可查該裝置的授權、系統和 Shell。這個已安裝版本只接受空物件，提供 Manager 的 readiness。兩份宣告與四次模型回條均保存在上述回執。對拍仍需要含原裝置範圍的版本，以及原 Work 的 terminal_exec、terminal_read、terminal_control 和 environment_service 實際後端。
