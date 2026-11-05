# 原模型工具視圖補核

CORE-015 正常時序：固定原版宣告與 Code 直調集合 → 本部署實際已安裝的核心工具取交集 → 凍結完整清單與 revision → 原生模型讀 lookup_tools 摘要／單項原 schema → 業務操作仍經原執行授權。查詢不增加工具、不啟動 VM、不更新 frozen revision。

故障時序：lookup 函式對未知名稱、混用 name 與分頁、超界或異型參數回拒絕；真 Main 中未知名稱不影響同批獨立時間查詢。不符合 paid schema 的模型回應仍由既有解碼層 held，逐項 schema 錯誤續跑需另行補核。多詞 Unicode casefold 搜尋、240 字元原摘要、最後一頁、隱藏 schema 換 revision 及來源撤權均需驗證。重啟後沿原凍結清單，不以新配置竄改原執行。

本卡補原 lookup_tools、time_get_current、update_short_state 宣告與已存在核心能力的 Code 可見性。模型 lookup 保留原結果信封，外層仍是核心效果回條。已交接的 Host code.lookup 是獨立 SDK 合同，保持現有格式；已安裝擴展的明示 code_direct 仍按該合同處理，穩定消費驗收待真擴展回條。未安裝的舊擴展工具不因固定名稱清單而出現。

固定原 oracle 50 組純函式結果與四個原版 provider 用例通過；Mac `CORE-015/original-tool-views-mac-02/` 127 項及 `tool-views-adjacent-01/{CORE-012,CORE-013,CORE-024,CORE-034,CORE-037}` 通過。核心效果外層、原 Artifact 預算與授權邊界保留。已補 direct 模式排除 execute_code 的原投影；實際已配置 Code 的模式才聲明它。
