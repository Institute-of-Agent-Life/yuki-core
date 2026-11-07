# 逐能力 minor 發行

CORE-053 提供本地組包與資格檢查。實際 GitHub 發布沿已核准的單次發行操作進行。使用者指定全部 CI 在本地執行，因此本卡沿用 `ci/check.py`，工作流入口為下列 CLI。

## 正常時序

1. 從上一個已發 stable 的原 manifest、原 IDL 投影和原消費清單建立 `base.json`，核對其 SHA-256。基底含 `version`、`surface`、`consumers`、`min_reader` 和 `handshake_sha256`（原 initialize schema 只除去 status／SDK 版本 metadata 後的規範 JSON SHA-256），並保存原發布／見證摘要。首次 registry 的版本為 `1.0.0`。
2. 從唯一 IDL 明選本次新方法，或既有方法的可選欄位。`plan` 讀最新已驗版本，保留全部既有穩定面與消費者義務，輸出下一個 minor、原父摘要、來源 commit、IDL 摘要、聯集與 `min_reader`。
3. 在該乾淨來源生成兩套 SDK，準備限制為該聯集的三端 Host。三端用原舊消費包與本次真消費包重新跑正常、拒權及冷啟案例，保存逐幀記錄和功能斷言。每份回條綁定本次 plan、同一平台 Host、同一套 Rust／Python SDK 及原 consumer package 的 SHA-256。
4. `promote` 重算唯一 IDL 投影、檢查原檔摘要、兩 SDK 內含的方法與版本、Host 廣告的方法、逐方法／可選欄位的原 wire、功能結果及三端消費清單。通過後才形成完整 ZIP。
5. 在跨進程鎖內再比父摘要，以一次 rename 將完整版本放入 `versions/1.n.0`。原始證據、包摘要、兼容義務與 `min_reader` 隨 manifest 交付。包狀態為本地 `release-ready`，發布完成後由獨立發布回條記錄實際遠端位置。

## 故障時序

缺真包、缺逐方法證據、漏舊 consumer、缺正常／拒權／冷啟功能斷言、SDK 含未選方法、Host 廣告額外方法、來源或任一摘要不符，都在可見版本建立前退出。新增必需欄位、缺省效果、原 grant、舊欄型別或約束的修改會被拒絕。

兩個候選可以同時產生 `1.1.0` 計畫。第一個取得完整資格後佔用該版本。第二個在最終鎖內發現父摘要已前進，必須重新 `plan`、生成新的聯集 SDK／Host 並重跑全部消費者。舊回條的 plan 摘要會被拒絕，第二個最後產生 `1.2.0`。準備過程使用不可見暫存目錄，普通失敗會清除自己的暫存，進程中斷留下的暫存也不會成為 stable。

## 操作入口

```sh
python ci/promote-surface.py init --registry /path/registry --base /path/base.json --sha256 <原基底摘要>
python ci/promote-surface.py plan --registry /path/registry --request /path/request.json --out /path/plan.json
python ci/promote-surface.py build-sdks --registry /path/registry --plan /path/plan.json --out /path/sdk-candidate
python ci/promote-surface.py build-host --registry /path/registry --plan /path/plan.json --out /path/native-host --target-dir /path/idle-native-target
python tests/peers/base/profile_check.py --host /path/native-host/yuki-core --kit /path/sdk-candidate --plan /path/plan.json --out /path/native-admission
python ci/promote-surface.py promote --registry /path/registry --plan /path/plan.json --evidence /path/qualification
```

`request.json` 包含 `parent_stable_digest`、`candidate_source`、本能力真包 ID 的 `consumers`，以及 `selection`。新方法選擇為 `{"gateway.activity":{"new_method":true}}`。可選欄位使用原 schema 鍵路徑，例如 `{"gateway.inbound":{"fields":[["params","properties","optional_field"]]}}`。巢狀欄位以相同的 `properties` 鍵交替延伸。結果欄位從 `result` 開始，證據必須包含原請求 ID 對應的回應。

