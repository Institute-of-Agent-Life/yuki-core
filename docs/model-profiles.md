# 模型設定與原請求

CORE-048 使用固定 v4 Admin DTO、v3 TOML 和聯合檔案 revision。HTTP 提供 `GET /admin/v1/models`、`POST /admin/v1/models/apply`、`POST /admin/v1/models/secrets`；其餘管理操作由 CORE-037／055 接續。

在資料目錄的 `yuki.toml` 設定 `model_profiles_file = "model_profiles.toml"`，並保留 `[main]` 的根預算、網路允許範圍和工具政策。部署者可從 `config/model_profiles.example.toml` 建立設定。相對檔案路徑以資料目錄為準；已存在的註釋、未知欄位和 server-side headers 會保留。公開讀回不包含 headers 或密鑰。

可選 `operator_registry_file` 指向固定 v4 的 `[[operators]]` 宣告，包含 UUID4 `principal_id`、`token_env` 和明確 `capabilities`。模型讀／寫能力分別為 `control.config.file.content.read`、`control.config.file.mutate`。HTTP 每次查驗原 bearer 或真正的 chat 設備資格；已識別的 operator 缺權時直接拒絕。OS operator 使用既有私有 socket／named pipe。

```text
yuki-core --data <資料目錄> operator profiles read
yuki-core --data <資料目錄> operator profiles apply <ModelsBody JSON>
yuki-core --data <資料目錄> operator profiles secret <SecretBody JSON>
yuki-core --data <資料目錄> operator profiles receipt <原 request_id>
```

ModelsBody 必須帶原 UUID4 `request_id`、讀回的 `expected_revision` 和完整公開 `document`；可選 `api_keys` 以檔案中已參照的環境別名更新旁檔。SecretBody 使用 `name`、`value` 及相同的 ID／revision 規則。精確格式見 `protocol/baseline-v4/admin-openapi.json`，不補造省略的版本欄位。

六個 task route 都參照已宣告 Profile。聊天新激活使用 `chat_agent`；已啟用的核心提取使用 `memory_extraction`。已批准的輔助 route 可在其部署 `[plugins.aux_routes.<name>.profile]` 指定 `profile_task`，明選 `memory_self_reflection`、`memory_dream`、`plugin_agent_session` 或 `utility_structured`。外部呼叫不能選擇任意 Profile；原 route 的 purpose、來源、入口、網路權限和根額度仍生效。部署檔中未指定 task 的既有輔助 route 保留原設定。

Main／Work、提取批次及輔助邏輯請求會封存選定 Profile，後來的設定變動不改原合同。獨立搜尋使用 Main 封存的 `search_connection`，只送搜尋輸入。原進程仍持有舊憑據時可續接；冷啟動找不到匹配的原憑據時，保留未派發請求，不改用新密鑰，也不增加未知請求的重試權。

非聊天連接變動回報 `saved_pending_restart`。重啟時，已啟用的 hybrid embedding 讀取其連接與旁檔憑據，新的模型／憑據域使用不同索引身份；明選 lexical-only 不會啟用 embedding。搜尋的全域啟停仍約束 Profile，`disabled` 不因存在密鑰而啟用。vision、ASR 和 semantic observer 的安裝及實際消費仍由各自 owner 接入；未取得 semantic observer 的載入回條時，`semantic_matches_loaded` 為 null。

保存先記原內容 hash、原操作與目標 bytes，再替換原目錄中的檔案。原 ID 重送會查回執與目前 hash；外部內容不符時拒絕覆寫。兩個操作者使用同一舊 revision 只有一個可成功。若進程在替換中途終止，可用同一 body／ID 恢復；pending 的 hybrid 連接暫停新模型呼叫，health 顯示 degraded，恢復回條要求重啟後才啟動連接。

驗證入口為 `python ci/check.py CORE-048 --out <新的產物目錄>`。案例使用實際 Rust owner、隔離資料、HTTP recorder 和兩個打包 SDK；Linux 額外掛載 `/yuki-profile-readonly` 時驗 kernel readonly mount。候選 ZIP 帶 commit、schema／SDK 版本、每個成員 SHA-256 和驗證清單，標記未發布；真實 EXT-049 消費證據另行記錄。
