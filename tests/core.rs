use calendar_rust::{
    app::{SaveState, Session},
    calendar::{self, CalendarProvider},
    data::{LunarData, RawMonth},
    events::EventKind,
    storage::{MemoRepository, Store},
};
use chrono::{Datelike, Duration, NaiveDate};
use std::{collections::BTreeMap, path::PathBuf};

fn date(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}
fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("calendar-data")
}

#[test]
fn all_supported_month_grids_are_contiguous_and_complete() {
    let mut count = 0;
    for year in 1912..=2050 {
        for month in 1..=12 {
            let grid = calendar::grid_dates(year, month).unwrap();
            assert_eq!(grid.len(), 42);
            assert_eq!(grid[0].weekday().num_days_from_sunday(), 0);
            for pair in grid.windows(2) {
                assert_eq!((pair[1] - pair[0]).num_days(), 1);
            }
            let in_month = grid.iter().filter(|date| date.month() == month).count();
            assert_eq!(
                in_month,
                calendar::month_length(year, month).unwrap() as usize
            );
            count += in_month;
        }
    }
    assert_eq!(count, 50_770);
    assert!(!calendar::is_leap_year(1900));
    assert!(calendar::is_leap_year(2000));
    assert!(!calendar::is_leap_year(2100));
}

#[test]
fn october_grid_and_year_edges_match_contract() {
    let grid = calendar::grid_dates(2026, 10).unwrap();
    assert_eq!(grid[0], date("2026-09-27"));
    assert_eq!(grid[41], date("2026-11-07"));
    assert_eq!(
        (0..6)
            .map(|row| calendar::row_week(grid[row * 7]).1)
            .collect::<Vec<_>>(),
        vec![40, 41, 42, 43, 44, 45]
    );
    let grid = calendar::grid_dates(2050, 12).unwrap();
    assert_eq!(grid[0], date("2050-11-27"));
    assert_eq!(grid[41], date("2051-01-07"));
    assert!(!calendar::selectable(grid[41]));
    assert_eq!(
        calendar::navigate_month(date("2028-01-31"), 1).unwrap(),
        date("2028-02-29")
    );
    assert_eq!(
        calendar::navigate_month(date("2031-12-31"), 1).unwrap(),
        date("2032-01-31")
    );
    assert!(calendar::navigate_month(calendar::MIN, -1).is_err());
    assert!(calendar::navigate_month(calendar::MAX, 1).is_err());
    assert_eq!(calendar::row_week(date("2021-01-01")), (2020, 53));
}

#[test]
fn packaged_source_fidelity_and_audited_solar_term_correction() {
    let mut source = LunarData::open(&data_dir()).expect("請先依 README 建置離線資料包");
    assert_eq!(source.manifest.full_snapshot_invalid_solar_terms, 556);
    assert_eq!(source.manifest.days, 29_616);
    let raw = source.day(date("2026-10-14")).unwrap();
    assert_eq!(
        (raw.lunar.year, raw.lunar.month, raw.lunar.day),
        (2026, 9, 5)
    );
    assert_eq!(raw.day_ganzhi.full, "辛酉");
    assert_eq!(raw.day_ganzhi.nayin.as_deref(), Some("石榴木"));
    assert_eq!(raw.zodiac_clash, "兔");
    raw.validate_hours().unwrap();
    assert_eq!(raw.auspicious_hours, ["子", "寅", "卯", "午", "未", "酉"]);
    let invalid = source.day(date("2026-01-01")).unwrap();
    assert!(invalid.solar_term.validate(invalid.gregorian).is_err());
    let mut provider = CalendarProvider::open(&data_dir()).unwrap();
    let day = provider.info(date("2026-10-14"));
    assert_eq!(day.short_label(), "初五");
    assert!(
        !day.events
            .iter()
            .any(|event| event.kind == EventKind::SolarTerm)
    );
    for (input, name) in [
        ("2026-10-08", "寒露"),
        ("2026-10-23", "霜降"),
        ("2026-10-10", "國慶紀念日"),
    ] {
        assert!(
            provider
                .info(date(input))
                .events
                .iter()
                .any(|event| event.name == name)
        );
    }
    let invalid = provider.info(date("2026-01-01"));
    assert!(invalid.lunar.is_ok());
    let adopted = invalid.solar_term.unwrap().unwrap();
    assert_eq!(adopted.start_date, date("2025-12-21"));
    assert_eq!(adopted.end_date, date("2026-01-05"));
    assert!(matches!(
        invalid.almanac,
        calendar::AlmanacAvailability::Available(_)
    ));
    for (input, year, zodiac) in [("2026-02-16", "乙巳", "蛇"), ("2026-02-17", "丙午", "馬")]
    {
        let info = provider.info(date(input));
        let lunar = info.lunar.unwrap();
        assert_eq!(lunar.year_ganzhi, year);
        assert_eq!(lunar.zodiac, zodiac);
    }
    assert!(matches!(
        provider.info(date("1969-12-31")).almanac,
        calendar::AlmanacAvailability::OutOfRange
    ));
    assert!(provider.info(calendar::MIN).lunar.is_ok());
    assert!(provider.info(date("1970-01-01")).lunar.unwrap().year < 1970);
    assert!(
        provider
            .info(date("2026-02-16"))
            .events
            .iter()
            .any(|event| event.name == "除夕")
    );
}

