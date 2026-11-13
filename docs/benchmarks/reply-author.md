# 被回覆訊息原作者的事實歸屬

CORE-026／040 的這個切片恢復原 `memory_change.target.subject_ref=replied_message_author`。`person_group` 的 create 指向本群唯一被回覆原訊息的原 Person，聲明作者與引文仍來自當前說話人。原作者是本人時記為 explicit；是另一人時記為 third_party。reassign 仍需先讀本人原事實的精確版本，只能改到另一人。

固定 v4 `329ca7c487414d2f54ddd98ff4727446ed2446cd` 的實跑投影在 `fixtures/memory/v4-reply-author.json`。`baseline/replay/reply_author.py --source V4 --out NEW_OUTPUT` 用該 checkout 的 Python 與臨時資料庫重建 direct、self_reply、reassigned、self_reassigned 和 missing。v4 接受前三項，拒絕後兩項。原 Mac Host `b7c0f32e377329e41afd3a1fcfce03c3ee52e55e` 的正常 direct 工具呼叫原先失敗為 invalid_tool_arguments。

正常時序：接納被回覆訊息 → 接納同群回覆 → 原模型呼叫 memory_change → 在同一 writer 交易中核對原來源、receiver Assignment、群 Entry、回覆接納前的唯一 locator、作者 Assignment 與有效 Person → 寫入事實及原引文 → 冷啟動讀回原回執。被回覆來源作為依賴隨事實讀取傳遞，並不成為提出聲明者的引文。刪除的粒度是原 execution；同一次執行產生的其他事實也會隨其來源失效。

故障時序：缺引用、跨群／私聊、歧義 locator／多個 reply 段、錯誤 scope、內部 reply_author 名稱、他人 evidence_event_id、無效作者與已刪來源均拒絕。刪除被回覆來源後，衍生事實正文及引文清除，曾讀取該事實的回執也不可重讀。晚入庫的目標不補配舊回覆；晚入庫的同號訊息不破壞已唯一的舊引用，判斷按接納序號而非訊息聲稱的發生時間。v4 admission 實跑也確認未解析的原引用不因目標後來入庫而改寫。這些額外邊界依 Rust 現行隱私規則收緊；v4 mutation oracle 的輸入已是歷史事件，沒有把跨 Entry 的原始 transport locator 當成相同驗收。

`tests/harness/reply_author.py` 隨 `python ci/check.py CORE-026 --out NEW_OUTPUT` 實跑。此次局部原生證據、舊版拒絕、新版回條、編譯輸出及來源雜湊保留在 `artifacts/CORE-026/reply-author/`。這次只改前景 memory_change，既有自動／歷史抽取路徑保留原行為。完整 CORE-040 仍為 partial：這個原公開參數的同功能重放不替代缺少的原工具宣告、匹配安裝的完整 native replay 或速度／成本裁決。
