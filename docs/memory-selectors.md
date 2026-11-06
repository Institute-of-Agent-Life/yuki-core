# CORE-026 原 selector 定位

正常時序先設：在目前人物的原主體內按原 memory_key／old_content／category 尋找，唯一精確項目再以原 fact_id 的已讀 revision／proof 更正。更新 metadata、撤回與恢復共用定位；恢復只在未到期的 retracted 原事實中找。模糊結果最多回三個候選，模型須明確讀取候選的原 ID，再用原 ID 操作。

故障時序先設：fact_id 與 selector 同給、create 給 selector、沒有合法 target、只有 category、空白定位、重複精確項目、部分文字與 SQL 萬用字元、跨人物／共同群但非本人、過期／已忘記、未讀與過時版本。前述情形不得誤選另一個事實或新增版本；候選不含私聊原證據。冷啟重放舊工具回執，不重新定位或再寫入。

固定來源為 329ca7c487414d2f54ddd98ff4727446ed2446cd 的 mutation/service.py::_locate_fact 與 repository.py::list_mutation_locator_candidates。保留原 normalizer、exact-first、最多四筆比較及三筆提示的規則；Rust 本卡 K07 的已讀原版本要求仍適用。此補核先實作 current_speaker/person；其他主體不宣告。

恢復定位讀取原 retracted 版本之前最後一份真實正文；withdraw 本身的 NULL 正文不是可搜尋內容。所有來源版本均已擦除時不產生候選。歧義候選屬於實際讀取，即使修改回滾也仍登記來源依賴，確保忘記後原候選回執被清除。測試種子透過真實 Main 原執行寫入，再逐一完成原輪，避免把尚未完成的種子輪誤當待測 selector 請求。

新增事實時，若同時提供已有的 fact_id，原版會以 `create_does_not_accept_fact_id` 拒絕。Rust 在選擇人物或群主體前執行同一檢查。付費模型故障重放使用另一個合法 key，並在拒絕後核對事實與版本數均未增加；正常建立、按原 ID 讀取及唯一 selector 更正仍走原流程。
