# 原執行主體權限目錄

CORE-037 補接原 `get_my_capabilities` 到 Main。先固定 329ca7c 的模型宣告、descriptor 與 summary／focused／full 結果，再用 Rust 解析目前原主體；目錄只提供原權限 metadata，不執行或授予操作、不回傳設定值／密鑰，也不表示插件或設備當前在線。

正常時序：原工具 intent → 當前原執行授權 → 由原 Person 的目前 root 角色或已登記 SELF／委派來源決定 user／superuser → 按 category／query 過濾 → 短 writer 再核角色及來源 → 發布同格式結果。查詢不使用模型傳入的 Person／角色；summary 不展開整個目錄，focused 返回相關項，full 返回原 ID 分組。

故障時序：他人 ID／未知參數、無錨來源、已撤角色／停用人物／轉移帳號、已清來源不得取舊超管結果；同一次讀取到發布間撤權也重算或拒絕。SELF 與原委派不得借另一個超管人物升權。無匹配回空目錄；未知 category 不改為全部。結果只作內部參考，不作執行回執。

固定舊版 oracle 的 16 組結果、真 Main 往返、同參數撤權後重查、冷啟動與 writer 發布邊界均通過。大型 focused 結果保留原 Artifact 及 SHA-256，並由 Main 依原 path 合同回讀 counts。人類委派跟隨原人的目前角色；SELF 與其子執行保持 user。三平台乾淨候選及後續跨倉消費見 implementation-progress.md。