#[test]
fn adapter_rejects_missing_duplicate_or_mismatched_data() {
    let source = data_dir();
    let db = rusqlite::Connection::open_with_flags(
        source.join("lunar.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let raw: Vec<u8> = db
        .query_row(
            "SELECT raw FROM month WHERE year=2026 AND month=10",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut decoder = flate2::read::GzDecoder::new(raw.as_slice());
    let mut json = String::new();
    std::io::Read::read_to_string(&mut decoder, &mut json).unwrap();
    let mut month: RawMonth = serde_json::from_str(&json).unwrap();
    month.validate(2026, 10).unwrap();
    month.days[0].lunar.zodiac = "未知".into();
    assert!(month.validate(2026, 10).is_err());
    month.days[0].lunar.zodiac = "馬".into();
    month.days[1].gregorian = month.days[0].gregorian;
    assert!(month.validate(2026, 10).is_err());
    month.days.remove(0);
    assert!(month.validate(2026, 10).is_err());
}

#[derive(Default)]
struct FakeRepository {
    memos: BTreeMap<NaiveDate, String>,
    fail_save: bool,
    fail_load: bool,
}
impl MemoRepository for FakeRepository {
    fn load_memo(&self, date: NaiveDate) -> anyhow::Result<String> {
        anyhow::ensure!(!self.fail_load, "injected read failure");
        Ok(self.memos.get(&date).cloned().unwrap_or_default())
    }
    fn save_memo(&mut self, date: NaiveDate, body: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!self.fail_save, "injected write failure");
        self.memos.insert(date, body.into());
        Ok(())
    }
}

#[test]
fn failed_saves_block_all_date_and_mode_changes_without_mixing_drafts() {
    let today = date("2026-10-14");
    let mut session = Session::new(FakeRepository::default(), today).unwrap();
    session.memo.edit("繁體中文 😀\nA 日".into()).unwrap();
    session.repository.fail_save = true;
    assert!(session.select(date("2026-10-10")).is_err());
    assert!(session.navigate(1).is_err());
    assert!(session.set_month(2027, 2).is_err());
    assert!(session.set_mode(calendar_rust::app::ViewMode::Day).is_err());
    assert_eq!(session.selected, today);
    assert_eq!(session.memo.date, today);
    assert!(session.memo.dirty());
    assert!(matches!(session.memo.state, SaveState::Failed(_)));
    session.repository.fail_save = false;
    session.select(date("2026-10-10")).unwrap();
    session.memo.edit("B 日".into()).unwrap();
    session.select(today).unwrap();
    assert_eq!(session.memo.body, "繁體中文 😀\nA 日");
    session.repository.fail_load = true;
    assert!(session.select(date("2026-10-11")).is_err());
    assert_eq!(session.selected, today);
    assert_eq!(session.memo.body, "繁體中文 😀\nA 日");
}

#[test]
fn today_following_and_out_of_range_today_preserve_true_date() {
    let today = date("2026-10-14");
    let mut session = Session::new(FakeRepository::default(), today).unwrap();
    session.select(date("2026-10-10")).unwrap();
    session.update_today(today + Duration::days(1)).unwrap();
    assert_eq!(session.selected, date("2026-10-10"));
    assert_eq!(session.today, date("2026-10-15"));
    session.go_today().unwrap();
    session.update_today(date("2026-10-16")).unwrap();
    assert_eq!(session.selected, date("2026-10-16"));
    let session = Session::new(FakeRepository::default(), date("2051-01-01")).unwrap();
    assert_eq!(session.today, date("2051-01-01"));
    assert_eq!(session.selected, calendar::MAX);
    assert!(!session.follow_today);
    assert!(session.notice.is_some());
}

#[test]
fn sqlite_memo_unicode_clear_whitespace_and_reopen_are_persistent() {
    let directory = tempfile::tempdir().unwrap();
    let day = date("2026-10-14");
    {
        let mut store = Store::open(directory.path()).unwrap();
        store
            .save_memo(day, "繁體中文 emoji 😀\nsecond line")
            .unwrap();
        assert!(
            Store::open(directory.path()).is_err(),
            "second writer must be excluded"
        );
    }
    {
        let mut store = Store::open(directory.path()).unwrap();
        assert_eq!(
            store.load_memo(day).unwrap(),
            "繁體中文 emoji 😀\nsecond line"
        );
        store.save_memo(day, " ").unwrap();
        assert_eq!(store.load_memo(day).unwrap(), " ");
        store.save_memo(day, "").unwrap();
    }
    let store = Store::open(directory.path()).unwrap();
    assert_eq!(store.load_memo(day).unwrap(), "");
}

#[test]
fn approved_historical_corrections_and_optional_events_are_traceable() {
    let mut provider = CalendarProvider::open(&data_dir()).unwrap();
    let lunar = provider.info(date("1925-05-22")).lunar.unwrap();
    assert_eq!(
        (lunar.year, lunar.month, lunar.day, lunar.is_leap_month),
        (1925, 4, 1, true)
    );
    let corrections: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(data_dir().join("corrections.json")).unwrap())
            .unwrap();
    assert_eq!(
        corrections
            .iter()
            .filter(|row| row["field"] == "lunar")
            .count(),
        30
    );
    assert!(corrections.iter().any(|row| row["date"] == "2026-01-01"
        && row["raw"]["startDate"] == "2026-12-22"
        && row["adopted"]["startDate"] == "2025-12-21"));
    calendar_rust::data::verify_package(&data_dir()).unwrap();
    assert!(
        !provider
            .info(date("2026-01-04"))
            .events
            .iter()
            .any(|event| event.name == "世界點字日")
    );
    provider.supplemental = true;
    assert!(
        provider
            .info(date("2026-01-04"))
            .events
            .iter()
            .any(|event| event.name == "世界點字日")
    );
    let january = provider.info(date("2026-01-01"));
    assert_eq!(
        january
            .events
            .iter()
            .filter(|event| calendar_rust::events::canonical_name(&event.name) == "開國紀念日")
            .count(),
        1
    );
}

#[test]
fn ghost_gate_close_uses_the_real_month_end_without_day_zero_or_leap_repeats() {
    let lunar = calendar::lunar_from_parts(2026, 7, 29, false).unwrap();
    let next = calendar::lunar_from_parts(2026, 8, 1, false).unwrap();
    let events = calendar_rust::events::resolve(date("2026-09-10"), &lunar, Some(&next));
    assert!(events.iter().any(|event| event.name == "關鬼門"));
    let leap = calendar::lunar_from_parts(2026, 7, 29, true).unwrap();
    assert!(
        !calendar_rust::events::resolve(date("2026-09-10"), &leap, Some(&next))
            .iter()
            .any(|event| event.name == "關鬼門")
    );
    assert!(calendar::lunar_from_parts(2026, 7, 0, false).is_err());
}
