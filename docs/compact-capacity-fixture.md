# CORE-023 硬上限反例

PR #57 與 #59 的 Ubuntu CI 在原 private-compact 的 hard 情境第一個模型請求前停止，診斷為 model_request_capacity，物理 attempts=0。增加原工具宣告後，完整首輪已超過 fixture 固定的 20,000 上限，尚未走到「不可丟棄未觀察工具輪」的反例。

fixture 改用明示 64,000 上限，先確認包含完整真實工具宣告的初始請求可入場，再回覆 64,000 個中文字的原 get 參數。未觀察的原輪必須保留，超過同一硬上限時仍須 held；不得派發 E 或新的正常請求。新增斷言確認全程恰好一次物理模型 attempt。產品容量計算、裁剪規則及測試拒絕條件不變。

原 CI 證據：artifacts/CORE-023/github-pr57-42120758440、github-pr57-42120747418、github-pr59-42124961198。本地重現與修正驗證接續記錄。

修正驗證：`artifacts/CORE-023/ci-hard-capacity-mac-01/` 全部通過；同一失敗亦在 `ci-regression-mac-01/` 以最新原來源重現。完整正例、unknown、reset、erased、硬上限、重啟及原 Work 範圍測試均保留。
