use calendar_rust::{
    app::Session,
    migration::{self, Conflict},
    storage::{self, DateHighlight, MarkerPosition, MemoRepository, Settings, Store},
};
use chrono::NaiveDate;
use std::{
    io::{Cursor, Write},
    path::Path,
};

fn date(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}
fn png() -> Vec<u8> {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        32,
        16,
        image::Rgba([10, 180, 90, 128]),
    ));
    let mut buffer = Cursor::new(Vec::new());
    image
        .write_to(&mut buffer, image::ImageFormat::Png)
        .unwrap();
    buffer.into_inner()
}

#[test]
fn backup_uses_live_wal_snapshot_and_restores_memos_images_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("user")).unwrap();
    let day = date("2026-10-14");
    store.save_memo(day, "備份😀").unwrap();
    let id = store.register_image(&png(), 32, 16).unwrap();
    store.set_daily_asset(day, Some(&id)).unwrap();
    let settings = Settings {
        header_asset: Some(id.clone()),
        today_highlight: DateHighlight {
            background: 0x123456,
            marker_position: MarkerPosition::TopRight,
        },
        memo_highlight: DateHighlight {
            background: 0xABCDEF,
            marker_position: MarkerPosition::TopLeft,
        },
        ..Settings::default()
    };
    store.save_settings(&settings).unwrap();
    let path = dir.path().join("backup.zip");
    store.backup(&path, "source-A").unwrap();
    let verified = storage::verify_backup(&path).unwrap();
    assert_eq!(verified.memos, 1);
    assert_eq!(verified.images, 1);
    store.save_memo(day, "新內容").unwrap();
    store.set_daily_asset(day, None).unwrap();
    store.save_settings(&Settings::default()).unwrap();
    let safety = store.restore(verified, "source-B").unwrap();
    assert!(safety.is_file());
    assert_eq!(store.load_memo(day).unwrap(), "備份😀");
    assert_eq!(store.daily_asset(day).unwrap(), Some(id.clone()));
    assert_eq!(store.settings().unwrap().header_asset, Some(id.clone()));
    assert_eq!(
        store.settings().unwrap().today_highlight,
        settings.today_highlight
    );
    assert_eq!(
        store.settings().unwrap().memo_highlight,
        settings.memo_highlight
    );
    assert_eq!(
        std::fs::read(store.asset_path(&id).unwrap()).unwrap(),
        png()
    );
    let before = storage::verify_backup(&safety).unwrap();
    assert_eq!(before.manifest.source_version, "source-B");
}

#[test]
fn legacy_settings_keep_existing_values_and_default_new_date_highlights() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let json = serde_json::json!({
        "version": 1,
        "display": "Gregorian",
        "colors": storage::DEFAULT_COLORS,
        "zoom": 1.25,
        "always_on_top": true,
        "window_position": [10.0, 20.0],
        "header_asset": null,
        "supplemental_events": true
    });
    let db = rusqlite::Connection::open(directory.path().join("calendar.sqlite3")).unwrap();
    db.execute("INSERT INTO app_settings VALUES(1,?1)", [json.to_string()])
        .unwrap();
    let settings = store.settings().unwrap();
    assert_eq!(settings.display, storage::CalendarDisplay::Gregorian);
    assert_eq!(settings.zoom, 1.25);
    assert!(settings.always_on_top && settings.supplemental_events);
    assert_eq!(settings.window_position, Some([10.0, 20.0]));
    assert_eq!(settings.today_highlight.background, 0xE5EDF1);
    assert_eq!(
        settings.today_highlight.marker_position,
        MarkerPosition::TopLeft
    );
    assert_eq!(settings.memo_highlight.background, 0xFFF2CC);
    assert_eq!(
        settings.memo_highlight.marker_position,
        MarkerPosition::TopRight
    );
}

