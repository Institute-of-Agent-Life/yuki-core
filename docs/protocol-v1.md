# 插件公共基底 1.0.0

基底支援有原始雙向記錄的六個 JSON-RPC 方法。Telegram 0.1.0-alpha.4 使用 `initialize`、`gateway.inbound`、`gateway.plan_send`、`gateway.dispatch_part`。web-fetch 0.1.0-alpha.2 使用 `initialize`、`tool.call`、`tool.status`。實際安裝仍須分別授予 `gateway.inbound`、`gateway.send` 和 `tool.call`。握手 revision 為 1，Host 方法 schema revision 為 2，兩個基底 SDK 的版本均為 1.0.0。

擴展倉 EXT-067 的候選生產者、消費者提交和原始驗收清單雜湊釘在 `protocol/plugin/stable/1.0/freeze.json`。每一個入選方法的方向、授權和資料結構均由相應的協議定義匯出。七組原始記錄涵蓋正常三段送達、第二段丟失回應、撤權、跨人、錯 handle、缺方法和擦除後重開。第二段回應遺失時保留 unknown，七例重開均未重送。實測的插件只持久保存收件位置及鎖；持久保存來源資料的安裝須繼續用能處理原擦除義務的宿主。

## 驗收與使用

1. 從發布回執取得對應平台 ZIP 的 SHA-256、受測來源提交和獨立的 Ed25519 公鑰。在受信任的 core 原始碼下執行 `python ci/verify_base.py <ZIP> --sha256 <SHA-256> --commit <SOURCE> --public-key <KEY.pub>`。簽名涵蓋原生程式、協議、SDK、授權條款及 SBOM。
2. 在空資料目錄執行 `bin/yuki-base --data <DIR> init --memory-mode lexical-only`，再執行 `bin/yuki-base --data <DIR> run`。Windows 使用 `bin/yuki-base.exe`。普通核心程式也能以 `--plugin-base-1` 初始化同一基底。資料目錄記住該選擇，舊候選資料則交回原安裝讀取。
3. 從 `support/` 取得 wheel、crate、握手 golden 與 `host-schema.json`。啟動時 Host 只向每個插件列出其已授權方法。要求更高功能的插件會得到握手錯誤，健康插件繼續服務。已派發而回應不明的分段保留 unknown，重啟後沿用原收件游標。
4. 執行 `python ci/check.py CORE-044 --out artifacts/CORE-044/01/` 產生 `frozen-wire.json`、兩個已安裝 SDK 的對拍及逐卡 manifest。依 `ci/base_dist.py --help` 指定相同來源的 CORE-039 簽署原生候選、CORE-044 驗收清單、獨立雜湊、驗證公鑰與打包簽署金鑰，可生成對應平台的完整包。公開下載位置與發布狀態以進度記錄和 GitHub Release 為準。

使用獨立的真插件重放時，執行 `python tests/peers/base/qualify.py --help`。輸入為該平台的已驗簽基底包、EXT-067 鎖定原始碼與離線 Cargo 依賴。Windows 可另給擴展倉 #42 的固定插件目錄；重放器核對 EXE、鎖檔及授權後，以同一宿主執行候選與 stable SDK 兩組共十四例。