資格目錄的 `evidence.json` 包含以下原件。

- `synthetic`、`plan_sha256`、`candidate_source`。
- `assets` 固定為 `host-macos`、`host-linux`、`host-windows`、`rust-sdk`、`python-sdk`。每個原件均使用相對 `path` 及 `sha256`，SDK 必須是實際 crate／wheel。
- `platforms` 固定為 `macos`、`linux`、`windows`。各平台的 `artifacts` 綁定自己的 Host 與兩套共用 SDK 摘要。`consumers` 必須等於 plan 中的完整聯集。
- 每個 consumer 包含 `source_commit`、原 `package`，以及 `cases`。必需 `normal`、`denied`、`restart`，可另加該功能的未知效果、部分失敗、忘記與撤權等驗收。
- 每個 case 指向一份有摘要的原 JSON。該檔含 `status=PASS`、全為 true 的具名 `assertions`、`plan_sha256`、`artifacts`、`consumer_sha256`、`advertised_methods`、已見證 `methods` 與有摘要的 `transcript`。逐行 JSON transcript 使用 `direction` 和原 `frame`。

原功能的場景覆蓋仍依矩陣該列的真 consumer 驗收。組包器檢查這些回條的來源與完整性，現場執行器負責產生真實的功能斷言。Host 必須實際限制到選定方法，較大的開發候選需要先生成對應的受限 Host 並重放，才能交給此入口。

`init --synthetic` 只建立獨立測試 registry，產物永久標記 `synthetic-control`。其回條不能交給實際發行 registry。`python ci/check.py CORE-053 --out <新目錄>` 使用真 B0 見證作基底，兩個合成欄位測競爭、重算、漏證拒絕及原包保留，另從正常 CLI 生成一組標記合成的 SDK。這些測試的 `real_capabilities_promoted` 為空。

## 受限原生 Host

`build-host` 從原 plan 與 SHA-256 編入所選聯集，使用本機已鎖定的 Rust 與離線 Cargo cache。正式 registry 要求來源乾淨且 commit 精確匹配。編譯前後逐檔核對來源，輸出二進位與 `host-build.json`。目標編譯目錄須處於閒置狀態，避免與使用該執行檔的驗收並行。

受限二進位的普通 `init` 與 `run` 自動選用編入的聯集。冷啟沿資料卷標記選同一條穩定線路。`plugin-surface` 在開啟資料目錄前輸出原 profile digest、來源及方法清單。一般開發版的 `--plugin-base-1` 使用隨來源保存的原 B0 schema 投影，因此後續 IDL 的可選欄位仍需明選並取得新見證。

正常時序先編譯，再用兩套安裝後的 SDK 交換握手，讓既有 tool grant 與新增 activity grant 在真 Host 上線。故障時序讓未選的 forget grant 無法啟動，讓已握手程序的未選 callback 被拒，然後冷啟重跑同一組檢查。`profile_check.py` 驗證指定 `gateway.activity` 的隔離控制，完整能力發布仍使用前述真 consumer 回條。

`plan.min_reader` 是插件 wire 版本。原生 `host-build.json` 另列實際初始化觀察到的 `storage.schema_version` 與 `storage.minimum_reader`。本來源的資料 reader 為 55，原 B0 發行物為 54。切換資料卷時使用資料 reader 限制與 K14 的驗收回條。所有受限候選仍標 `published=false`。

本地全量 suite 完成、編譯快取取得獨佔使用後，再執行 `python ci/native_surface_check.py --out <新目錄> --target-dir <閒置 target>`。它從原 B0 自動建立 activity 合成 profile，調用正常 SDK／Host 構建入口並重跑上述真程序隔離控制，輸出帶來源與逐檔 SHA-256 的 `validation.json`。三個本地平台各執行一次。
