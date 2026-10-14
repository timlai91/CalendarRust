use crate::data::digest;
use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use fs2::FileExt;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const SCHEMA: u32 = 1;
const SCHEMA_SQL: &str = "
    CREATE TABLE memo(date TEXT PRIMARY KEY,body TEXT NOT NULL,updated_at TEXT NOT NULL);
    CREATE TABLE image_asset(asset_id TEXT PRIMARY KEY,path TEXT UNIQUE NOT NULL,sha256 TEXT NOT NULL,format TEXT NOT NULL,width INTEGER NOT NULL,height INTEGER NOT NULL);
    CREATE TABLE daily_image(date TEXT PRIMARY KEY,asset_id TEXT NOT NULL REFERENCES image_asset(asset_id),updated_at TEXT NOT NULL);
    CREATE TABLE app_settings(id INTEGER PRIMARY KEY CHECK(id=1),body TEXT NOT NULL);
    CREATE TABLE import_record(fingerprint TEXT PRIMARY KEY,summary TEXT NOT NULL,updated_at TEXT NOT NULL);";
pub const MEMO_LIMIT: usize = 256 * 1024;
pub const DEFAULT_COLORS: [u32; 7] = [
    0xFF80C0, 0x8EC7FF, 0x00FF00, 0x00FFFF, 0x808080, 0x0080C0, 0xD2E6D2,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalendarDisplay {
    #[default]
    Roc,
    Gregorian,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub display: CalendarDisplay,
    pub colors: [u32; 7],
    pub zoom: f32,
    pub always_on_top: bool,
    pub window_position: Option<[f32; 2]>,
    pub header_asset: Option<String>,
    pub supplemental_events: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            display: CalendarDisplay::Roc,
            colors: DEFAULT_COLORS,
            zoom: 1.0,
            always_on_top: false,
            window_position: None,
            header_asset: None,
            supplemental_events: false,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "不支援的設定版本");
        ensure!(
            self.zoom.is_finite() && (0.75..=2.0).contains(&self.zoom),
            "設定縮放值無效"
        );
        ensure!(self.colors.iter().all(|c| *c <= 0xFFFFFF), "設定顏色無效");
        if let Some(position) = self.window_position {
            ensure!(position.iter().all(|v| v.is_finite()), "視窗座標無效");
        }
        Ok(())
    }
}

pub trait MemoRepository {
    fn load_memo(&self, date: NaiveDate) -> Result<String>;
    fn save_memo(&mut self, date: NaiveDate, body: &str) -> Result<()>;
}

