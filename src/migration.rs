use crate::{
    calendar,
    data::digest,
    media,
    storage::{MEMO_LIMIT, Store},
};
use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use encoding_rs::{BIG5, UTF_16BE, UTF_16LE};
use rusqlite::{OptionalExtension, params};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    Skip,
    Replace,
    Merge,
}

#[derive(Clone)]
pub struct DecodedText {
    pub encoding: String,
    pub body: String,
}

pub fn decode_candidates(bytes: &[u8]) -> Result<Vec<DecodedText>> {
    let mut result = Vec::new();
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        result.push(DecodedText {
            encoding: "UTF-8 BOM".into(),
            body: std::str::from_utf8(&bytes[3..])
                .context("UTF-8 BOM 內容無效")?
                .to_string(),
        });
    } else if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let encoding = if bytes[0] == 0xFF { UTF_16LE } else { UTF_16BE };
        let body = encoding
            .decode_without_bom_handling_and_without_replacement(&bytes[2..])
            .context("UTF-16 內容無效")?;
        result.push(DecodedText {
            encoding: encoding.name().into(),
            body: body.into_owned(),
        });
    } else {
        if let Ok(body) = std::str::from_utf8(bytes) {
            result.push(DecodedText {
                encoding: "UTF-8".into(),
                body: body.into(),
            });
        }
        if let Some(body) = BIG5.decode_without_bom_handling_and_without_replacement(bytes)
            && !result.iter().any(|candidate| candidate.body == body)
        {
            result.push(DecodedText {
                encoding: "Big5／CP950".into(),
                body: body.into_owned(),
            });
        }
    }
    ensure!(
        !result.is_empty(),
        "無法以嚴格 UTF-8／Big5 解碼；未使用替代字元"
    );
    for text in &mut result {
        text.body = text.body.replace("\r\n", "\n").replace('\r', "\n");
    }
    Ok(result)
}

pub fn date_candidates(stem: &str) -> Vec<NaiveDate> {
    if !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return Vec::new();
    }
    let mut result = Vec::new();
    if (5..=8).contains(&stem.len()) {
        let split = stem.len() - 4;
        let (Ok(year), Ok(month), Ok(day)) = (
            stem[..split].parse::<i32>(),
            stem[split..split + 2].parse::<u32>(),
            stem[split + 2..].parse::<u32>(),
        ) else {
            return result;
        };
        if stem.len() == 8
            && let Some(date) = NaiveDate::from_ymd_opt(year, month, day)
        {
            result.push(date);
        }
        if split <= 3
            && year >= 1
            && let Some(date) = NaiveDate::from_ymd_opt(year + 1911, month, day)
        {
            result.push(date);
        }
    }
    result
}

pub fn parse_mapping(line: &str) -> Result<(NaiveDate, &str)> {
    ensure!(
        line.len() >= 9 && line.is_char_boundary(8),
        "圖片映射不足八位日期及路徑"
    );
    let key = &line[..8];
    ensure!(
        key.bytes().all(|byte| byte.is_ascii_digit()),
        "圖片映射日期不是 YYYYMMDD"
    );
    let date = NaiveDate::parse_from_str(key, "%Y%m%d").context("圖片映射日期無效")?;
    let path = &line[8..];
    ensure!(!path.is_empty(), "圖片映射路徑為空");
    Ok((date, path))
}

pub enum ImportContent {
    Memo(Vec<DecodedText>),
    Image(PathBuf),
    Invalid,
}

pub struct ImportItem {
    pub name: String,
    pub dates: Vec<NaiveDate>,
    pub chosen_date: Option<usize>,
    pub encoding: Option<usize>,
    pub content: ImportContent,
    pub enabled: bool,
    pub conflict: Conflict,
    pub problem: Option<String>,
    fingerprint: String,
}

impl ImportItem {
    pub fn date(&self) -> Option<NaiveDate> {
        self.chosen_date
            .and_then(|index| self.dates.get(index).copied())
    }
    pub fn body(&self) -> Option<&str> {
        if let ImportContent::Memo(candidates) = &self.content {
            self.encoding
                .and_then(|index| candidates.get(index).map(|text| text.body.as_str()))
        } else {
            None
        }
    }
}

pub struct ImportPlan {
    pub items: Vec<ImportItem>,
    pub allow_partial: bool,
}

