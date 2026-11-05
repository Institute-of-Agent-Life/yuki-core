# 安裝候選宿主

CORE-039 的分發按來源 commit、系統與架構各產生一份 release ZIP。取得候選時，從交接記錄取得預期的 commit、ZIP SHA-256、fixture SHA-256 及可信簽署公鑰。驗證程式須取自該 commit 或已驗證的分發工作。每包列出實含方法、資料 reader、SDK 版本及各成員雜湊。

```sh
python3 verify_dist.py --package "$ARCHIVE" --public-key "$PINNED_SIGNER" \
  --commit "$SOURCE_COMMIT" --package-sha "$ARCHIVE_SHA256" \
  --fixtures-sha "$FIXTURES_SHA256" --destination ./installed-host
./installed-host/bin/yuki-core --data ./new-data init
./installed-host/bin/yuki-core --data ./new-data run
```

Windows 使用 `bin/yuki-core.exe`。初始化會建立新的資料目錄；程序回報監聽位址後可查 `/readyz`。ZIP 包含 release 執行檔、握手與業務 IDL、兩個 SDK、golden、獨立程序 fixture、來源索引和建置依賴授權清單。

在來源 checkout 執行 `python ci/check.py CORE-039 --out artifacts/CORE-039/01` 可重跑原生宿主及雙 SDK。以 `python ci/candidate_job.py native --manifest artifacts/CORE-039/01/manifest.json --out artifacts/host --key /path/to/candidate-signing-key` 簽署已驗來源。Linux OCI 使用 `python ci/candidate_job.py oci --out artifacts/linux-oci --key /path/to/candidate-signing-key`。OCI 裝載同一來源的非 root 核心；測試時停用網路和 Linux 權能，根檔案系統唯讀，僅資料目錄可寫。

下載 OCI 後，將兩個校驗程式放在同一目錄，執行以下命令。各項 pin 取自可信交接記錄。指令會重新遍歷 OCI 描述檔、config 和 layer 的內容雜湊。

```sh
python3 verify_oci.py --package "$OCI_ARCHIVE" --manifest oci-candidate.json \
  --public-key "$PINNED_SIGNER" --commit "$SOURCE_COMMIT" \
  --package-sha "$ARCHIVE_SHA256" --platform linux/arm64
docker load --input "$OCI_ARCHIVE"
```

Candidate host 工作流程可手動為指定 commit 產出原生包及 OCI 驗證附件。預設簽署金鑰只用於該次工作，公鑰可從受信任的工作回條另行釘住。正式包以指定發布者公鑰核驗；每份候選的發布狀態和下載位置見實作進度。
