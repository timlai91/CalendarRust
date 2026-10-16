use anyhow::{Context, Result, bail, ensure};
use chrono::{Datelike, NaiveDate};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

pub const PACKAGE_VERSION: u32 = 1;
pub const MAX_MONTH_BYTES: u64 = 2 * 1024 * 1024;
pub const ZODIAC: [(&str, &str); 12] = [
    ("鼠", "rat"),
    ("牛", "ox"),
    ("虎", "tiger"),
    ("兔", "rabbit"),
    ("龍", "dragon"),
    ("蛇", "snake"),
    ("馬", "horse"),
    ("羊", "sheep"),
    ("猴", "monkey"),
    ("雞", "rooster"),
    ("狗", "dog"),
    ("豬", "pig"),
];

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn file_digest(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hasher.update(&buffer[..length]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawLunar {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub is_leap_month: bool,
    pub month_name: String,
    pub day_name: String,
    pub year_ganzhi: String,
    pub zodiac: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Ganzhi {
    pub stem: String,
    pub branch: String,
    pub full: String,
    pub nayin: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolarTerm {
    pub name: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub next_name: String,
    pub next_date: NaiveDate,
    pub desc: String,
}

impl SolarTerm {
    pub fn validate(&self, date: NaiveDate) -> Result<()> {
        ensure!(
            self.start_date <= date && date < self.end_date && self.next_date == self.end_date,
            "原始節氣區間無效（raw 保留，採用值須另查核准校正）"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawDay {
    pub day: u32,
    pub gregorian: NaiveDate,
    pub week_day: String,
    pub lunar: RawLunar,
    pub day_ganzhi: Ganzhi,
    pub month_ganzhi: Ganzhi,
    pub solar_term: SolarTerm,
    pub auspicious: Vec<String>,
    pub inauspicious: Vec<String>,
    pub good_gods: Vec<String>,
    pub bad_gods: Vec<String>,
    pub auspicious_hours: Vec<String>,
    pub inauspicious_hours: Vec<String>,
    pub hour_ganzhi: BTreeMap<String, String>,
    pub zodiac_clash: String,
    pub clash_direction: String,
    pub clash_detail: String,
    pub clash_year_ganzhi: String,
    pub peng_taboo: String,
    pub fetal_god: String,
    pub lucky_direction: BTreeMap<String, String>,
    pub twelve_officer: serde_json::Value,
    pub deity_birthday: Vec<String>,
    pub deity_info: Vec<serde_json::Value>,
    #[serde(flatten)]
    pub additional: BTreeMap<String, serde_json::Value>,
}

impl RawDay {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.day == self.gregorian.day(), "日期欄位不一致");
        ensure!(
            (1..=12).contains(&self.lunar.month) && (1..=30).contains(&self.lunar.day),
            "農曆月日無效"
        );
        ensure!(
            ZODIAC.iter().any(|(name, _)| *name == self.lunar.zodiac),
            "未知年生肖"
        );
        let expected = [
            "星期日",
            "星期一",
            "星期二",
            "星期三",
            "星期四",
            "星期五",
            "星期六",
        ];
        ensure!(
            self.week_day == expected[self.gregorian.weekday().num_days_from_sunday() as usize],
            "來源星期錯誤"
        );
        Ok(())
    }

    pub fn twelve_officer_description(&self) -> Result<String> {
        if self.twelve_officer.is_null() {
            return Ok("來源未列".into());
        }
        let name = self.twelve_officer["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .context("十二建除名稱缺漏或無效")?;
        let god = self.twelve_officer["god"]
            .as_str()
            .filter(|god| !god.trim().is_empty())
            .context("十二建除值日神缺漏或無效")?;
        let is_yellow_road = self.twelve_officer["isYellowRoad"]
            .as_bool()
            .context("十二建除黃黑道標記缺漏或無效")?;
        let road = if is_yellow_road {
            "黃道日"
        } else {
            "黑道日"
        };
        Ok(format!("{name}（值日神：{god}，{road}）"))
    }

    pub fn validate_hours(&self) -> Result<()> {
        let expected: BTreeSet<&str> = "子丑寅卯辰巳午未申酉戌亥"
            .char_indices()
            .map(|(i, c)| &"子丑寅卯辰巳午未申酉戌亥"[i..i + c.len_utf8()])
            .collect();
        let good: BTreeSet<&str> = self.auspicious_hours.iter().map(String::as_str).collect();
        let bad: BTreeSet<&str> = self.inauspicious_hours.iter().map(String::as_str).collect();
        ensure!(
            good.len() == self.auspicious_hours.len()
                && bad.len() == self.inauspicious_hours.len()
                && good.is_disjoint(&bad)
                && good.union(&bad).copied().collect::<BTreeSet<_>>() == expected
                && self
                    .hour_ganzhi
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
                    == expected,
            "來源吉凶時辰重複、缺漏或互相衝突；此欄不可用"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMonth {
    pub year: i32,
    pub month: u32,
    pub total_days: usize,
    pub days: Vec<RawDay>,
}

impl RawMonth {
    pub fn validate(&self, year: i32, month: u32) -> Result<()> {
        ensure!(
            self.year == year && self.month == month,
            "來源月份 metadata 不符"
        );
        let first = NaiveDate::from_ymd_opt(year, month, 1).context("來源年月無效")?;
        let next = first
            .checked_add_months(chrono::Months::new(1))
            .context("年月超界")?;
        let days = (next - first).num_days() as usize;
        ensure!(
            self.total_days == days && self.days.len() == days,
            "來源月份缺日或重複"
        );
        for (index, day) in self.days.iter().enumerate() {
            ensure!(
                day.gregorian == first + chrono::Duration::days(index as i64),
                "來源日期序列不連續"
            );
            day.validate()
                .with_context(|| format!("{} 資料無效", day.gregorian))?;
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub name: String,
    pub sha256: String,
    pub bytes: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub source: String,
    pub upstream_commit: Option<String>,
    pub original_range: String,
    pub adopted_range: String,
    pub validation: String,
    pub correction_version: String,
    pub snapshot_sha256: String,
    pub database_sha256: String,
    pub history_sha256: String,
    pub adopted_sha256: String,
    pub hko_fixture_sha256: String,
    pub corrections_sha256: String,
    pub days: usize,
    pub invalid_solar_terms: Vec<String>,
    pub full_snapshot_invalid_solar_terms: usize,
    pub files: Vec<SourceFile>,
    pub artwork: Vec<serde_json::Value>,
}

pub struct LunarData {
    db: Connection,
    cache: BTreeMap<(i32, u32), Arc<RawMonth>>,
    lru: VecDeque<(i32, u32)>,
    pub manifest: Manifest,
}

#[derive(Serialize, Deserialize)]
pub struct HistoricalDay {
    pub date: NaiveDate,
    pub lunar: crate::calendar::LunarDate,
    pub month_ganzhi: String,
    pub day_ganzhi: String,
    pub solar_term: String,
}

#[derive(Deserialize)]
pub struct SupplementalEvent {
    pub month: u32,
    pub day: u32,
    pub name: String,
    pub emoji: String,
    pub desc: String,
}

impl LunarData {
    pub fn supplemental_events(&self, date: NaiveDate) -> Result<Vec<SupplementalEvent>> {
        let mut statement = self
            .db
            .prepare("SELECT raw FROM supplemental WHERE month=?1 AND day=?2 ORDER BY ordinal")?;
        statement
            .query_map(params![date.month(), date.day()], |row| {
                row.get::<_, String>(0)
            })?
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect()
    }
    pub fn historical(&self, date: NaiveDate) -> Result<HistoricalDay> {
        let (raw, sha): (String, String) = self.db.query_row(
            "SELECT raw,sha256 FROM history WHERE date=?1",
            [date.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        ensure!(digest(raw.as_bytes()) == sha, "歷史資料雜湊不符");
        let mut day: HistoricalDay = serde_json::from_str(&raw)?;
        ensure!(day.date == date, "歷史資料日期不符");
        let adopted: Option<String> = self
            .db
            .query_row(
                "SELECT adopted FROM correction WHERE date=?1 AND field='lunar'",
                [date.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(adopted) = adopted {
            day.lunar = serde_json::from_str(&adopted)?;
        }
        Ok(day)
    }

    pub fn period(&self, date: NaiveDate) -> Result<SolarTerm> {
        let raw: String = self
            .db
            .query_row(
                "SELECT raw FROM solar_period WHERE start<=?1 AND ?1<end",
                [date.to_string()],
                |row| row.get(0),
            )
            .context("缺少核准節氣區間，不以固定月日或其他算法補造")?;
        let period: SolarTerm = serde_json::from_str(&raw)?;
        period.validate(date)?;
        Ok(period)
    }
    pub fn open(directory: &Path) -> Result<Self> {
        let manifest: Manifest = serde_json::from_slice(
            &std::fs::read(directory.join("manifest.json"))
                .context("缺少離線來源清單；請重新解壓完整發布包")?,
        )?;
        ensure!(manifest.version == PACKAGE_VERSION, "不支援的來源包版本");
        let db = Connection::open_with_flags(
            directory.join("lunar.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .context("無法開啟離線 LunarData 資料庫")?;
        let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(version == PACKAGE_VERSION, "來源資料庫版本不符");
        Ok(Self {
            db,
            cache: BTreeMap::new(),
            lru: VecDeque::new(),
            manifest,
        })
    }

    pub fn day(&mut self, date: NaiveDate) -> Result<RawDay> {
        ensure!(
            (1970..=2051).contains(&date.year()),
            "此日期無 LunarData 資料"
        );
        let key = (date.year(), date.month());
        if !self.cache.contains_key(&key) {
            let (compressed, expected): (Vec<u8>, String) = self
                .db
                .query_row(
                    "SELECT raw, sha256 FROM month WHERE year=?1 AND month=?2",
                    params![key.0, key.1],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .context("來源月份缺檔或無法讀取；不改用另一算法掩蓋")?;
            let mut bytes = Vec::new();
            GzDecoder::new(compressed.as_slice())
                .take(MAX_MONTH_BYTES + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= MAX_MONTH_BYTES && digest(&bytes) == expected,
                "來源月份損壞或雜湊不符"
            );
            let month: RawMonth = serde_json::from_slice(&bytes).context("來源 JSON 損壞")?;
            month.validate(key.0, key.1)?;
            if self.cache.len() == 5
                && let Some(old) = self.lru.pop_front()
            {
                self.cache.remove(&old);
            }
            self.cache.insert(key, Arc::new(month));
        }
        self.lru.retain(|entry| *entry != key);
        self.lru.push_back(key);
        self.cache
            .get(&key)
            .and_then(|m| m.days.get(date.day() as usize - 1))
            .cloned()
            .context("來源缺少指定日期")
    }
}

pub fn build_package(source: &Path, output: &Path, fixture: &Path) -> Result<Manifest> {
    std::fs::create_dir_all(output)?;
    let data_dir = output.join("assets").join("calendar-data");
    let artwork_dir = output.join("assets").join("artwork");
    let license_dir = output.join("licenses");
    for directory in [&data_dir, &artwork_dir, &license_dir] {
        std::fs::create_dir_all(directory)?;
    }
    let temporary = tempfile::NamedTempFile::new_in(&data_dir)?;
    let mut db = Connection::open(temporary.path())?;
    db.execute_batch("PRAGMA user_version=1;
        CREATE TABLE month(year INTEGER,month INTEGER,raw BLOB NOT NULL,sha256 TEXT NOT NULL,PRIMARY KEY(year,month));
        CREATE TABLE history(date TEXT PRIMARY KEY,raw TEXT NOT NULL,sha256 TEXT NOT NULL);
        CREATE TABLE correction(date TEXT,field TEXT,adopted TEXT NOT NULL,PRIMARY KEY(date,field));
        CREATE TABLE solar_period(start TEXT PRIMARY KEY,end TEXT NOT NULL,raw TEXT NOT NULL);
        CREATE TABLE supplemental(ordinal INTEGER PRIMARY KEY,month INTEGER,day INTEGER,raw TEXT NOT NULL);")?;
    let transaction = db.transaction()?;
    let mut fixture_bytes = Vec::new();
    GzDecoder::new(std::fs::File::open(fixture)?)
        .take(10 * 1024 * 1024 + 1)
        .read_to_end(&mut fixture_bytes)?;
    ensure!(fixture_bytes.len() <= 10 * 1024 * 1024, "HKO fixture 超限");
    let hko_manifest_path = fixture
        .parent()
        .context("fixture 位置無效")?
        .join("hko-manifest.json");
    let hko_manifest_bytes = std::fs::read(hko_manifest_path)?;
    let hko_manifest: serde_json::Value = serde_json::from_slice(&hko_manifest_bytes)?;
    let fixture_sha = digest(&fixture_bytes);
    ensure!(
        hko_manifest["fixture_sha256"].as_str() == Some(&fixture_sha),
        "HKO fixture 與來源清單不符"
    );
    let rows: Vec<(NaiveDate, i32, u32, u32, bool, String)> =
        serde_json::from_slice(&fixture_bytes)?;
    let mut references = BTreeMap::new();
    let mut term_events = Vec::new();
    for (date, year, month, day, leap, term) in rows {
        ensure!(
            references.insert(date, (year, month, day, leap)).is_none(),
            "HKO fixture 重複日期"
        );
        if !term.is_empty() {
            term_events.push((date, term));
        }
    }
    let mut periods = BTreeMap::new();
    for pair in term_events.windows(2) {
        let period = SolarTerm {
            name: pair[0].1.clone(),
            start_date: pair[0].0,
            end_date: pair[1].0,
            next_name: pair[1].1.clone(),
            next_date: pair[1].0,
            desc: "節氣日期採香港天文台 UTC+08:00 公開曆表；臺灣日界同為 UTC+08:00".into(),
        };
        transaction.execute(
            "INSERT INTO solar_period VALUES(?1,?2,?3)",
            params![
                period.start_date.to_string(),
                period.end_date.to_string(),
                serde_json::to_string(&period)?
            ],
        )?;
        periods.insert(period.start_date, period);
    }
    let mut corrections = Vec::new();
    let mut manifest = Manifest {
        version: PACKAGE_VERSION,
        source: "LunarData／擇日知錄 · MIT".into(),
        upstream_commit: None,
        original_range: "1970-01-01..2100-12-31".into(),
        adopted_range: "1970-01-01..2051-01-31（2051 僅邊界格）".into(),
        validation: "結構／雜湊已檢查；核准 HKO UTC+08:00 為採用與比對基準；執行 verify-calendar 產生逐日報告，不宣稱第三源再驗證".into(),
        correction_version: "hko-v1；UTC+08:00 核准曆表校正；raw 保留；節氣標準繁體名稱".into(),
        snapshot_sha256: String::new(),
        database_sha256: String::new(),
        history_sha256: String::new(),
        adopted_sha256: String::new(),
        hko_fixture_sha256: fixture_sha,
        corrections_sha256: String::new(),
        days: 0,
        invalid_solar_terms: Vec::new(),
        full_snapshot_invalid_solar_terms: 0,
        files: Vec::new(),
        artwork: Vec::new(),
    };
    let holiday_bytes = std::fs::read(source.join("assets").join("holidays.json"))?;
    let holiday_values: Vec<serde_json::Value> = serde_json::from_slice(&holiday_bytes)?;
    for (index, value) in holiday_values.iter().enumerate() {
        let event: SupplementalEvent = serde_json::from_value(value.clone())?;
        ensure!(
            NaiveDate::from_ymd_opt(2000, event.month, event.day).is_some()
                && !event.name.is_empty(),
            "LunarData 補充事件日期／名稱無效"
        );
        transaction.execute(
            "INSERT INTO supplemental VALUES(?1,?2,?3,?4)",
            params![
                index as i64,
                event.month,
                event.day,
                serde_json::to_string(value)?
            ],
        )?;
    }
    manifest.files.push(SourceFile {
        name: "assets\\holidays.json".into(),
        sha256: digest(&holiday_bytes),
        bytes: holiday_bytes.len(),
    });
    for year in 1970..=2100 {
        for month in 1..=12 {
            let relative = format!("{year}\\{month:02}.json");
            let bytes = std::fs::read(source.join(&relative))
                .with_context(|| format!("讀取來源 {relative} 失敗"))?;
            ensure!(
                bytes.len() as u64 <= MAX_MONTH_BYTES,
                "來源月份超過大小上限"
            );
            let raw: RawMonth = serde_json::from_slice(&bytes)?;
            raw.validate(year, month)?;
            let adopted = year <= 2050 || (year == 2051 && month == 1);
            for day in &raw.days {
                if day.solar_term.validate(day.gregorian).is_err() {
                    manifest.full_snapshot_invalid_solar_terms += 1;
                    if adopted {
                        manifest.invalid_solar_terms.push(day.gregorian.to_string());
                    }
                }
                if adopted {
                    let period = periods
                        .range(..=day.gregorian)
                        .next_back()
                        .map(|(_, period)| period)
                        .context("HKO 缺少前一節氣")?;
                    period.validate(day.gregorian)?;
                    if day.solar_term.start_date != period.start_date
                        || day.solar_term.end_date != period.end_date
                        || day.solar_term.next_date != period.next_date
                        || day.solar_term.name != period.name
                        || day.solar_term.next_name != period.next_name
                    {
                        corrections.push(serde_json::json!({"date":day.gregorian,"field":"solar_term",
                                "raw":day.solar_term,"adopted":period,"reason":"以核准 HKO 逐年曆表重建前後節氣及跨年區間，非年份減一；含標準繁體名稱正規化",
                                "reference":"HKO / Government of the Hong Kong SAR / DATA.GOV.HK; hko-manifest.json"}));
                    }
                }
            }
            let sha = digest(&bytes);
            manifest.files.push(SourceFile {
                name: relative,
                sha256: sha.clone(),
                bytes: bytes.len(),
            });
            if adopted {
                let mut encoder = GzBuilder::new()
                    .mtime(0)
                    .write(Vec::new(), Compression::default());
                encoder.write_all(&bytes)?;
                transaction.execute(
                    "INSERT INTO month VALUES(?1,?2,?3,?4)",
                    params![year, month, encoder.finish()?, sha],
                )?;
                manifest.days += raw.days.len();
            }
        }
    }
    let mut history_hash = Sha256::new();
    let mut date = crate::calendar::CORE_MIN;
    let end = NaiveDate::from_ymd_opt(1969, 12, 31).context("歷史邊界無效")?;
    while date <= end {
        let day = crate::calendar::historical_candidate(date);
        let raw = serde_json::to_string(&day)?;
        let expected = references.get(&date).context("HKO fixture 缺少歷史日期")?;
        if (
            day.lunar.year,
            day.lunar.month,
            day.lunar.day,
            day.lunar.is_leap_month,
        ) != *expected
        {
            let adopted =
                crate::calendar::lunar_from_parts(expected.0, expected.1, expected.2, expected.3)?;
            transaction.execute(
                "INSERT INTO correction VALUES(?1,'lunar',?2)",
                params![date.to_string(), serde_json::to_string(&adopted)?],
            )?;
            corrections.push(serde_json::json!({"date":date,"field":"lunar","raw":day.lunar,"adopted":adopted,
                "reason":"歷史候選與核准 HKO 民用曆表不同，按官方日期對照採用；未修改候選 raw",
                "reference":"HKO / Government of the Hong Kong SAR / DATA.GOV.HK; hko-manifest.json"}));
        }
        let period = periods
            .range(..=date)
            .next_back()
            .map(|(_, period)| period)
            .context("HKO 缺少歷史節氣區間")?;
        period.validate(date)?;
        let expected_term = if period.start_date == date {
            period.name.as_str()
        } else {
            ""
        };
        if day.solar_term != expected_term {
            corrections.push(serde_json::json!({"date":date,"field":"historical_solar_term",
                "raw":day.solar_term,"adopted":expected_term,
                "reason":"以核准 HKO UTC+08:00 逐年節氣日期及標準繁體名稱替代歷史候選事件",
                "reference":"HKO / Government of the Hong Kong SAR / DATA.GOV.HK; hko-manifest.json"}));
        }
        history_hash.update(raw.as_bytes());
        transaction.execute(
            "INSERT INTO history VALUES(?1,?2,?3)",
            params![date.to_string(), raw, digest(raw.as_bytes())],
        )?;
        date += chrono::Duration::days(1);
    }
    manifest.history_sha256 = format!("{:x}", history_hash.finalize());
    let correction_bytes = serde_json::to_vec_pretty(&corrections)?;
    manifest.corrections_sha256 = digest(&correction_bytes);
    transaction.commit()?;
    db.close().map_err(|(_, error)| error)?;
    temporary.persist(data_dir.join("lunar.sqlite3"))?;
    manifest.database_sha256 = digest(&std::fs::read(data_dir.join("lunar.sqlite3"))?);
    std::fs::copy(
        source.join("LICENSE"),
        license_dir.join("LunarData-MIT.txt"),
    )?;
    std::fs::write(
        license_dir.join("HKO-data-notice.txt"),
        include_str!("..\\licenses\\HKO-data-notice.txt"),
    )?;
    for (name, file) in ZODIAC {
        let relative = format!("assets\\zodiac\\{file}.png");
        let bytes = std::fs::read(source.join(&relative))?;
        let original = image::load_from_memory(&bytes)?;
        let thumbnail = original.thumbnail(512, 512);
        let target = artwork_dir.join(format!("{file}.png"));
        thumbnail.save_with_format(&target, image::ImageFormat::Png)?;
        manifest.artwork.push(serde_json::json!({
            "zodiac": name, "source": relative, "source_sha256": digest(&bytes),
            "original_size": [original.width(), original.height()],
            "derived_sha256": digest(&std::fs::read(target)?),
            "derived_size": [thumbnail.width(), thumbnail.height()],
            "conversion": "image::DynamicImage::thumbnail; contain 512x512; PNG; alpha preserved",
            "license": "MIT; Copyright (c) 2026 當麻實驗室"
        }));
    }
    let snapshot = serde_json::to_vec(&manifest.files)?;
    manifest.snapshot_sha256 = digest(&snapshot);
    manifest.adopted_sha256 = digest(
        format!(
            "{}:{}:{}:{}:{}",
            manifest.snapshot_sha256,
            manifest.history_sha256,
            manifest.correction_version,
            manifest.hko_fixture_sha256,
            manifest.corrections_sha256
        )
        .as_bytes(),
    );
    std::fs::write(data_dir.join("corrections.json"), correction_bytes)?;
    std::fs::copy(fixture, data_dir.join("hko-reference.json.gz"))?;
    std::fs::write(data_dir.join("hko-manifest.json"), hko_manifest_bytes)?;
    std::fs::write(
        data_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

pub fn verify_package(directory: &Path) -> Result<()> {
    let provider = LunarData::open(directory)?;
    let actual = file_digest(&directory.join("lunar.sqlite3"))?;
    if actual != provider.manifest.database_sha256 {
        bail!("來源資料庫完整性檢查失敗");
    }
    ensure!(
        file_digest(&directory.join("corrections.json"))? == provider.manifest.corrections_sha256,
        "校正紀錄雜湊不符"
    );
    let mut fixture = Vec::new();
    GzDecoder::new(std::fs::File::open(
        directory.join("hko-reference.json.gz"),
    )?)
    .take(32 * 1024 * 1024 + 1)
    .read_to_end(&mut fixture)?;
    ensure!(
        fixture.len() <= 32 * 1024 * 1024
            && digest(&fixture) == provider.manifest.hko_fixture_sha256,
        "HKO 基準雜湊不符"
    );
    Ok(())
}