#[test]
fn date_highlights_persist_and_invalid_values_are_not_replaced_with_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let settings = Settings {
        today_highlight: DateHighlight {
            background: 0xFFFFFF,
            marker_position: MarkerPosition::TopRight,
        },
        memo_highlight: DateHighlight {
            background: 0,
            marker_position: MarkerPosition::TopLeft,
        },
        ..Settings::default()
    };
    {
        let mut store = Store::open(directory.path()).unwrap();
        store.save_settings(&settings).unwrap();
    }
    let mut store = Store::open(directory.path()).unwrap();
    assert_eq!(
        store.settings().unwrap().today_highlight,
        settings.today_highlight
    );
    assert_eq!(
        store.settings().unwrap().memo_highlight,
        settings.memo_highlight
    );
    for today in [false, true] {
        let mut invalid = settings.clone();
        if today {
            invalid.today_highlight.background = 0x1000000;
        } else {
            invalid.memo_highlight.background = 0x1000000;
        }
        assert!(store.save_settings(&invalid).is_err());
        assert_eq!(
            store.settings().unwrap().today_highlight,
            settings.today_highlight
        );
        assert_eq!(
            store.settings().unwrap().memo_highlight,
            settings.memo_highlight
        );
    }
    let db = rusqlite::Connection::open(directory.path().join("calendar.sqlite3")).unwrap();
    for invalid in [
        serde_json::json!({"background": 0x123456, "marker_position": "BottomLeft"}),
        serde_json::json!({"background": 0x1000000, "marker_position": "TopRight"}),
        serde_json::json!({"marker_position": "TopRight"}),
    ] {
        let mut json = serde_json::to_value(&settings).unwrap();
        json["today_highlight"] = invalid;
        let body = json.to_string();
        db.execute("UPDATE app_settings SET body=?1 WHERE id=1", [&body])
            .unwrap();
        assert!(store.settings().is_err());
        assert_eq!(
            db.query_row("SELECT body FROM app_settings WHERE id=1", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
            body
        );
    }
}

#[test]
fn memo_markers_refresh_after_import_and_full_restore() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("user")).unwrap();
    let today = date("2026-10-14");
    let previous = date("2026-09-30");
    let imported = date("2026-10-10");
    store.save_memo(previous, "原有備忘").unwrap();
    let backup = dir.path().join("backup.zip");
    store.backup(&backup, "source").unwrap();
    let mut session = Session::new(store, today).unwrap();
    assert!(session.has_memo(previous));
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("20261010.txt"), "匯入備忘😀").unwrap();
    let plan = migration::scan(&source, None, None, None).unwrap();
    migration::apply(&plan, &mut session.repository, "source").unwrap();
    session.reload_memo().unwrap();
    assert!(session.has_memo(previous));
    assert!(session.has_memo(imported));
    session.memo.edit("還原前新增".into()).unwrap();
    session.flush().unwrap();
    assert!(session.has_memo(today));
    session
        .repository
        .restore(storage::verify_backup(&backup).unwrap(), "source")
        .unwrap();
    session.reload_memo().unwrap();
    assert!(session.has_memo(previous));
    assert!(!session.has_memo(imported));
    assert!(!session.has_memo(today));
}

