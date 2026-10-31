use calendar_rust::{
    calendar::CalendarProvider,
    export::{self, ExportOptions},
    storage::DEFAULT_COLORS,
};
use std::{
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

fn options(year: i32, target: PathBuf) -> (PathBuf, ExportOptions) {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("calendar-data");
    let provider = CalendarProvider::open(&data).unwrap();
    let options = ExportOptions {
        year,
        colors: DEFAULT_COLORS,
        header_png: None,
        target,
        overwrite: false,
        supplemental: false,
        source_snapshot: provider.lunar_data.manifest.adopted_sha256.clone(),
    };
    (data, options)
}

fn entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> String {
    let mut value = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut value)
        .unwrap();
    value
}

#[test]
fn xlsx_contents_include_all_dates_sources_styles_print_setup_and_no_external_links() {
    let dir = tempfile::tempdir().unwrap();
    for (year, count) in [(2026, 365), (2028, 366), (2050, 365)] {
        let (data, options) = options(year, dir.path().join(format!("{year}.xlsx")));
        let (sender, _) = mpsc::channel();
        assert!(export::write(&data, &options, &AtomicBool::new(false), &sender).unwrap());
        let mut archive =
            zip::ZipArchive::new(std::fs::File::open(&options.target).unwrap()).unwrap();
        let workbook = entry(&mut archive, "xl/workbook.xml");
        assert!(workbook.contains("年曆"));
        assert!(workbook.contains("資料來源"));
        let sheet = entry(&mut archive, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("orientation=\"landscape\""));
        assert!(sheet.contains("fitToPage=\"1\""));
        // OOXML defaults both dimensions to one; the writer omits default attributes.
        assert!(!sheet.contains("fitToWidth="));
        assert!(!sheet.contains("fitToHeight="));
        assert!(sheet.contains("節日／紀念日不等於現行法定休假"));
        let sources = entry(&mut archive, "xl/worksheets/sheet2.xml");
        assert_eq!(sources.matches("<row ").count(), count + 13);
        let strings = entry(&mut archive, "xl/sharedStrings.xml");
        assert!(strings.contains(&format!("{year}-01-01")));
        assert!(strings.contains(&format!("{year}-12-31")));
        if year == 2028 {
            assert!(strings.contains("2028-02-29"));
        }
        for i in 0..archive.len() {
            let name = archive.by_index(i).unwrap().name().to_string();
            assert!(!name.contains("externalLink") && !name.contains("vbaProject"));
        }
    }
}

#[test]
fn cancel_and_failed_export_preserve_original_destination() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("existing.xlsx");
    std::fs::write(&target, b"original").unwrap();
    let (data, mut options) = options(2026, target.clone());
    let cancel = AtomicBool::new(false);
    let (sender, _) = mpsc::channel();
    assert!(export::write(&data, &options, &cancel, &sender).is_err());
    options.overwrite = true;
    cancel.store(true, Ordering::Relaxed);
    assert!(!export::write(&data, &options, &cancel, &sender).unwrap());
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
    cancel.store(false, Ordering::Relaxed);
    options.header_png = Some(b"not an image".to_vec());
    assert!(export::write(&data, &options, &cancel, &sender).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
}

#[test]
fn xlsx_embeds_header_and_preserves_all_seven_custom_colors() {
    let directory = tempfile::tempdir().unwrap();
    let (data, mut options) = options(2028, directory.path().join("抬頭與七色.xlsx"));
    options.colors = [
        0x123456, 0x234567, 0x345678, 0x456789, 0x56789A, 0x6789AB, 0x789ABC,
    ];
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        32,
        16,
        image::Rgba([15, 90, 80, 255]),
    ))
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    options.header_png = Some(png.into_inner());
    let (sender, _) = mpsc::channel();
    assert!(export::write(&data, &options, &AtomicBool::new(false), &sender).unwrap());
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&options.target).unwrap()).unwrap();
    let mut embedded = Vec::new();
    archive
        .by_name("xl/media/image1.png")
        .unwrap()
        .read_to_end(&mut embedded)
        .unwrap();
    assert_eq!(Some(embedded), options.header_png);
    let styles = entry(&mut archive, "xl/styles.xml");
    for color in options.colors {
        assert!(styles.contains(&format!("FF{color:06X}")));
    }
    let drawing = entry(&mut archive, "xl/drawings/drawing1.xml");
    assert!(drawing.contains("年曆抬頭圖片"));
    let strings = entry(&mut archive, "xl/sharedStrings.xml");
    let mut day = chrono::NaiveDate::from_ymd_opt(2028, 1, 1).unwrap();
    while chrono::Datelike::year(&day) == 2028 {
        assert!(strings.contains(&day.to_string()), "{day}");
        day += chrono::Duration::days(1);
    }
}
