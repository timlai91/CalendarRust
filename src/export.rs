use crate::{
    calendar::{CalendarProvider, DateInfo, WEEKDAYS, row_week},
    events::{self, EventKind},
};
use anyhow::{Context, Result, ensure};
use chrono::Datelike;
use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Image, Workbook};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

pub const COLOR_NAMES: [&str; 7] = [
    "星期六、日",
    "歷史節日（非休假表）",
    "節氣／特定節日",
    "補假（只示範、不推算）",
    "內框線",
    "外框線",
    "背景",
];

pub fn day_color_index(day: &DateInfo, year: i32, month: u32) -> usize {
    if day.date.month() != month || day.date.year() != year {
        return 6;
    }
    if day
        .events
        .iter()
        .any(|event| matches!(event.kind, EventKind::Memorial | EventKind::LunarFestival))
    {
        1
    } else if day
        .events
        .iter()
        .any(|event| event.kind == EventKind::SolarTerm)
    {
        2
    } else if matches!(day.date.weekday().num_days_from_sunday(), 0 | 6) {
        0
    } else {
        6
    }
}

#[derive(Clone)]
pub struct ExportOptions {
    pub year: i32,
    pub colors: [u32; 7],
    pub header_png: Option<Vec<u8>>,
    pub target: PathBuf,
    pub overwrite: bool,
    pub supplemental: bool,
    pub source_snapshot: String,
}

pub enum Progress {
    Month(u32),
    Complete(PathBuf),
    Cancelled,
    Failed(String),
}

pub struct ExportJob {
    pub receiver: Receiver<Progress>,
    pub cancel: Arc<AtomicBool>,
    pub handle: Option<JoinHandle<()>>,
    pub month: u32,
}

impl ExportJob {
    pub fn start(data: PathBuf, options: ExportOptions) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let handle = thread::Builder::new()
            .name("calendar-xlsx".into())
            .spawn(move || {
                let result = write(&data, &options, &worker_cancel, &sender);
                let message = match result {
                    Ok(true) => Progress::Complete(options.target),
                    Ok(false) => Progress::Cancelled,
                    Err(error) => Progress::Failed(format!("年曆匯出失敗：{error:#}")),
                };
                let _ = sender.send(message);
            })?;
        Ok(Self {
            receiver,
            cancel,
            handle: Some(handle),
            month: 0,
        })
    }
}

