use crate::{
    data::{LunarData, RawDay, SolarTerm},
    events::{self, DayEvent, EventKind},
};
use anyhow::{Context, Result, ensure};
use chrono::{Datelike, Duration, NaiveDate};
use lunar_rust::{
    lunar::LunarRefHelper,
    solar::{self, SolarRefHelper},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MIN: NaiveDate = NaiveDate::from_ymd_opt(1912, 1, 1).unwrap();
pub const MAX: NaiveDate = NaiveDate::from_ymd_opt(2050, 12, 31).unwrap();
pub const CORE_MIN: NaiveDate = NaiveDate::from_ymd_opt(1911, 12, 1).unwrap();
pub const CORE_MAX: NaiveDate = NaiveDate::from_ymd_opt(2051, 1, 31).unwrap();
pub const WEEKDAYS: [&str; 7] = [
    "星期日",
    "星期一",
    "星期二",
    "星期三",
    "星期四",
    "星期五",
    "星期六",
];
pub const WEEKDAY_EN: [&str; 7] = ["Sun", "Mon", "Tues", "Wed", "Thur", "Fri", "Sat"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LunarDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub is_leap_month: bool,
    pub month_name: String,
    pub day_name: String,
    pub year_ganzhi: String,
    pub zodiac: String,
}

impl LunarDate {
    pub fn description(&self) -> String {
        format!(
            "農曆 {}年 {}{}{} · {}年／{}",
            self.year,
            if self.is_leap_month { "閏" } else { "" },
            self.month_name,
            self.day_name,
            self.year_ganzhi,
            self.zodiac
        )
    }
}

#[derive(Clone, Debug)]
pub enum AlmanacAvailability {
    Available(Box<RawDay>),
    OutOfRange,
    Invalid(String),
}

#[derive(Clone, Debug)]
pub struct DateInfo {
    pub date: NaiveDate,
    pub lunar: std::result::Result<LunarDate, String>,
    pub month_ganzhi: String,
    pub day_ganzhi: String,
    pub solar_term: std::result::Result<Option<SolarTerm>, String>,
    pub events: Vec<DayEvent>,
    pub event_error: Option<String>,
    pub almanac: AlmanacAvailability,
    pub source: String,
}

impl DateInfo {
    pub fn short_label(&self) -> String {
        if let Some(event) = self.events.first() {
            return event.name.clone();
        }
        match &self.lunar {
            Ok(lunar) if lunar.day == 1 => format!(
                "{}{}",
                if lunar.is_leap_month { "閏" } else { "" },
                lunar.month_name
            ),
            Ok(lunar) => lunar.day_name.clone(),
            Err(_) => "資料不可用".into(),
        }
    }

    pub fn tooltip(&self) -> String {
        let (year, week) = row_week(self.date);
        let lunar = match &self.lunar {
            Ok(l) => l.description(),
            Err(e) => e.clone(),
        };
        let events = self
            .events
            .iter()
            .map(|e| format!("{}〔{}〕", e.name, e.source))
            .collect::<Vec<_>>()
            .join("\n");
        let term = match &self.solar_term {
            Ok(Some(term)) => format!(
                "節氣期間：{}（{} ～ {} 前）",
                term.name, term.start_date, term.end_date
            ),
            Ok(None) => String::new(),
            Err(e) => format!("節氣欄位不可用：{e}"),
        };
        [
            format!(
                "{} {}（民國 {} 年）",
                self.date,
                WEEKDAYS[self.date.weekday().num_days_from_sunday() as usize],
                self.date.year() - 1911,
            ),
            lunar,
            format!("月干支 {} · 日干支 {}", self.month_ganzhi, self.day_ganzhi),
            format!("列週數：{year}年第{week}週"),
            events,
            self.event_error.clone().unwrap_or_default(),
            term,
        ]
        .iter()
        .flat_map(|section| section.lines())
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
    }
}

pub struct CalendarProvider {
    pub lunar_data: LunarData,
    pub supplemental: bool,
}

impl CalendarProvider {
    pub fn open(data_directory: &Path) -> Result<Self> {
        Ok(Self {
            lunar_data: LunarData::open(data_directory)?,
            supplemental: false,
        })
    }

    pub fn info(&mut self, date: NaiveDate) -> DateInfo {
        if !(CORE_MIN..=CORE_MAX).contains(&date) {
            return unavailable(date, "超出曆法資料範圍".into());
        }
        let mut info = if date.year() >= 1970 {
            match self.lunar_data.day(date) {
                Ok(raw) => from_raw(raw),
                Err(error) => return unavailable(date, format!("{error:#}")),
            }
        } else {
            match self.lunar_data.historical(date) {
                Ok(day) => from_historical(day),
                Err(error) => return unavailable(date, format!("歷史來源損壞／缺日：{error:#}")),
            }
        };
        info.solar_term = self
            .lunar_data
            .period(date)
            .map(Some)
            .map_err(|error| format!("{error:#}"));
        if let (Ok(Some(period)), AlmanacAvailability::Available(raw)) =
            (&mut info.solar_term, &info.almanac)
            && traditional(&raw.solar_term.name) == period.name
        {
            period.desc = raw.solar_term.desc.clone();
        }
        if let Ok(lunar) = &info.lunar {
            let next_date = date + Duration::days(1);
            let next_lunar = if matches!(lunar.month, 7 | 12) && lunar.day >= 29 {
                if next_date.year() >= 1970 {
                    match self.lunar_data.day(next_date) {
                        Ok(raw) => Some(raw_lunar(&raw)),
                        Err(error) => {
                            info.event_error =
                                Some(format!("無法確認次日，月末事件不可用：{error:#}"));
                            None
                        }
                    }
                } else {
                    match self.lunar_data.historical(next_date) {
                        Ok(day) => Some(day.lunar),
                        Err(error) => {
                            info.event_error =
                                Some(format!("無法確認次日，月末事件不可用：{error:#}"));
                            None
                        }
                    }
                }
            } else {
                None
            };
            info.events = events::resolve(date, lunar, next_lunar.as_ref());
        }
        if let Ok(Some(term)) = &info.solar_term
            && term.start_date == date
        {
            info.events.push(events::event(
                format!("term-{}-{}", date, term.name),
                &term.name,
                EventKind::SolarTerm,
                &info.source,
            ));
        }
        if self.supplemental
            && let AlmanacAvailability::Available(raw) = &info.almanac
        {
            for name in &raw.deity_birthday {
                if !info.events.iter().any(|event| event.name == *name) {
                    info.events.push(events::event(
                        format!("lunardata-{date}-{name}"),
                        name,
                        EventKind::Supplemental,
                        "LunarData 民俗事件（未合併別名）",
                    ));
                }
            }
            if self.supplemental {
                match self.lunar_data.supplemental_events(date) {
                    Ok(events) => {
                        for (index, event) in events.iter().enumerate() {
                            if !info.events.iter().any(|existing| {
                                events::canonical_name(&existing.name)
                                    == events::canonical_name(&event.name)
                            }) {
                                info.events.push(events::event(
                                    format!("supplemental-{date}-{index}"),
                                    &event.name,
                                    EventKind::Supplemental,
                                    &format!(
                                        "LunarData 可選節日補充；{} {}",
                                        event.emoji, event.desc
                                    ),
                                ));
                            }
                        }
                    }
                    Err(error) => info.event_error = Some(format!("補充事件不可用：{error:#}")),
                }
            }
        }
        info
    }

    pub fn month(&mut self, year: i32, month: u32) -> Result<MonthGrid> {
        let dates = grid_dates(year, month)?;
        Ok(MonthGrid {
            year,
            month,
            days: dates.into_iter().map(|date| self.info(date)).collect(),
        })
    }
}

fn raw_lunar(raw: &RawDay) -> LunarDate {
    LunarDate {
        year: raw.lunar.year,
        month: raw.lunar.month,
        day: raw.lunar.day,
        is_leap_month: raw.lunar.is_leap_month,
        month_name: raw
            .lunar
            .month_name
            .trim_start_matches(['閏', '闰'])
            .trim_end_matches(['大', '小'])
            .to_string(),
        day_name: raw.lunar.day_name.clone(),
        year_ganzhi: raw.lunar.year_ganzhi.clone(),
        zodiac: raw.lunar.zodiac.clone(),
    }
}

fn from_raw(raw: RawDay) -> DateInfo {
    let solar_term = raw
        .solar_term
        .validate(raw.gregorian)
        .map(|_| Some(raw.solar_term.clone()))
        .map_err(|e| e.to_string());
    DateInfo {
        date: raw.gregorian,
        lunar: Ok(raw_lunar(&raw)),
        month_ganzhi: raw.month_ganzhi.full.clone(),
        day_ganzhi: raw.day_ganzhi.full.clone(),
        solar_term,
        events: Vec::new(),
        event_error: None,
        almanac: AlmanacAvailability::Available(Box::new(raw)),
        source: "".into(),
    }
}

pub fn historical_candidate(date: NaiveDate) -> crate::data::HistoricalDay {
    let lunar =
        solar::from_ymd(date.year() as i64, date.month() as i64, date.day() as i64).get_lunar();
    let month = lunar.get_month();
    let year = lunar.get_year();
    let lunar_date = LunarDate {
        year: year as i32,
        month: month.unsigned_abs() as u32,
        day: lunar.get_day() as u32,
        is_leap_month: month < 0,
        month_name: format!(
            "{}月",
            traditional(&lunar.get_month_in_chinese()).trim_start_matches('閏')
        ),
        day_name: traditional(&lunar.get_day_in_chinese()),
        year_ganzhi: lunar.get_year_in_gan_zhi(),
        zodiac: traditional(&lunar.get_year_sheng_xiao()),
    };
    crate::data::HistoricalDay {
        date,
        lunar: lunar_date,
        month_ganzhi: lunar.get_month_in_gan_zhi(),
        day_ganzhi: lunar.get_day_in_gan_zhi(),
        solar_term: traditional(&lunar.get_jie_qi()),
    }
}

fn from_historical(day: crate::data::HistoricalDay) -> DateInfo {
    DateInfo {
        date: day.date,
        lunar: Ok(day.lunar),
        month_ganzhi: day.month_ganzhi,
        day_ganzhi: day.day_ganzhi,
        solar_term: Ok(None),
        events: Vec::new(),
        event_error: None,
        almanac: AlmanacAvailability::OutOfRange,
        source: "農曆：lunar_rust 1.0.1".into(),
    }
}

fn unavailable(date: NaiveDate, reason: String) -> DateInfo {
    DateInfo {
        date,
        lunar: Err(reason.clone()),
        month_ganzhi: "不可用".into(),
        day_ganzhi: "不可用".into(),
        solar_term: Err(reason.clone()),
        events: Vec::new(),
        event_error: Some(reason.clone()),
        almanac: AlmanacAvailability::Invalid(reason),
        source: "資料不可用；未替換算法".into(),
    }
}

pub fn traditional(value: &str) -> String {
    value
        .replace('龙', "龍")
        .replace('马', "馬")
        .replace('鸡', "雞")
        .replace('猪', "豬")
        .replace('腊', "臘")
        .replace('闰', "閏")
        .replace('惊', "驚")
        .replace('蛰', "蟄")
        .replace("谷雨", "穀雨")
        .replace("小满", "小滿")
        .replace("芒种", "芒種")
        .replace("处暑", "處暑")
}

pub fn lunar_from_parts(year: i32, month: u32, day: u32, leap: bool) -> Result<LunarDate> {
    ensure!(
        (1..=12).contains(&month) && (1..=30).contains(&day),
        "校正農曆數值無效"
    );
    let stems = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
    let branches = [
        "子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥",
    ];
    let months = [
        "正月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ];
    let days = [
        "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十", "十一",
        "十二", "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十", "廿一", "廿二",
        "廿三", "廿四", "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
    ];
    let branch = (year - 4).rem_euclid(12) as usize;
    Ok(LunarDate {
        year,
        month,
        day,
        is_leap_month: leap,
        month_name: months[month as usize - 1].into(),
        day_name: days[day as usize - 1].into(),
        year_ganzhi: format!(
            "{}{}",
            stems[(year - 4).rem_euclid(10) as usize],
            branches[branch]
        ),
        zodiac: crate::data::ZODIAC[branch].0.into(),
    })
}

pub struct MonthGrid {
    pub year: i32,
    pub month: u32,
    pub days: Vec<DateInfo>,
}

pub fn selectable(date: NaiveDate) -> bool {
    (MIN..=MAX).contains(&date)
}
pub fn clamp(date: NaiveDate) -> NaiveDate {
    date.clamp(MIN, MAX)
}

pub fn is_leap_year(year: i32) -> bool {
    year % 400 == 0 || (year % 4 == 0 && year % 100 != 0)
}

pub fn month_length(year: i32, month: u32) -> Result<u32> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).context("年月無效")?;
    let next = first
        .checked_add_months(chrono::Months::new(1))
        .context("年月超界")?;
    Ok((next - first).num_days() as u32)
}

pub fn navigate_month(date: NaiveDate, delta: i32) -> Result<NaiveDate> {
    let months = date.year() * 12 + date.month0() as i32 + delta;
    let year = months.div_euclid(12);
    let month = months.rem_euclid(12) as u32 + 1;
    let result = NaiveDate::from_ymd_opt(year, month, date.day().min(month_length(year, month)?))
        .context("目標日期無效")?;
    ensure!(selectable(result), "已到支援範圍邊界（1912–2050）");
    Ok(result)
}

pub fn grid_dates(year: i32, month: u32) -> Result<[NaiveDate; 42]> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).context("月份無效")?;
    let start = first - Duration::days(first.weekday().num_days_from_sunday() as i64);
    Ok(std::array::from_fn(|index| {
        start + Duration::days(index as i64)
    }))
}

pub fn row_week(date: NaiveDate) -> (i32, u32) {
    let sunday = date - Duration::days(date.weekday().num_days_from_sunday() as i64);
    let iso = (sunday + Duration::days(4)).iso_week();
    (iso.year(), iso.week())
}
