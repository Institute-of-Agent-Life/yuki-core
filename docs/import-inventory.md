# v4 owner 快照盤點

`python baseline/inventory.py --snapshot <唯讀快照> --out <快照外的新檔案>`
從 `snapshot.json` 選指定來源。實例清單和報表由原 owner 在授權本地保存。
`fixtures/v4-0156/create.py` 可建立完整的合成輸入。

先由各 owner 停寫並記下同一 barrier 和隱私水位，或用 SQLite backup
在相關 owner 的凍結窗取一致副本。每個 DB 宣告主檔與已取 WAL 的
SHA-256，沒有 WAL 時明寫 null。盤點器把相符的主檔及 WAL 複製到獨立暫存目錄，
僅在副本開 SQLite。遺漏一份已宣告的 WAL 會退出失敗。

報表列出原表筆數、主鍵集合摘要、外鍵異常、不可讀索引、缺失物件、
原作用域及水位。每一列的 target 為 null，effect_delta 為零。
主庫 revision 取自 alembic_version；Connector 版本取自 dist-info METADATA；
映像 manifest 另與部署摘要核對。README、CHANGELOG 的文字只記入 documents。

參與庫、插件庫、Connector journal 和 workspace manifest 各自保留 owner。
Manager HOME、媒體、原生 protocol 及工具物件由宣告的去識別子目錄盤點。
憑據只接受 namespace 名稱。檔案根只可指向已去識別且不含祕密的 owner 資料。
原 Work 的原鍵及資料仍留在來源快照。真部署尚待授權快照，合成盤點不授予切換資格。