pub fn scan(
    directory: &Path,
    mapping: Option<&Path>,
    image_base: Option<&Path>,
    mapping_encoding: Option<&str>,
) -> Result<ImportPlan> {
    let mut items = Vec::new();
    let mut files = std::fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    files.sort_by_key(|entry| entry.file_name());
    for entry in files {
        let path = entry.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("");
        let dates = date_candidates(stem);
        let result = (|| -> Result<(Vec<DecodedText>, String)> {
            ensure!(
                entry.metadata()?.len() <= MEMO_LIMIT as u64 + 3,
                "備忘來源超過 256 KiB"
            );
            let bytes = std::fs::read(&path)?;
            Ok((decode_candidates(&bytes)?, digest(&bytes)))
        })();
        let (content, encoding, problem, fingerprint) = match result {
            Ok((texts, fingerprint)) => {
                let choice = (texts.len() == 1).then_some(0);
                (ImportContent::Memo(texts), choice, None, fingerprint)
            }
            Err(error) => (
                ImportContent::Invalid,
                None,
                Some(format!("{error:#}")),
                digest(name.as_bytes()),
            ),
        };
        items.push(ImportItem {
            name,
            chosen_date: (dates.len() == 1).then_some(0),
            dates,
            encoding,
            content,
            enabled: true,
            conflict: Conflict::Skip,
            problem,
            fingerprint,
        });
    }
    if let Some(mapping) = mapping {
        ensure!(
            std::fs::metadata(mapping)?.len() <= 8 * 1024 * 1024,
            "圖片映射檔案超限"
        );
        let bytes = std::fs::read(mapping)?;
        let candidates = decode_candidates(&bytes)?;
        let chosen = match mapping_encoding {
            Some(encoding) => candidates
                .iter()
                .find(|text| text.encoding.starts_with(encoding))
                .context("指定編碼無法嚴格解碼映射檔")?,
            None => {
                ensure!(
                    candidates.len() == 1,
                    "圖片映射有多種編碼候選；請在預覽選擇 UTF-8 或 Big5 後重新掃描"
                );
                &candidates[0]
            }
        };
        for (index, line) in chosen.body.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let fingerprint = digest(format!("{}:{index}:{line}", digest(&bytes)).as_bytes());
            let parsed = (|| -> Result<(NaiveDate, PathBuf)> {
                let (date, raw_path) = parse_mapping(line)?;
                let path = PathBuf::from(raw_path);
                let path = if path.is_absolute() {
                    path
                } else {
                    image_base
                        .context("相對圖片路徑須明確選擇舊 EXE／圖片基準目錄")?
                        .join(path)
                };
                ensure!(path.is_file(), "圖片遺失，請修復來源後重新掃描");
                Ok((date, path))
            })();
            let (dates, content, problem) = match parsed {
                Ok((date, path)) => (vec![date], ImportContent::Image(path), None),
                Err(error) => (
                    Vec::new(),
                    ImportContent::Invalid,
                    Some(format!("{error:#}")),
                ),
            };
            items.push(ImportItem {
                name: format!("pCalendar.dat 第 {} 行", index + 1),
                chosen_date: (!dates.is_empty()).then_some(0),
                dates,
                encoding: None,
                content,
                enabled: true,
                conflict: Conflict::Skip,
                problem,
                fingerprint,
            });
        }
    }
    ensure!(!items.is_empty(), "來源沒有可預覽的 .txt 或圖片映射項目");
    Ok(ImportPlan {
        items,
        allow_partial: false,
    })
}

