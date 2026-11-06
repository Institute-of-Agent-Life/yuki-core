# CORE-026 原事實有效期間

正常時序先設：原事件的 Main 使用部署時區解讀沒有 offset 的 ISO 日期／時間；明示 offset 轉為 UTC。create、correct 和 update_metadata 在同一事實寫入交易內保存有效期間與版本快照，讀回及冷啟仍指向原期間。先以固定原版 resolver 和實際 mutation 對拍省略欄位、僅給起點、僅給終點以及更正的規則，再落實 Rust 行為。

故障時序先設：無效日期、倒置期間、空白終點、過時已讀 proof、偽造時區及已忘記事實，均不得留下半筆 metadata 或新版本。夏令時間重複／跳過時刻按原 ZoneInfo fold=0 解讀，精度保留到微秒。過期事實不進普通召回與 selector；按原 ID 的歷史查閱不被誤當恢復授權。遺忘清除各版有效期間，舊回執重放不恢復它。

來源固定為 `329ca7c487414d2f54ddd98ff4727446ed2446cd`。此項沿用原人物與已讀版本權限，不擴大可寫主體。

原版實測：update_metadata 省略期間保留原值；只給起點／終點會替換整個期間。模型 correct 省略日期清除期間。未攜 metadata patch 的既有有型別 writer 保持原合同，提取器繼續保存其自身完整分類與期間，不讓空期間列攔下原提取 metadata。

reader 45 新增按 fact.status 清理目前 metadata 的 trigger，升級時亦清除既有 erased 事實殘留；原 reader 44 實際 binary 所產的已忘記與活躍事實 fixture 驗證定向清除、活躍期間保留及原回執不變。既有 reader 43→目前 reader 的原請求重放測試也通過。Host／握手／SDK 沒有變更；沒有新增付費 API。
