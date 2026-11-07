# CORE-045 固定版本相容驗收

固定已發布的 plugin-base 1.0.0 與 Telegram 0.1.0，另固定擴展 #58 的三端 alpha.9 原包。候選 Host 使用本次正常 CI 編譯出的 release 執行檔。穩定 Host／候選 Host 與穩定插件／候選插件組成四格，各使用獨立資料目錄，驗證原 Bot 入站、三段文字、第二段失回應及冷啟讀回原效果。

每格同時安裝兩個故障實例。一個使用最低支持版本以外的 offer，另一個在 Host 回應中移除必要的 namespace。兩者均須失敗，健康實例須完成投遞。原始與變異後的線路各自保留。首次發行以前沒有 stable，歷史版本格明列 N/A，B0 覆蓋四格的固定基底。

`ci/compat-lock.json` 綁定 GitHub 發行資產、擴展提交、SDK 版本、大小和 SHA-256。`python ci/compat_matrix.py prepare` 只下載固定公開輸入至摘要命名的快取。正常卡入口先準備輸入，再在 loopback-only worker 執行 `python ci/check.py CORE-045 --out <新目錄>`。已存在的快取每次驗 SHA，下載中斷不會留下可接受的半包。此入口使用公開資產，無發布密鑰。

具名輸出 `compat-cells.json` 記錄支持範圍、逐格來源、執行檔摘要、原生回執及拒絕理由。新增預覽能力的正式發布仍依 CORE-053 的真消費證據裁決。

Telegram 0.1.0 與 alpha.9 的 Linux arm64 原包要求 glibc 2.39 以上及 curl。本地矩陣使用既有擴展驗證鏡像 `sha256:3668e80e411d173fd09120481df76baa4c50e2c2e3620f192cffdc49a0623e96`，其 glibc 為 2.41。`ci/compat_matrix.py` 在下載／重放前檢查這兩項條件。最初使用的核心鏡像 glibc 2.36 缺少 curl，載入原插件時回報 `GLIBC_2.39 not found`，該輪保留失敗。四格在符合要求的本地鏡像重新執行。

每個插件 relay 保留獨立 stderr，Host 每次結束前保存各安裝的 online 狀態，供定位握手前的原生載入失敗。