pub fn apply(plan: &ImportPlan, store: &mut Store, source_version: &str) -> Result<Vec<String>> {
    let mut report = Vec::new();
    let mut memos = Vec::new();
    let mut images = Vec::new();
    let mut records = Vec::new();
    for item in &plan.items {
        if !item.enabled {
            report.push(format!("{}：未選取", item.name));
            continue;
        }
        let resolved = (|| -> Result<(NaiveDate, String)> {
            ensure!(
                item.problem.is_none(),
                "{}",
                item.problem.as_deref().unwrap_or("")
            );
            let date = item.date().context("日期候選尚未確認")?;
            let fingerprint =
                digest(format!("{}:{date}:{:?}", item.fingerprint, item.encoding).as_bytes());
            Ok((date, fingerprint))
        })();
        let (date, fingerprint) = match resolved {
            Ok(value) => value,
            Err(error) => {
                ensure!(
                    plan.allow_partial,
                    "{}：{error:#}；未核准部分匯入，未提交任何項目",
                    item.name
                );
                report.push(format!("{}：未匯入：{error:#}", item.name));
                continue;
            }
        };
        let seen: bool = store.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM import_record WHERE fingerprint=?1)",
            [&fingerprint],
            |r| r.get(0),
        )?;
        if seen {
            report.push(format!("{}：先前已匯入，略過", item.name));
            continue;
        }
        match &item.content {
            ImportContent::Memo(_) => {
                let body = match item.body() {
                    Some(body) => body,
                    None => {
                        ensure!(
                            plan.allow_partial,
                            "{}：編碼歧義尚未確認，未提交",
                            item.name
                        );
                        report.push(format!("{}：編碼未確認", item.name));
                        continue;
                    }
                };
                ensure!(body.len() <= MEMO_LIMIT, "{}：備忘超限", item.name);
                memos.push((item.name.clone(), date, body.to_string(), item.conflict));
            }
            ImportContent::Image(path) => match media::load(path) {
                Ok(image) => images.push((item.name.clone(), date, image, item.conflict)),
                Err(error) => {
                    ensure!(plan.allow_partial, "{}：{error:#}，未提交", item.name);
                    report.push(format!("{}：圖片無效：{error:#}", item.name));
                    continue;
                }
            },
            ImportContent::Invalid => unreachable!("invalid entries have problems"),
        }
        report.push(format!(
            "{} → {}{}（確認規則 {:?}；結果見下方）",
            item.name,
            date,
            if calendar::selectable(date) {
                ""
            } else {
                "：目前 UI 不可瀏覽，資料仍保留"
            },
            item.conflict
        ));
        records.push((
            fingerprint,
            format!("{date}；{:?}；編碼候選 {:?}", item.conflict, item.encoding),
        ));
    }
    ensure!(
        !records.is_empty(),
        "沒有可提交的新增項目；來源與現況未變更"
    );
    store.automatic_backup("pre-import", source_version)?;
    let mut image_records = Vec::new();
    for (name, date, image, conflict) in images {
        ensure!(
            conflict != Conflict::Merge,
            "圖片不可選擇合併；請選略過或取代"
        );
        let id = store.register_image(&image.png, image.width, image.height)?;
        image_records.push((name, date, id, conflict));
    }
    let transaction = store.db.transaction()?;
    for (name, date, body, conflict) in memos {
        let existing: Option<String> = transaction
            .query_row(
                "SELECT body FROM memo WHERE date=?1",
                [date.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let body = match (existing, conflict) {
            (Some(_), Conflict::Skip) => {
                report.push(format!("{name} → {date}：衝突，保留原備忘並略過來源"));
                continue;
            }
            (Some(old), Conflict::Merge) => {
                report.push(format!("{name} → {date}：已合併備忘"));
                format!("{old}\n{body}")
            }
            (Some(_), Conflict::Replace) => {
                report.push(format!("{name} → {date}：已取代備忘"));
                body
            }
            _ => {
                report.push(format!("{name} → {date}：已匯入新備忘"));
                body
            }
        };
        ensure!(
            body.len() <= MEMO_LIMIT,
            "合併備忘後超過 256 KiB，整批交易回滾"
        );
        if body.is_empty() {
            transaction.execute("DELETE FROM memo WHERE date=?1", [date.to_string()])?;
        } else {
            transaction.execute("INSERT INTO memo VALUES(?1,?2,datetime('now')) ON CONFLICT(date) DO UPDATE SET body=excluded.body,updated_at=excluded.updated_at",
                params![date.to_string(), body])?;
        }
    }
    for (name, date, id, conflict) in image_records {
        let query = if conflict == Conflict::Skip {
            "INSERT OR IGNORE INTO daily_image VALUES(?1,?2,datetime('now'))"
        } else {
            "INSERT INTO daily_image VALUES(?1,?2,datetime('now')) ON CONFLICT(date) DO UPDATE SET asset_id=excluded.asset_id,updated_at=excluded.updated_at"
        };
        let changed = transaction.execute(query, params![date.to_string(), id])?;
        report.push(format!(
            "{name} → {date}：{}",
            if changed == 0 {
                "圖片衝突，已保留原映射"
            } else {
                "圖片映射已匯入"
            }
        ));
    }
    for (fingerprint, summary) in records {
        transaction.execute(
            "INSERT INTO import_record VALUES(?1,?2,datetime('now'))",
            params![fingerprint, summary],
        )?;
    }
    transaction.commit()?;
    Ok(report)
}
