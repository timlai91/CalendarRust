# CalendarRust

依 `pcalendar-rust-spec.md` 0.5 實作的獨立 Rust Windows 桌面年曆。產品名稱與主程式皆為 **CalendarRust**；不是原 pCalendar 作者／工房或擇日知錄發行的版本。

**目前為 0.1.0 候選版，尚未宣告 M5／全部人工驗收通過。** 已實作本輪 P0／P1 功能；下方列明相容性差異及仍需目標電腦驗收的項目。

## 啟動與資料

解壓完整 `CalendarRust-0.1.0-windows-x64.zip`，雙擊其中的 `CalendarRust.exe`。請保留旁邊的 `assets`、`licenses` 與本文件，不能只複製 EXE。唯讀資源相對於執行檔解析，不依賴工作目錄、原 LunarData 目錄、Python、Office、Rust 或 Cargo。

目標環境為 Windows 11 x64。使用本機已安裝的微軟正黑體／新細明體及 Segoe UI Emoji，不下載或散布 Windows 字型；缺少繁中文字型時會明示錯誤，請安裝 Windows 繁體中文補充字型。程式以靜態 MSVC CRT 建置，無安裝程式、尚未簽章，Windows SmartScreen 可能提出未知發行者警告。

使用者資料位於 `%LOCALAPPDATA%\CalendarRust\`：

| 路徑 | 用途 |
|---|---|
| `calendar.sqlite3`、可能存在的 `-wal`／`-shm` | 備忘、設定、圖片映射及匯入紀錄 |
| `images` | 內容定址的受管理 PNG |
| `backups` | 還原／匯入前的自動完整備份 |
| `calendar.lock` | 排除第二個寫入實例 |

不要只複製正在使用中的 SQLite 主檔。請透過「設定／資料管理 → 建立完整備份 ZIP」取得一致快照；不會把備忘正文寫入診斷 log，也不自動連網。

## 功能與操作

- 1912–2050 年，42 格月曆、日曆、ISO 列週數、民國／西元切換、年／月導覽及日期帶。主介面沿用 B 分區及 C 配色，收緊為 840 × 640；頂部工具列高 32，控制項等高並垂直置中，左緣對齊主曆區。工作區不足時可雙向捲動，設定可調縮放。
- 日期提示依實際資料逐行顯示；無事件、無錯誤或無節氣資料時不保留空白行。
- 左側時鐘與「今天」保留真實本地時間；選日高亮、事件、備忘、圖片與農民曆共用同一選日。午夜只有在跟隨今天時切日，保存失敗不偷換選日。
- 備忘支援繁中、emoji、換行，停止輸入 1 秒保存；失焦、切日／月／模式、Ctrl+S 與離開時 flush。空字串刪除，純空白保留，上限 256 KiB。失敗保留草稿並阻擋切換，可重試、另存草稿或明確放棄。
- 每日自選圖片與年曆抬頭圖分開管理。支援 JPEG／BMP／ICO／PNG；Windows WMF／EMF 靜態轉點陣。等比顯示，不刪外部原圖。原始輸入限 20 MiB、8192 px、3200 萬像素；無可驗證範圍的非 placeable WMF 明確拒絕，不以正方形拉伸。
- 農民曆另開原生詳情視窗，顯示宜忌、干支／納音、沖煞、彭祖百忌、建除、胎神、方位、神煞、十二時辰及來源介紹。缺資料／null／壞來源分別顯示，不改算法掩蓋。生肖採有授權的 512 px 等比縮圖；自選每日圖優先。
- 純 Rust XLSX，12 月、3 欄 × 4 列、A4 橫向、七色、可選內嵌抬頭圖、資料來源明細。預覽使用真實一月資料及相同色彩規則。匯出在背景執行、凍結選項、可取消，成功後才提交目的檔；不含私人備忘、每日圖路徑、外連或巨集。完成後可主動以系統關聯開啟。

Tab 移動焦點；月曆日期方向鍵移動焦點，Enter／空白選日。文字編輯保留游標／Ctrl+A。日期格、日期帶與背景有右鍵命令選單。

| 快捷鍵 | 功能 |
|---|---|
| Ctrl+F1／F2 | 上／下一年 |
| Ctrl+F3／F4 | 上／下一月 |
| Ctrl+F5 | 今天 |
| Ctrl+F6 | 民國／西元 |
| Ctrl+F7 | 月曆／日曆 |
| Ctrl+S | 保存備忘 |
| Ctrl+T | 最上層 |
| Ctrl+I／Ctrl+A | 說明／關於（文字編輯時 Ctrl+A 仍全選文字） |
| Ctrl+Q | 安全離開 |

## 曆表、校正與授權

1970–2050 及邊界月份採本地 **LunarData／擇日知錄** MIT 快照，共封裝 29,616 日；1912–1969 的基礎農曆原值由 MIT `lunar_rust 1.0.1` 預生成，不在 runtime 建立無上限歷史算法 cache，亦不補造 1970 年前的農民曆宜忌。

經使用者核准，採 **香港天文台／香港特別行政區政府／DATA.GOV.HK** 的 UTC+08:00 公開逐年曆表修正歷史農曆差異並重建所有前後節氣區間，包含跨年。1925 年候選閏四月有 30 日差異；原 LunarData 全快照辨識 556 筆節氣區間異常，封裝範圍內 364 筆。原始 JSON 與歷史候選皆保留，沒有直接把錯誤年份減一。

| 檔案 | 內容 |
|---|---|
| `assets\calendar-data\manifest.json` | 原始檔／快照、採用範圍、資料庫／歷史／HKO／校正 hash，生肖來源、尺寸及縮圖參數 |
| `lunar.sqlite3` | gzip 原始月份、歷史候選、採用農曆校正、HKO 節氣區間及可選補充事件 |
| `corrections.json` | 原值、採用值、原因及依據；含節氣名稱標準繁體化 |
| `hko-reference.json.gz`、`hko-manifest.json` | 逐日事實與來源逐檔 URL／SHA-256／取得時間 |
| `verification-report.json`（發布包） | 50,770 日農曆年月日／閏月、歲次／生肖及節氣事件對照結果 |
| `licenses\LunarData-MIT.txt` | 完整 MIT 與 Copyright (c) 2026 當麻實驗室 |
| `licenses\HKO-data-notice.txt` | 公開數據再用、權利歸屬、來源、條款及限制 |
| `licenses\third-party\index.json` | 固定 lockfile 的第三方套件、授權、通知原文／來源及 hash |

**HKO 同時是核准的採用及比對基準；零差異不等於所有校正另經第三個獨立來源驗證。** UTC+08:00 民用日期對照也不代表重建所有歷史地區法定時間或不同流派。歲次／生肖以農曆正月初一換年，不以立春或公曆元旦換年；節氣事件只在交節日顯示，非整段期間每天重複。

事件為使用者核准的獨立整理清單，不保證與原版全部標籤等價。公曆紀念日、農曆節日、多事件、除夕及月末關鬼門皆有來源身分；閏月不重複一般農曆節日。設定可開啟 LunarData 的 93 筆公曆節日與民俗補充，僅明列的別名去重，其餘保留來源。不推算補班／補假；補假色只用圖例。

**節日／紀念日不等於現行法定休假。農民曆為民俗參考，不作安全、醫療或重大決策依據。** 原版避凶／托盤精確還原、三派／飛星／個人化及子時換日互動依核准留待 P2，本版不宣稱等價。

## 備份、還原與舊資料

完整 ZIP 備份包含 SQLite online backup 快照、設定及被引用的受管理圖片，附 schema／來源版號與 SHA-256。還原前檢查清單、版本、安全路徑、hash、資料庫 schema／引用及備忘鍵；經預覽確認後先備份現況，再交易完整取代，**不是合併**。未知或較高 schema 拒絕開啟，不建立空資料掩蓋。

舊版匯入不執行或修改舊 EXE，不搜尋系統敏感目錄。請明確選擇備忘 `.txt` 目錄、可選 `pCalendar.dat` 及相對圖片路徑基準。掃描嚴格 UTF-8／Big5／CP950／有 BOM 的 UTF-16，預覽日期／編碼／正文；未確認歧義整批阻擋，除非明確同意部分匯入。DAT 前八字元為西元 `YYYYMMDD`，剩餘保留完整空白／中文路徑。每項衝突選擇略過、取代或合併備忘，提交前自動完整備份；來源 fingerprint 避免重複覆寫，結果報告列出實際處置。沒有使用者真實舊資料樣本，實際相容性仍需確認。

## 從原始碼建置

開發環境需 MSVC Build Tools／Windows SDK；固定工具鏈見 `rust-toolchain.toml`，版本依賴見 `Cargo.lock`。一般啟動不需這些工具。

```powershell
Set-Location CalendarRust
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked --bin verify-calendar -- 'assets\calendar-data' 'tests\fixtures\hko-calendar.json.gz' 'tests\fixtures\comparison-report.json'
cargo build --release --locked --target x86_64-pc-windows-msvc --bin CalendarRust
```

開發時資源並非位於 target 目錄：

```powershell
cargo run --locked --bin CalendarRust -- --resources . --data-dir '.\target\dev-user'
```

完整封裝：

```powershell
.\scripts\package.ps1
```

此腳本執行檢查、核對固定版本授權、建 Release、驗證 x64／PE32+／GUI subsystem，產生 `dist\CalendarRust-0.1.0-windows-x64.zip` 與同名解壓目錄，附檔案 hash。不覆蓋既有同名目錄。已附離線第三方通知；變更 lockfile 後首次重收集時，從本機 crate 及缺漏項目的固定上游 Git revision 取得通知，需要 `gh` 與網路。程式本身不連網。

只有要重新生成來源包時才需要獨立的 LunarData 快照；Python 僅用於更新開發 fixture：

```powershell
python scripts\fetch-hko-fixture.py
cargo run --locked --bin build-data -- '..\LunarData-main' '.' 'tests\fixtures\hko-calendar.json.gz'
```

不要自行更新 HKO fixture 後省略校正審核與全範圍驗證。來源包、校正清單、採用 hash 及報告需一起更新。

固定時間截圖使用獨立測試資料，不接觸一般資料：

```powershell
cargo run --locked --bin CalendarRust -- --resources . --data-dir '.\target\screenshot-user' --fixed-time '2026-10-14T15:34:00+08:00' --screenshot '.\target\CalendarRust.png'
```

測試時可加 `--selected-date '2026-10-10'` 及 `--view month`／`day`／`export`，重現今天與選日不同的固定狀態；截圖完成後會正常保存並離開。這是畫面狀態檢查，不取代滑鼠／鍵盤／IME 操作驗收。

`.\scripts\smoke-test.ps1` 會以隔離資料及不同工作目錄啟動 Release，擷取三種檢視，核對今日標題並實際開啟選日的原生農民曆視窗；加 `-MeasureIdle` 可記錄本機 60 秒真實時鐘的 CPU／working set，結果與截圖寫入 `target` 的獨立子目錄。不傳送全域鍵盤／滑鼠輸入，只向本次測試 PID 的視窗送訊息；不同參考機及混合 DPI 仍需另驗。

## 驗收界線

本次本機已取得的證據：

| 項目 | 結果 |
|---|---|
| Rust 核心／整合測試 | 22 項通過；格式與 strict Clippy 通過 |
| 核准 HKO 基準 | 50,770 日；農曆／閏月／歲次／生肖差異 0，節氣事件差異 0，不可用節氣 0 |
| Release 原生畫面 | 月曆、日曆、真實一月匯出預覽；今天 14／選日 10 分離；繁中與 emoji 備忘可顯示 |
| 農民曆 | 實際點選後另開含選日日期的原生視窗，正常保存及關閉 |
| 本機可見 idle，真實時鐘 | 60.88 秒；22 邏輯處理器，整機平均 CPU 0.028%，最大 working set 169.28 MiB |

這是目前開發電腦的結果，不是規格參考機或全套人工驗收的替代。初測曾有 222 MiB 記憶體超標；修正為只在正黑體缺少時載入新細明體替補，再重新完成 60 秒量測，沒有停用曆表／生肖資料。

自動化涵蓋全支援月份網格、邊界／歲次、原始資料忠實度、核准 HKO 全日基準、保存故障注入、Unicode 持久化、單寫入實例、匯入衝突、備份還原及壞來源／路徑拒絕、XLSX 內容／樣式／圖片／列印結構。這些不能替代下列實機驗收：

- 繁中 IME 組字、完整鍵盤／右鍵操作與螢幕閱讀器，睡眠／調時／午夜恢復。
- 96／120／144／192 DPI、混合 DPI 螢幕移動與拔除、低工作區／放大字級的焦點及捲動。
- 真實磁碟滿／唯讀／斷電與保存中強制終止；真實舊版備忘／DAT 樣本。
- Excel／LibreOffice 實際開啟與 A4 列印預覽；不同來源 WMF／EMF 圖檔。
- 參考機 NFR-02 至 NFR-05 的完整 UI 啟動／100 次互動、60 秒 CPU／working set、5 次匯出量測。
- 未安裝開發工具／Office 的乾淨 Windows 11 x64，解壓後雙擊、不同工作目錄及斷網啟動。

未完成的人工項目不是已通過；本候選版不宣告規格全部發布門檻結案。
