# 如何重跑 v3.2 文件與原件核對

**本輪：v3.2 核心文檔補核版。** 首先執行 `python audit/verify_core_review.py`；要核對與原交付包之間的依賴差分，追加 `--base <續核交付包.zip>`。它核對 H 案例、原件內的程式行段、來源雜湊及舊 JUnit 節點，不執行 Yuki。輸入與產物見[逐項映射](results/core-doc-crosswalk.json)與[本輪實跑結果](results/core-review-check.json)。以下全倉盤點是沿用的可選工具，不在本輪範圍或首穩前置內。

四項檢查互不代替：文件結構、原件內容、目標 Git 倉盤點、交付 ZIP。前三個腳本只讀輸入，在指定目錄寫報告；不需要服務憑據，也不會啟動 Yuki。使用 Python 3.11 以上的標準庫；完整源碼盤點另需 Git 與本機固定版物件。以下從解壓後包含四份 Markdown 的目錄執行。

```sh
python audit/verify_plan.py
python audit/test_verifier.py
python audit/verify_supplement.py --out /tmp/yuki-32-r2
python audit/verify_evidence.py --archives audit/evidence --out /tmp/yuki-32-evidence
python audit/verify_package.py /path/to/Yuki-Rust重寫任務書-v3.2-核心文檔補核版.zip --root .
```