#[test]
fn invalid_manifest_hash_and_traversal_do_not_touch_existing_data() {
    for name in [
        "../escape",
        "/absolute",
        "C:/escape",
        "images\\evil",
        "images/../evil",
        "a//b",
        "./a",
    ] {
        assert!(storage::safe_relative(name).is_err(), "{name}");
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(
        br#"{"version":1,"schema":1,"source_version":"test","files":{"../escape":"bad"}}"#,
    )
    .unwrap();
    zip.start_file("../escape", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"bad").unwrap();
    zip.finish().unwrap();
    assert!(storage::verify_backup(&path).is_err());
    assert!(!dir.path().join("escape").exists());
}

#[test]
fn future_schema_is_rejected_without_resetting_existing_data() {
    let dir = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(dir.path().join("calendar.sqlite3")).unwrap();
    db.execute_batch("PRAGMA user_version=99; CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('keep');").unwrap();
    drop(db);
    assert!(Store::open(dir.path()).is_err());
    let db = rusqlite::Connection::open(dir.path().join("calendar.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT value FROM sentinel", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
}

#[test]
fn strict_decoding_and_date_candidates_never_guess_invalid_keys() {
    assert_eq!(
        migration::date_candidates("1151014"),
        vec![date("2026-10-14")]
    );
    assert_eq!(migration::date_candidates("1101"), vec![]);
    assert_eq!(
        migration::date_candidates("20261014"),
        vec![date("2026-10-14")]
    );
    assert!(migration::date_candidates("20260230").is_empty());
    assert!(migration::date_candidates("今天").is_empty());
    let text = "備忘\r\n第二行";
    let (bytes, _, failed) = encoding_rs::BIG5.encode(text);
    assert!(!failed);
    assert!(
        migration::decode_candidates(&bytes)
            .unwrap()
            .iter()
            .any(|text| text.body == "備忘\n第二行")
    );
    assert!(migration::decode_candidates(&[0xFF]).is_err());
    let (_, path) = migration::parse_mapping("20261014C:\\中文 路徑\\my image.png").unwrap();
    assert_eq!(path, "C:\\中文 路徑\\my image.png");
    assert!(migration::parse_mapping("20260230bad.png").is_err());
}

#[test]
fn import_preview_conflicts_partial_confirmation_and_idempotence() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("1151014.txt"), b"\xEF\xBB\xBFnew memo").unwrap();
    std::fs::write(source.join("invalid.txt"), b"cannot infer date").unwrap();
    let mut store = Store::open(&dir.path().join("user")).unwrap();
    store.save_memo(date("2026-10-14"), "old").unwrap();
    let mut plan = migration::scan(&source, None, None, None).unwrap();
    assert!(migration::apply(&plan, &mut store, "source").is_err());
    assert_eq!(store.load_memo(date("2026-10-14")).unwrap(), "old");
    plan.allow_partial = true;
    plan.items[0].conflict = Conflict::Merge;
    let report = migration::apply(&plan, &mut store, "source").unwrap();
    assert!(report.iter().any(|line| line.contains("未匯入")));
    assert_eq!(
        store.load_memo(date("2026-10-14")).unwrap(),
        "old\nnew memo"
    );
    assert!(migration::apply(&plan, &mut store, "source").is_err());
    assert_eq!(
        store.load_memo(date("2026-10-14")).unwrap(),
        "old\nnew memo"
    );
    assert_eq!(
        std::fs::read(source.join("1151014.txt")).unwrap(),
        b"\xEF\xBB\xBFnew memo"
    );
}

#[test]
fn media_size_boundaries_are_enforced_and_source_is_untouched() {
    use calendar_rust::media;
    media::validate_dimensions(8000, 4000).unwrap();
    assert!(media::validate_dimensions(8000, 4001).is_err());
    assert!(media::validate_dimensions(8193, 1).is_err());
    assert!(media::validate_dimensions(0, 1).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("來源 圖.png");
    std::fs::write(&path, png()).unwrap();
    let result = media::load(&path).unwrap();
    assert_eq!((result.width, result.height), (32, 16));
    assert!(Path::new(&path).is_file());
    std::fs::write(dir.path().join("bad.png"), b"bad").unwrap();
    assert!(media::load(&dir.path().join("bad.png")).is_err());
}

#[test]
fn valid_zip_with_wrong_content_hash_is_rejected_before_restore() {
    use std::io::Read;
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(&directory.path().join("user")).unwrap();
    store.save_memo(date("2026-10-14"), "unchanged").unwrap();
    let original = directory.path().join("original.zip");
    store.backup(&original, "source").unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(original).unwrap()).unwrap();
    let invalid = directory.path().join("wrong-hash.zip");
    let mut output = zip::ZipWriter::new(std::fs::File::create(&invalid).unwrap());
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if file.name() == "calendar.sqlite3" {
            *bytes.last_mut().unwrap() ^= 1;
        }
        output
            .start_file(file.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap();
    assert!(
        storage::verify_backup(&invalid)
            .err()
            .unwrap()
            .to_string()
            .contains("SHA-256")
    );
    assert_eq!(store.load_memo(date("2026-10-14")).unwrap(), "unchanged");
}

#[test]
fn unexpected_schema_objects_are_rejected_without_running_them() {
    let directory = tempfile::tempdir().unwrap();
    {
        let mut store = Store::open(directory.path()).unwrap();
        store.save_memo(date("2026-10-14"), "keep").unwrap();
    }
    let db = rusqlite::Connection::open(directory.path().join("calendar.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER unexpected AFTER INSERT ON memo BEGIN SELECT 1; END;")
        .unwrap();
    drop(db);
    assert!(Store::open(directory.path()).is_err());
    let db = rusqlite::Connection::open(directory.path().join("calendar.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT body FROM memo", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
}
