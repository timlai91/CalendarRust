use crate::calendar::LunarDate;
use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

pub const WARNING: &str = ""; //"節日／紀念日不等於現行法定休假";
pub const SOURCE: &str = ""; //"CalendarRust 獨立整理 v1；非原版資料表／政府辦公日表";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Memorial,
    LunarFestival,
    SolarTerm,
    Supplemental,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DayEvent {
    pub id: String,
    pub name: String,
    pub kind: EventKind,
    pub source: String,
    pub legacy_marked: bool,
}

pub fn event(id: String, name: &str, kind: EventKind, source: &str) -> DayEvent {
    DayEvent {
        id,
        name: name.into(),
        kind,
        source: source.into(),
        legacy_marked: false,
    }
}

pub fn canonical_name(name: &str) -> &str {
    match name {
        "元旦" => "開國紀念日",
        "國際婦女節" => "婦女節",
        "雙十節" | "國慶節" => "國慶紀念日",
        "聖誕節" => "耶誕節",
        "七夕情人節" => "七夕",
        _ => name,
    }
}

pub fn resolve(
    date: NaiveDate,
    lunar: &LunarDate,
    next_lunar: Option<&LunarDate>,
) -> Vec<DayEvent> {
    let mut result = Vec::new();
    for &(month, day, name) in SOLAR {
        if date.month() == month && date.day() == day {
            result.push(event(
                format!("solar-{month:02}{day:02}-{name}"),
                name,
                EventKind::Memorial,
                SOURCE,
            ));
        }
    }
    if date.month() == 5 && date.weekday() == Weekday::Sun && (8..=14).contains(&date.day()) {
        result.push(event(
            "mothers-day".into(),
            "母親節",
            EventKind::Memorial,
            SOURCE,
        ));
    }
    if !lunar.is_leap_month {
        for &(month, day, name) in LUNAR {
            if lunar.month == month && lunar.day == day {
                result.push(event(
                    format!("lunar-{month:02}{day:02}-{name}"),
                    name,
                    EventKind::LunarFestival,
                    SOURCE,
                ));
            }
        }
    }
    if next_lunar.is_some_and(|next| !next.is_leap_month && next.month == 1 && next.day == 1) {
        result.push(event(
            "lunar-new-years-eve".into(),
            "除夕",
            EventKind::LunarFestival,
            SOURCE,
        ));
    }
    if !lunar.is_leap_month && lunar.month == 7 && next_lunar.is_some_and(|next| next.day == 1) {
        result.push(event(
            "ghost-gate-close".into(),
            "關鬼門",
            EventKind::LunarFestival,
            SOURCE,
        ));
    }
    result
}

// Calendar facts are independently curated, not extracted original program resources.
const SOLAR: &[(u32, u32, &str)] = &[
    (1, 1, "開國紀念日"),
    (1, 6, "消防節"),
    (1, 11, "司法節"),
    (1, 23, "自由日"),
    (2, 14, "情人節"),
    (2, 19, "國民生活運動紀念日"),
    (2, 28, "和平紀念日"),
    (3, 1, "兵役節"),
    (3, 5, "童軍節"),
    (3, 8, "婦女節"),
    (3, 12, "植樹節"),
    (3, 14, "白色情人節"),
    (3, 20, "郵政節"),
    (3, 21, "世界森林日"),
    (3, 22, "世界水日"),
    (3, 25, "美術節"),
    (3, 26, "廣播節"),
    (3, 29, "青年節"),
    (4, 1, "愚人節"),
    (4, 4, "兒童節"),
    (4, 7, "世界衛生日"),
    (4, 22, "世界地球日"),
    (4, 23, "世界書香日"),
    (5, 1, "勞動節"),
    (5, 4, "文藝節"),
    (5, 8, "世界紅十字日"),
    (5, 12, "國際護師節"),
    (5, 18, "國際博物館日"),
    (5, 31, "世界無菸日"),
    (6, 3, "禁菸節"),
    (6, 5, "世界環境日"),
    (6, 6, "工程師節"),
    (6, 8, "世界海洋日"),
    (6, 9, "鐵路節"),
    (6, 15, "警察節"),
    (7, 1, "漁民節"),
    (7, 7, "七七抗戰紀念日"),
    (7, 11, "航海節"),
    (8, 8, "父親節"),
    (8, 14, "空軍節"),
    (8, 27, "鄭成功誕辰"),
    (9, 1, "記者節"),
    (9, 3, "軍人節"),
    (9, 8, "國際掃盲日"),
    (9, 9, "體育節"),
    (9, 21, "國際和平日"),
    (9, 28, "教師節"),
    (10, 4, "世界動物日"),
    (10, 10, "國慶紀念日"),
    (10, 21, "華僑節"),
    (10, 24, "聯合國日"),
    (10, 25, "臺灣光復節"),
    (10, 31, "萬聖夜"),
    (11, 1, "商人節"),
    (11, 11, "工業節"),
    (11, 12, "國父誕辰紀念日"),
    (11, 17, "國際學生節"),
    (11, 21, "防空節"),
    (12, 3, "國際身心障礙者日"),
    (12, 10, "世界人權日"),
    (12, 12, "憲兵節"),
    (12, 24, "平安夜"),
    (12, 25, "行憲紀念日"),
    (12, 25, "耶誕節"),
];

const LUNAR: &[(u32, u32, &str)] = &[
    (1, 1, "春節"),
    (1, 2, "初二"),
    (1, 3, "初三"),
    (1, 5, "開市"),
    (1, 9, "天公生"),
    (1, 15, "元宵節"),
    (2, 2, "土地公生"),
    (2, 19, "觀音誕辰"),
    (3, 3, "上巳節"),
    (3, 23, "媽祖誕辰"),
    (4, 8, "浴佛節"),
    (5, 5, "端午節"),
    (6, 19, "觀音成道"),
    (7, 1, "開鬼門"),
    (7, 7, "七夕"),
    (7, 15, "中元節"),
    (8, 15, "中秋節"),
    (9, 9, "重陽節"),
    (10, 15, "下元節"),
    (12, 8, "臘八節"),
    (12, 16, "尾牙"),
];