| 腳本 | 核對對象 | 本輪實際結果與不涵蓋範圍 |
|---|---|---|
| `verify_plan.py` | 從最終兩倉卡讀節點、先決、傳遞可達輸入、具名產物、直接下游；反核矩陣的逐卡映射、波次、跨倉交接、發布排除與明示錨點 | [plan-check.json](results/plan-check.json)。不根據生成器的記憶體模型報通過。審查 IDL 方法語義、遷移合法性仍要讀文字與日後產品測試，不由拓撲證明。網址中外部來源不計本地連結通過數。 |
| `test_verifier.py` | 對**臨時副本**故意換 owner、刪前置、改原件名、造環、破壞反向邊、發布門檻、波次、錨點等 | [adversarial.json](results/adversarial.json)。必須是正常版通過、損壞版被拒，不把預期錯誤當測試失敗。 |
| `verify_evidence.py` | 三個舊 [CI run 41551516736](https://github.com/YuanYeYouTao/Yuki/actions/runs/41551516736) 下載包；每個完整 ZIP SHA-256、CRC、16 個 Compact 案例原碼快照、已知六段出站、用量分母、遷移部分成功、套件 Python 行數 | [ci-source-proof.json](results/ci-source-proof.json)、[CI 447 檔用途與 SHA-256](results/ci-members.tsv)、[LOC 逐檔](results/package-loc.tsv)。舊 CI、非真平台，並非本輪重跑產品。`tests-integration` 不是全部 Yuki 測試。 |
| `inventory.py` | 遍歷指定 Git commit 所有 blob，正文先讀 AGENTS、架構索引、開發合同，再逐檔分批，列絕對 commit 與 blob、重複映射、引用、附件及互斥 LOC 類別 | 本輪未取得完整指定 checkout，故**沒有對目標 SHA 的全庫 inventory.json、全庫實際 LOC 或全文已讀認證**。已知來源另列[覆蓋裁決表](../全量文檔覆蓋與裁決表.md)。UTF-8 解碼成功也不代表內容已被判讀。 |
| `verify_package.py` | 查 ZIP 成員、重名、路徑安全、CRC、位元組數與雜湊清單，選驗解壓目錄 | ZIP 留在已解壓目錄之外，另行指定 `--root`。`results/delivery-sha256.tsv` 不記錄自己的 hash；核對程式會重新產生清單 bytes 並核整份，不信任 ZIP 中任意聲明。 |

## 基線、版本和數字

原倉 `main` 查詢與指定 SHA 相同：`329ca7c487414d2f54ddd98ff4727446ed2446cd`。[CI 測試 commit 對基線的實際比較](https://github.com/YuanYeYouTao/Yuki/compare/9138a528872bb9775b9ac05ddb38b5c1de6d7df2...329ca7c487414d2f54ddd98ff4727446ed2446cd)僅三份文檔不同。不能因此聲稱 CI 在 329ca7c 重跑，或將舊 README 解讀成現在的 schema。`upstream-integration.zip` 是原 artifacts 13193122831、`upstream-wheel.zip` 是 13192595427、`upstream-connector.zip` 是 13191891934。原 GitHub 下載可能有期限，包內留相同 bytes；以 `ci-source-proof.json` 的雜湊辨識。

實際盤點的 736 個 Python 檔來自兩個獨立分發範圍：主 wheel 中 `src/yuki` 679 檔 179,375 實體行、`src/yuki_plugin_sdk` 18 檔 2,147 行；Connector sdist 的執行程式 16 檔 5,693 行、腳本 3 檔 336 行、測試 20 檔 5,930 行。實體行含空白與註解，最後未換行算一行。生成檔依檔名/檔頭另分，這批 Python 中沒有匹配者，不是全倉生成碼為零。Vec1 原生二進位不折算成人寫 LOC；沒有完整主倉測試樹、遷移、vendor 的基線總數。新版各項仍未知。

## 取得完整 checkout 後的離線盤點

```sh
# 只讀已有 checkout；不 clone、不修改分支、不提交。
python audit/inventory.py /path/to/Yuki --ref 329ca7c487414d2f54ddd98ff4727446ed2446cd --out /tmp/yuki-32-fullgit
```

`docs-tree.tsv` 是逐路徑目錄；`body-batches.tsv` 提供每批的行區間與內容摘要。取正文用 `git -C /path/to/Yuki show '<commit>:<path>'`。`doc-refs.tsv` 區分同倉實檔、站外與未解析引用；正文中自由文字不保證可靜態解析。`evidence-tree.tsv` 先列指定目錄附件，再補明示引用中位於其他目錄的附件。`source-loc.tsv` 先列第三方、生成、遷移、測試、實驗、運維、執行；分類依路徑／標記，是可審的初分而非作者證明。工具不執行上游 scripts、patch、fixtures 或歷史命令，也不以程式讀過 bytes 代替逐份裁決。若比固定 commit 不同，需另存一份結果，不能覆蓋原基線。

將完整來源清單與本輪**已作出的裁決**接起來，不從存在性自動發簽核：

```sh
python audit/reconcile_coverage.py /tmp/yuki-32-fullgit --out /tmp/yuki-32-joined
```

輸出保留兩邊單獨存在的路徑、不同 blob、解析失敗、完全相同的文件對應；不會把「尚未讀」自動變成「已核」。主倉源碼仍需固定 clone 或完整 Git 物件，單獨用 wheel 當 checkout 會報錯。這兩步未在完整 Yuki Git 樹上跑過。

## 續核：可驗證的目錄範圍、原 CI 與資料缺口

`verify_supplement.py` 從逐檔 TSV 重建 Git tree，逐一核對五個分目錄的固定 SHA；另從所有直接子項重建 `docs/` tree，確認其中 12 份頂層升級指南均已閱讀（只認這一層，不延伸至其餘子樹）；再從最終任務卡與矩陣核 R01–R23 的唯一 owner、用例和具名產物。讀取三個新增的歷史 Actions ZIP，核 SHA-256、CRC、JUnit 真實案例／skipped，追到 Compact 原 call、原結果、下一個正常 Main；讀取原生隔離報告、原 wheel 的實際 Python 函式與精確行數。見[續核結果](results/r2-source-proof.json)、[另三個 CI ZIP 的 302 個成員](results/r2-ci-members.tsv)。這些查核不需付費 API，不運行產品。轉交時以原件內容、不是上游 job 名稱判定可支持的結論。

`upstream-unit.zip` ID 13193688341、`upstream-native.zip` ID 13192842650、`upstream-headed.zip` ID 13193680354。三者來自舊 run 41551516736，PR head `b7fad5c89c4e261b5221285a0ecc277b3f4b37bb`；Compact fixture 內自報 checkout `9138a528872bb9775b9ac05ddb38b5c1de6d7df2`。兩者都不是在 Rust 或原行為 SHA 上重新測試。headed 為 Linux 服務端的真實有頭環境，不是外部 Desktop 的各平台發行。[Desktop 另一個 run](https://github.com/Institute-of-Agent-Life/Yuki-Desktop/actions/runs/41541324476) 的 Intel macOS job（ID 125605924671）E2E failure 後打包 skipped。不把過去一次的失敗永久判定為不支援，按實際候選重新驗。

### 單檔帶入完整來源，不含未提交檔案與 Git 歷史

已有本機 Yuki checkout 時：

```sh
git -C /path/to/Yuki archive --format=zip -o /tmp/Yuki-329ca7c.zip 329ca7c487414d2f54ddd98ff4727446ed2446cd
python audit/check_snapshot.py /tmp/Yuki-329ca7c.zip
```

此檢查不執行上游內容，只從 ZIP 中的路徑、模式及原始 bytes 重建 Git root tree。根必須是 `c1885cc75e3273aea6f4ed4e994b762b0cdfba6e`。若原檔被 export-ignore、行尾轉換或權限資訊遺失而無法吻合，改用上方 `inventory.py` 直接讀 Git 物件；不降低校驗。已在含中文路徑、執行位元、symlink 的合成 Git 測試含竄改負例，詳見[原始結果](results/snapshot-selftest.json)。沒有在完整 Yuki ZIP 上取得目標根；不把合成測試或五個子樹當作全庫覆蓋。