pub fn write(
    data: &Path,
    options: &ExportOptions,
    cancel: &AtomicBool,
    progress: &Sender<Progress>,
) -> Result<bool> {
    ensure!(
        (1912..=2050).contains(&options.year),
        "匯出年份超出 1912–2050"
    );
    ensure!(
        options.colors.iter().all(|color| *color <= 0xFFFFFF),
        "匯出色碼無效"
    );
    ensure!(
        options.overwrite || !options.target.exists(),
        "目的檔已存在，未核准覆寫"
    );
    let mut provider = CalendarProvider::open(data)?;
    ensure!(
        provider.lunar_data.manifest.adopted_sha256 == options.source_snapshot,
        "來源快照已改變，請重新建立匯出工作"
    );
    provider.supplemental = options.supplemental;
    let snapshot = (1..=12)
        .map(|month| provider.month(options.year, month))
        .collect::<Result<Vec<_>>>()?;
    for grid in &snapshot {
        for day in &grid.days {
            ensure!(
                day.lunar.is_ok() && day.solar_term.is_ok() && day.event_error.is_none(),
                "{} 日期／事件來源不可用；拒絕輸出錯誤年曆",
                day.date
            );
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let mut workbook = Workbook::new();
    let general = Format::new()
        .set_font_name("Microsoft JhengHei")
        .set_font_size(8)
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_background_color(Color::RGB(options.colors[6]))
        .set_border(FormatBorder::Thin)
        .set_border_color(Color::RGB(options.colors[4]));
    let title = general
        .clone()
        .set_bold()
        .set_font_size(12)
        .set_border_color(Color::RGB(options.colors[5]));
    let plain = Format::new()
        .set_font_name("Microsoft JhengHei")
        .set_font_size(9)
        .set_text_wrap();
    let sheet = workbook.add_worksheet();
    sheet.set_name("年曆")?;
    sheet
        .set_landscape()
        .set_paper_size(9)
        .set_print_fit_to_pages(1, 1)
        .set_print_center_horizontally(true);
    sheet.set_margins(0.2, 0.2, 0.2, 0.2, 0.1, 0.1);
    sheet
        .set_header("&CCalendarRust")
        .set_footer("&C節日／紀念日不等於現行法定休假  &R&P");
    sheet.merge_range(
        0,
        0,
        0,
        25,
        &format!(
            "CalendarRust　西元 {} 年／民國 {} 年",
            options.year,
            options.year - 1911
        ),
        &title,
    )?;
    sheet.merge_range(
        1,
        0,
        1,
        25,
        "農曆歲次／生肖於正月初一換年；採核准 HKO UTC+08:00 曆表校正；不是政府辦公日曆。",
        &plain,
    )?;
    let offset = if let Some(png) = &options.header_png {
        let image = Image::new_from_buffer(png)?;
        let scale = (325.0 / image.width()).min(70.0 / image.height());
        let image = image
            .set_scale_width(scale)
            .set_scale_height(scale)
            .set_alt_text("年曆抬頭圖片");
        sheet.insert_image(2, 8, &image)?;
        for row in 2..7 {
            sheet.set_row_height(row, 12)?;
        }
        7
    } else {
        3
    };
    for block in 0..3_u16 {
        let start = block * 9;
        sheet.set_column_width(start, 3)?;
        for column in start + 1..=start + 7 {
            sheet.set_column_width(column, 6)?;
        }
        if block < 2 {
            sheet.set_column_width(start + 8, 1.5)?;
        }
    }
    let mut valid = Vec::new();
    for grid in &snapshot {
        if cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let index = grid.month - 1;
        let row = offset + (index / 3) * 15;
        let col = ((index % 3) * 9) as u16;
        sheet.merge_range(
            row,
            col,
            row,
            col + 7,
            &format!("{} 月", grid.month),
            &title,
        )?;
        sheet.write_string_with_format(
            row + 1,
            col,
            "週",
            &general
                .clone()
                .set_border_left_color(Color::RGB(options.colors[5])),
        )?;
        for (weekday, name) in WEEKDAYS.iter().enumerate() {
            let mut format = if weekday == 0 || weekday == 6 {
                general
                    .clone()
                    .set_background_color(Color::RGB(options.colors[0]))
            } else {
                general.clone()
            };
            if weekday == 6 {
                format = format.set_border_right_color(Color::RGB(options.colors[5]));
            }
            sheet.write_string_with_format(
                row + 1,
                col + 1 + weekday as u16,
                name.trim_start_matches("星期"),
                &format,
            )?;
        }
        for (index, day) in grid.days.iter().enumerate() {
            let cell_row = row + 2 + (index / 7) as u32 * 2;
            let cell_col = col + 1 + (index % 7) as u16;
            if index % 7 == 0 {
                let (year, week) = row_week(day.date);
                let label = if year == options.year {
                    week.to_string()
                } else {
                    format!("{year}\n{week}")
                };
                let mut week_format = general
                    .clone()
                    .set_border_left_color(Color::RGB(options.colors[5]));
                if index / 7 == 5 {
                    week_format =
                        week_format.set_border_bottom_color(Color::RGB(options.colors[5]));
                }
                sheet.merge_range(cell_row, col, cell_row + 1, col, &label, &week_format)?;
                sheet.set_row_height(cell_row, 13)?;
                sheet.set_row_height(cell_row + 1, 11)?;
            }
            let in_month = day.date.month() == grid.month && day.date.year() == options.year;
            let mut format = general.clone().set_background_color(Color::RGB(
                options.colors[day_color_index(day, options.year, grid.month)],
            ));
            if !in_month {
                format = format.set_font_color(Color::RGB(0x849297));
            }
            if index % 7 == 6 {
                format = format.set_border_right_color(Color::RGB(options.colors[5]));
            }
            sheet.write_number_with_format(
                cell_row,
                cell_col,
                day.date.day(),
                &format.clone().set_font_size(10).set_bold(),
            )?;
            if index / 7 == 5 {
                format = format.set_border_bottom_color(Color::RGB(options.colors[5]));
            }
            sheet.write_string_with_format(
                cell_row + 1,
                cell_col,
                day.short_label(),
                &format.set_font_size(7).set_text_wrap(),
            )?;
            if in_month {
                valid.push(day);
            }
        }
        let _ = progress.send(Progress::Month(grid.month));
    }
    let end = offset + 60;
    sheet.merge_range(
        end,
        0,
        end,
        25,
        &format!(
            "{}；無官方補班／補假推算。農民曆為民俗參考，不代表原版避凶算法。",
            events::WARNING
        ),
        &plain,
    )?;
    for (index, color) in options.colors.iter().enumerate() {
        let col = index as u16 * 3;
        sheet.merge_range(
            end + 1,
            col,
            end + 1,
            col + 2,
            COLOR_NAMES[index],
            &general
                .clone()
                .set_background_color(Color::RGB(*color))
                .set_font_size(7),
        )?;
    }
    sheet.set_print_area(0, 0, end + 1, 25)?;
    let expected = if crate::calendar::is_leap_year(options.year) {
        366
    } else {
        365
    };
    ensure!(valid.len() == expected, "匯出有效日期數錯誤");
    let sources = workbook.add_worksheet();
    sources.set_name("資料來源")?;
    sources
        .set_column_width(0, 16)?
        .set_column_width(1, 30)?
        .set_column_width(2, 60)?;
    let metadata = [
        (
            "程式",
            format!("CalendarRust {}", env!("CARGO_PKG_VERSION")),
        ),
        (
            "農曆／事件",
            if options.year < 1970 {
                "lunar_rust 1.0.1／MIT 歷史候選".into()
            } else {
                "LunarData／擇日知錄／MIT".into()
            },
        ),
        ("採用來源 SHA-256", options.source_snapshot.clone()),
        ("原始快照 SHA-256",provider.lunar_data.manifest.snapshot_sha256.clone()),
        ("HKO 來源與權利","香港天文台／香港特別行政區政府／DATA.GOV.HK；原始數據知識產權屬香港特別行政區政府及香港天文台；依 https://data.gov.hk/en/terms-and-conditions 再用，非政府背書".into()),
        ("LunarData 權利","LunarData／擇日知錄；Copyright (c) 2026 當麻實驗室；MIT；https://github.com/donma/LunarData".into()),
        (
            "校正",
            provider.lunar_data.manifest.correction_version.clone(),
        ),
        (
            "範圍",
            "1912–2050；1970 前無 LunarData 農民曆；節氣及歷史差異以核准 HKO 曆表校正，raw 保留"
                .into(),
        ),
        (
            "正確性",
            "HKO 同時為核准校正與比對基準，不宣稱第三個獨立來源再次驗證；本輸出不是政府辦公日曆"
                .into(),
        ),
        ("事件來源", events::SOURCE.into()),
        (
            "警告",
            format!(
                "{}；民俗參考，不作安全、醫療或重大決策依據",
                events::WARNING
            ),
        ),
        (
            "農曆年界",
            "農曆正月初一換歲次／生肖，不以公曆年或立春換年".into(),
        ),
    ];
    for (row, (name, value)) in metadata.iter().enumerate() {
        sources.write_string(row as u32, 0, *name)?;
        sources.merge_range(row as u32, 1, row as u32, 7, value, &plain)?;
        sources.set_row_height(row as u32, 44)?;
    }
    let heading = [
        "公曆日期",
        "農曆年月日",
        "閏月",
        "歲次／生肖",
        "事件（完整）",
        "列週年／週",
        "節氣可用性",
        "來源",
    ];
    let heading_row = metadata.len() as u32 + 2;
    for (col, name) in heading.iter().enumerate() {
        sources.write_string(heading_row, col as u16, *name)?;
    }
    for (row, day) in valid.iter().enumerate() {
        let row = row as u32 + heading_row + 1;
        sources.write_string(row, 0, day.date.to_string())?;
        match &day.lunar {
            Ok(lunar) => {
                sources.write_string(
                    row,
                    1,
                    format!("{}-{:02}-{:02}", lunar.year, lunar.month, lunar.day),
                )?;
                sources.write_boolean(row, 2, lunar.is_leap_month)?;
                sources.write_string(row, 3, format!("{}／{}", lunar.year_ganzhi, lunar.zodiac))?;
            }
            Err(error) => {
                sources.write_string(row, 1, format!("不可用：{error}"))?;
            }
        }
        sources.write_string(
            row,
            4,
            day.events
                .iter()
                .map(|event| event.name.as_str())
                .collect::<Vec<_>>()
                .join("、"),
        )?;
        let (year, week) = row_week(day.date);
        sources.write_string(row, 5, format!("{year}-W{week:02}"))?;
        sources.write_string(
            row,
            6,
            match &day.solar_term {
                Ok(_) => "可用（核准 HKO UTC+08:00 區間）".into(),
                Err(error) => format!("不可用：{error}"),
            },
        )?;
        sources.write_string(row, 7, &day.source)?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let parent = options.target.parent().context("匯出位置無效")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    workbook.save_to_writer(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(false);
    }
    if options.overwrite {
        temporary.persist(&options.target)?;
    } else {
        temporary.persist_noclobber(&options.target)?;
    }
    Ok(true)
}
