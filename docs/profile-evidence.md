# Profile 證據

`python ci/probe-profile.py --out <新目錄>` 產出 H23 四欄矩陣。固定 v4
在無外網下跑原 adapter，保存實際 request/response、nodeid、來源雜湊及
JUnit 摘要。結構化輸出的 payload 建立測試另列。真端點預設待驗。

Profile 鍵含完整 endpoint、region、model、wire、thinking 與 task。換代理
即建立新鍵。原生搜索須配對 server tool 事件與來源；完成但無來源、
被忽略、HTTP 拒絕及供應端工具錯誤各自保留。結構化搜索另外驗 results
及 URL，不能作為原生搜索證據。

官方資料查閱日為 2026-11-04。Chat 的 JSON 模式、Responses 搜索的
action.sources、Messages 的 server_tool_use 與 tool result、Gemini
GenerateContent grounding 各源自其協議文件。可查核連結分列於
`baseline/profile-evidence/sources.json`。

## 另行授權的端點測試

`--profile` 接受只含上述六項識別欄位的本地 JSON。指定 `--live`、
`--key-env <環境變數名稱>` 及 `--authorization <本地授權檔>` 才可派發。
未 opt-in、缺憑據、授權到期、profile_key 不符，都不發出請求。

授權檔需有 UUID authorization_id、authorized=true、相同 profile_key 及
task、帶時區的 expires_at、max_usd、endpoint_hard_cap_usd 與
endpoint_cap_receipt。授權者先在該專用端點確認供應端硬額度並記入 receipt。
測試程式以 authorization_id 原子預留單次額度，只送合成材料；錯誤與
逾時保留預留，不自動重試。未收到計費原件時 cost_usd 保持 null。
產物留在本機，內含已去除此探針憑據的請求、回應或傳輸錯誤類別。