pub struct Store {
    pub(crate) db: Connection,
    pub directory: PathBuf,
    _lock: File,
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory).context("無法建立使用者資料目錄；不會改存暫存目錄")?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("calendar.lock"))?;
        lock.try_lock_exclusive()
            .context("另一個 CalendarRust 已使用此資料目錄；請切回該視窗")?;
        for subdir in ["images", "backups", "logs"] {
            std::fs::create_dir_all(directory.join(subdir))?;
        }
        let mut db =
            Connection::open(directory.join("calendar.sqlite3")).context("無法開啟備忘資料庫")?;
        db.busy_timeout(Duration::from_secs(2))?;
        let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= SCHEMA,
            "資料庫由較新版程式建立；請使用新版，不會降版或清空資料"
        );
        if version == 0 {
            let count: u32 = db.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )?;
            ensure!(count == 0, "未辨識的既有資料庫，拒絕覆寫");
            let transaction = db.transaction()?;
            transaction.execute_batch(SCHEMA_SQL)?;
            transaction.execute_batch("PRAGMA user_version=1;")?;
            transaction.commit()?;
        }
        validate_schema(&db)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )?;
        Ok(Self {
            db,
            directory: directory.to_owned(),
            _lock: lock,
        })
    }

    pub fn settings(&self) -> Result<Settings> {
        let json: Option<String> = self
            .db
            .query_row("SELECT body FROM app_settings WHERE id=1", [], |r| r.get(0))
            .optional()?;
        let settings = match json {
            Some(json) => serde_json::from_str(&json).context("設定損壞，未套用靜默預設值")?,
            None => Settings::default(),
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        settings.validate()?;
        self.db.execute("INSERT INTO app_settings VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
            [serde_json::to_string(settings)?])?;
        Ok(())
    }

    pub fn asset_path(&self, id: &str) -> Result<PathBuf> {
        let (path, hash): (String, String) = self
            .db
            .query_row(
                "SELECT path,sha256 FROM image_asset WHERE asset_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .context("圖片資產記錄遺失")?;
        let relative = safe_relative(&path)?;
        ensure!(
            relative.components().count() == 2 && relative.starts_with("images"),
            "圖片資產路徑無效"
        );
        let full = self.directory.join(relative);
        ensure!(
            digest(&std::fs::read(&full).context("受管理圖片遺失")?) == hash,
            "受管理圖片雜湊不符"
        );
        Ok(full)
    }

    pub fn daily_asset(&self, date: NaiveDate) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT asset_id FROM daily_image WHERE date=?1",
                [date.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_daily_asset(&mut self, date: NaiveDate, asset: Option<&str>) -> Result<()> {
        match asset {
            Some(id) => {
                self.db.execute("INSERT INTO daily_image VALUES(?1,?2,datetime('now')) ON CONFLICT(date) DO UPDATE SET asset_id=excluded.asset_id,updated_at=excluded.updated_at",
                    params![date.to_string(), id])?;
            }
            None => {
                self.db
                    .execute("DELETE FROM daily_image WHERE date=?1", [date.to_string()])?;
            }
        }
        Ok(())
    }

    pub fn register_image(&mut self, png: &[u8], width: u32, height: u32) -> Result<String> {
        let id = digest(png);
        let filename = format!("{id}.png");
        let relative = format!("images/{filename}");
        let path = self.directory.join("images").join(filename);
        if !path.exists() {
            let mut temporary = tempfile::NamedTempFile::new_in(self.directory.join("images"))?;
            temporary.write_all(png)?;
            temporary.as_file().sync_all()?;
            temporary.persist_noclobber(&path)?;
        } else {
            ensure!(digest(&std::fs::read(&path)?) == id, "既有圖片資產損壞");
        }
        self.db.execute(
            "INSERT OR IGNORE INTO image_asset VALUES(?1,?2,?1,'PNG',?3,?4)",
            params![id, relative, width, height],
        )?;
        Ok(id)
    }

    pub fn cleanup_orphans(&mut self, header: Option<&str>) -> Result<usize> {
        let ids = {
            let mut stmt = self.db.prepare("SELECT asset_id,path FROM image_asset WHERE asset_id NOT IN (SELECT asset_id FROM daily_image)")?;
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut count = 0;
        for (id, path) in ids {
            if Some(id.as_str()) != header {
                let full = self.directory.join(safe_relative(&path)?);
                if full.is_file() {
                    std::fs::remove_file(full)?;
                }
                self.db
                    .execute("DELETE FROM image_asset WHERE asset_id=?1", [&id])?;
                count += 1;
            }
        }
        // Interrupted file-before-database commits leave only content-addressed PNGs.
        for entry in std::fs::read_dir(self.directory.join("images"))? {
            let entry = entry?;
            let filename = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = filename
                .strip_suffix(".png")
                .filter(|id| valid_asset_id(id))
            {
                let exists: bool = self.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM image_asset WHERE asset_id=?1)",
                    [id],
                    |r| r.get(0),
                )?;
                if !exists {
                    std::fs::remove_file(entry.path())?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    pub fn backup(&self, target: &Path, source_version: &str) -> Result<()> {
        ensure!(!target.exists(), "備份目的檔已存在，請使用不同檔名");
        let parent = target.parent().context("無效備份路徑")?;
        let staging = tempfile::tempdir_in(parent)?;
        let db_path = staging.path().join("calendar.sqlite3");
        self.db.backup("main", &db_path, None)?;
        let mut entries = BTreeMap::new();
        entries.insert("calendar.sqlite3".to_string(), db_path);
        let settings = self.settings()?;
        let mut ids = BTreeSet::new();
        let mut stmt = self
            .db
            .prepare("SELECT DISTINCT asset_id FROM daily_image")?;
        for id in stmt.query_map([], |r| r.get::<_, String>(0))? {
            ids.insert(id?);
        }
        if let Some(id) = settings.header_asset {
            ids.insert(id);
        }
        for id in ids {
            entries.insert(format!("images/{id}.png"), self.asset_path(&id)?);
        }
        let manifest = BackupManifest {
            version: 1,
            schema: SCHEMA,
            source_version: source_version.into(),
            files: entries
                .iter()
                .map(|(name, path)| Ok((name.clone(), crate::data::file_digest(path)?)))
                .collect::<Result<_>>()?,
        };
        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        {
            let mut zip = zip::ZipWriter::new(output.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("manifest.json", options)?;
            zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
            for (name, path) in entries {
                zip.start_file(name, options)?;
                std::io::copy(&mut File::open(path)?, &mut zip)?;
            }
            zip.finish()?;
        }
        output.as_file().sync_all()?;
        output.persist_noclobber(target)?;
        Ok(())
    }

    pub fn restore(&mut self, verified: VerifiedBackup, source_version: &str) -> Result<PathBuf> {
        let safety = self.automatic_backup("pre-restore", source_version)?;
        let restored = Connection::open(verified.directory.path().join("calendar.sqlite3"))?;
        validate_schema(&restored)?;
        let check: String = restored.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        ensure!(check == "ok", "備份資料庫損壞");
        for (name, hash) in &verified.manifest.files {
            if name.starts_with("images/") {
                let relative = safe_relative(name)?;
                let destination = self.directory.join(&relative);
                let bytes = std::fs::read(verified.directory.path().join(relative))?;
                ensure!(digest(&bytes) == *hash, "暫存備份圖片已變更");
                if destination.exists() {
                    ensure!(
                        digest(&std::fs::read(&destination)?) == *hash,
                        "同名圖片資產衝突"
                    );
                } else {
                    let mut temporary =
                        tempfile::NamedTempFile::new_in(self.directory.join("images"))?;
                    temporary.write_all(&bytes)?;
                    temporary.as_file().sync_all()?;
                    temporary.persist_noclobber(destination)?;
                }
            }
        }
        let backup = rusqlite::backup::Backup::new(&restored, &mut self.db)?;
        backup.run_to_completion(32, Duration::from_millis(5), None)?;
        drop(backup);
        Ok(safety)
    }

    pub fn automatic_backup(&self, label: &str, source_version: &str) -> Result<PathBuf> {
        let slot = tempfile::Builder::new()
            .prefix(label)
            .suffix(".zip")
            .tempfile_in(self.directory.join("backups"))?;
        let path = slot.path().to_path_buf();
        slot.close()?;
        self.backup(&path, source_version)?;
        Ok(path)
    }
}

impl MemoRepository for Store {
    fn load_memo(&self, date: NaiveDate) -> Result<String> {
        Ok(self
            .db
            .query_row(
                "SELECT body FROM memo WHERE date=?1",
                [date.to_string()],
                |r| r.get(0),
            )
            .optional()
            .context("備忘讀取失敗，不會以空內容覆寫")?
            .unwrap_or_default())
    }

    fn save_memo(&mut self, date: NaiveDate, body: &str) -> Result<()> {
        ensure!(body.len() <= MEMO_LIMIT, "備忘超過 256 KiB，未截斷或保存");
        let transaction = self.db.transaction()?;
        if body.is_empty() {
            transaction.execute("DELETE FROM memo WHERE date=?1", [date.to_string()])?;
        } else {
            transaction.execute("INSERT INTO memo VALUES(?1,?2,datetime('now')) ON CONFLICT(date) DO UPDATE SET body=excluded.body,updated_at=excluded.updated_at",
                params![date.to_string(), body])?;
        }
        transaction.commit().context("備忘交易提交失敗")?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub version: u32,
    pub schema: u32,
    pub source_version: String,
    pub files: BTreeMap<String, String>,
}

pub struct VerifiedBackup {
    directory: tempfile::TempDir,
    pub manifest: BackupManifest,
    pub memos: u32,
    pub images: usize,
}

pub fn safe_relative(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.contains('\\') && !name.contains(':') && !name.contains('\0'),
        "不安全的封裝路徑"
    );
    let mut path = PathBuf::new();
    for part in name.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "封裝路徑越界"
        );
        path.push(part);
    }
    ensure!(!path.is_absolute(), "封裝不可包含絕對路徑");
    Ok(path)
}

pub fn verify_backup(path: &Path) -> Result<VerifiedBackup> {
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    ensure!(archive.len() <= 60000, "備份檔案數超限");
    let manifest: BackupManifest = {
        let file = archive.by_name("manifest.json")?;
        ensure!(file.size() <= 8 * 1024 * 1024, "備份清單超限");
        serde_json::from_reader(file.take(8 * 1024 * 1024 + 1))?
    };
    ensure!(
        manifest.version == 1 && manifest.schema == SCHEMA,
        "不支援的備份／schema 版本"
    );
    ensure!(
        manifest.files.contains_key("calendar.sqlite3")
            && archive.len() == manifest.files.len() + 1,
        "備份清單缺漏"
    );
    let directory = tempfile::tempdir()?;
    let mut seen = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let name = file.name().to_string();
        safe_relative(&name)?;
        ensure!(seen.insert(name.clone()), "備份含重複路徑");
        if name == "manifest.json" {
            continue;
        }
        let expected = manifest
            .files
            .get(&name)
            .context("備份含未列入清單的檔案")?;
        ensure!(
            name == "calendar.sqlite3"
                || name
                    .strip_prefix("images/")
                    .and_then(|name| name.strip_suffix(".png"))
                    .is_some_and(valid_asset_id),
            "備份包含不支援的項目"
        );
        let max = if name == "calendar.sqlite3" {
            512 * 1024 * 1024
        } else {
            128 * 1024 * 1024
        };
        ensure!(file.size() <= max, "備份單檔超限");
        total = total.checked_add(file.size()).context("備份大小溢位")?;
        ensure!(total <= 2 * 1024 * 1024 * 1024, "備份總大小超限");
        let mut bytes = Vec::new();
        file.take(max + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= max && digest(&bytes) == *expected,
            "備份 SHA-256 不符"
        );
        let destination = directory.path().join(safe_relative(&name)?);
        std::fs::create_dir_all(destination.parent().context("無效備份位置")?)?;
        std::fs::write(destination, bytes)?;
    }
    let db = Connection::open_with_flags(
        directory.path().join("calendar.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    ensure!(version == SCHEMA, "備份 schema 與清單不符");
    validate_schema(&db)?;
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    ensure!(integrity == "ok", "備份資料庫損壞");
    let memos = db.query_row("SELECT COUNT(*) FROM memo", [], |r| r.get(0))?;
    let mut memo_rows = db.prepare("SELECT date,body FROM memo")?;
    for memo in memo_rows.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (key, body) = memo?;
        let date = NaiveDate::parse_from_str(&key, "%Y-%m-%d").context("備份備忘日期無效")?;
        ensure!(
            date.to_string() == key && body.len() <= MEMO_LIMIT,
            "備份日期鍵或正文大小無效"
        );
    }
    drop(memo_rows);
    let foreign_errors: bool = db.prepare("PRAGMA foreign_key_check")?.exists([])?;
    ensure!(!foreign_errors, "備份圖片外鍵引用損壞");
    let settings: Option<String> = db
        .query_row("SELECT body FROM app_settings WHERE id=1", [], |r| r.get(0))
        .optional()?;
    let header = if let Some(json) = settings {
        let settings: Settings = serde_json::from_str(&json)?;
        settings.validate()?;
        settings.header_asset
    } else {
        None
    };
    let mut references = BTreeSet::new();
    let mut stmt = db.prepare("SELECT DISTINCT asset_id FROM daily_image")?;
    for id in stmt.query_map([], |r| r.get::<_, String>(0))? {
        references.insert(id?);
    }
    if let Some(id) = header {
        references.insert(id);
    }
    for id in references {
        let (relative, sha): (String, String) = db.query_row(
            "SELECT path,sha256 FROM image_asset WHERE asset_id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            relative == format!("images/{id}.png") && manifest.files.get(&relative) == Some(&sha),
            "備份圖片引用缺漏或不安全"
        );
    }
    let images = manifest.files.len() - 1;
    drop(stmt);
    drop(db);
    Ok(VerifiedBackup {
        directory,
        manifest,
        memos,
        images,
    })
}

fn valid_asset_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_schema(db: &Connection) -> Result<()> {
    let expected = Connection::open_in_memory()?;
    expected.execute_batch(SCHEMA_SQL)?;
    let schema = |connection: &Connection| -> Result<Vec<(String, String, String)>> {
        let mut query=connection.prepare("SELECT type,name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name")?;
        Ok(query
            .query_map([], |row| {
                let sql: String = row.get(2)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    sql.split_whitespace().collect::<Vec<_>>().join(" "),
                ))
            })?
            .collect::<rusqlite::Result<_>>()?)
    };
    ensure!(
        schema(db)? == schema(&expected)?,
        "資料庫結構不符（含未知表、觸發器或檢視），拒絕開啟／還原"
    );
    Ok(())
}
