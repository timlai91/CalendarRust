use anyhow::{Context, Result, ensure};
use calendar_rust::{
    calendar::{self, CalendarProvider},
    data::digest,
    events::EventKind,
};
use chrono::NaiveDate;
use serde::Serialize;
use std::{collections::BTreeSet, io::Read, path::PathBuf};

#[derive(Serialize)]
struct Difference {
    date: NaiveDate,
    field: String,
    expected: String,
    actual: String,
}

#[derive(Serialize)]
struct Report {
    oracle: String,
    fixture_sha256: String,
    implementation_sha256: String,
    checked_days: usize,
    lunar_differences: Vec<Difference>,
    solar_term_differences: Vec<Difference>,
    quarantined_solar_term_days: Vec<NaiveDate>,
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let data = PathBuf::from(
        args.next()
            .context("用法：verify-calendar <來源資料目錄> <獨立 fixture.gz> <報告.json>")?,
    );
    let fixture = PathBuf::from(args.next().context("缺少獨立 fixture")?);
    let output = PathBuf::from(args.next().context("缺少報告位置")?);
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(fixture)?).read_to_end(&mut bytes)?;
    let rows: Vec<(NaiveDate, i32, u32, u32, bool, String)> = serde_json::from_slice(&bytes)?;
    let mut provider = CalendarProvider::open(&data)?;
    ensure!(
        digest(&bytes) == provider.lunar_data.manifest.hko_fixture_sha256,
        "fixture 與核准資料包的基準不同"
    );
    let mut seen = BTreeSet::new();
    let stems = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
    let branches = [
        "子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥",
    ];
    let animals = [
        "鼠", "牛", "虎", "兔", "龍", "蛇", "馬", "羊", "猴", "雞", "狗", "豬",
    ];
    let mut report=Report {
        oracle:"Hong Kong Observatory / Government of the Hong Kong SAR / DATA.GOV.HK; UTC+08:00; approved adoption AND comparison baseline, not third-source revalidation".into(),
        fixture_sha256:digest(&bytes),implementation_sha256:provider.lunar_data.manifest.adopted_sha256.clone(),
        checked_days:0,lunar_differences:Vec::new(),solar_term_differences:Vec::new(),quarantined_solar_term_days:Vec::new(),
    };
    for (date, year, month, day, leap, term) in rows {
        if !(calendar::MIN..=calendar::MAX).contains(&date) {
            continue;
        }
        let info = provider.info(date);
        ensure!(seen.insert(date), "fixture 日期重複：{date}");
        report.checked_days += 1;
        let cycle = year - 4;
        let expected = format!(
            "{year}-{month:02}-{day:02}; leap={leap}; {}{}/{}",
            stems[cycle.rem_euclid(10) as usize],
            branches[cycle.rem_euclid(12) as usize],
            animals[cycle.rem_euclid(12) as usize]
        );
        let actual = match info.lunar {
            Ok(lunar) => format!(
                "{}-{:02}-{:02}; leap={}; {}/{}",
                lunar.year,
                lunar.month,
                lunar.day,
                lunar.is_leap_month,
                lunar.year_ganzhi,
                lunar.zodiac
            ),
            Err(error) => format!("不可用：{error}"),
        };
        if expected != actual {
            report.lunar_differences.push(Difference {
                date,
                field: "lunar".into(),
                expected,
                actual,
            });
        }
        if info.solar_term.is_err() {
            report.quarantined_solar_term_days.push(date);
        } else {
            let actual = info
                .events
                .iter()
                .filter(|event| event.kind == EventKind::SolarTerm)
                .map(|event| event.name.as_str())
                .collect::<Vec<_>>()
                .join("、");
            if actual != term {
                report.solar_term_differences.push(Difference {
                    date,
                    field: "solar_term".into(),
                    expected: term,
                    actual,
                });
            }
        }
    }
    ensure!(
        report.checked_days == 50_770,
        "獨立 fixture 缺日／重複，沒有完成完整核對"
    );
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "核對 {} 日：農曆差異 {}，節氣差異 {}，隔離節氣 {} 日",
        report.checked_days,
        report.lunar_differences.len(),
        report.solar_term_differences.len(),
        report.quarantined_solar_term_days.len()
    );
    ensure!(
        report.lunar_differences.is_empty()
            && report.solar_term_differences.is_empty()
            && report.quarantined_solar_term_days.is_empty(),
        "獨立對照尚未通過發布門檻；請查看完整差異報告，不以抽樣或同來源自測取代"
    );
    Ok(())
}
