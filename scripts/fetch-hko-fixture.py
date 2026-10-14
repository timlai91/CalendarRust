"""Optional developer-only independent fixture retrieval; never used by the app."""
import concurrent.futures
import datetime
import gzip
import hashlib
import json
import pathlib
import re
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "tests" / "fixtures"
MONTHS = {"正": 1, "一": 1, "二": 2, "三": 3, "四": 4, "五": 5, "六": 6,
          "七": 7, "八": 8, "九": 9, "十": 10, "十一": 11, "十二": 12}
DAYS = ["初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十",
        "十一", "十二", "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十",
        "廿一", "廿二", "廿三", "廿四", "廿五", "廿六", "廿七", "廿八", "廿九", "三十"]
LINE = re.compile(r"^(\d{4})年(\d+)月(\d+)日\s+(\S+)\s+星期[日一二三四五六]\s*(\S*)")


def retrieve(year):
    url = f"https://www.hko.gov.hk/tc/gts/time/calendar/text/files/T{year}c.txt"
    request = urllib.request.Request(url, headers={"User-Agent": "CalendarRust-fixture-verification/0.1"})
    with urllib.request.urlopen(request, timeout=60) as response:
        raw = response.read(256_001)
    if len(raw) > 256_000:
        raise ValueError(f"{year}: source size limit exceeded")
    text = raw.decode("utf-8-sig", errors="strict")
    return year, text, {"url": url, "sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw)}


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
        source = sorted(executor.map(retrieve, range(1911, 2052)))
    result = []
    files = []
    lunar_year = month = None
    leap = False
    previous = None
    for year, text, metadata in source:
        count = 0
        for line in text.splitlines():
            matched = LINE.match(line)
            if not matched:
                continue
            y, m, d, lunar, term = matched.groups()
            date = datetime.date(int(y), int(m), int(d))
            if date.year != year:
                raise ValueError(f"{year}: unexpected date {date}")
            if previous and date != previous + datetime.timedelta(days=1):
                raise ValueError(f"noncontiguous source at {date}")
            previous = date
            count += 1
            if lunar.endswith("月"):
                leap = lunar.startswith(("閏", "闰"))
                label = lunar.removeprefix("閏").removeprefix("闰").removesuffix("月")
                month = MONTHS[label]
                if month == 1 and not leap:
                    lunar_year = date.year
                elif lunar_year is None:
                    lunar_year = date.year - (month >= 11 and date.month <= 2)
                lunar_day = 1
            else:
                lunar_day = DAYS.index(lunar) + 1
            if date >= datetime.date(1911, 11, 1) and date <= datetime.date(2051, 2, 28):
                if lunar_year is None or month is None:
                    raise ValueError(f"unseeded lunar month at {date}")
                result.append([date.isoformat(), lunar_year, month, lunar_day, leap, term])
        if count != (366 if year % 4 == 0 and (year % 100 != 0 or year % 400 == 0) else 365):
            raise ValueError(f"{year}: missing calendar rows ({count})")
        files.append(metadata)
    payload = json.dumps(result, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    (OUTPUT / "hko-calendar.json.gz").write_bytes(gzip.compress(payload, mtime=0))
    manifest = {
        "source": "Hong Kong Observatory / Government of the Hong Kong SAR / DATA.GOV.HK",
        "dataset": "https://data.gov.hk/tc-data/dataset/hk-hko-rss-gregorian-lunar-calendar-conversion-table",
        "terms": "https://data.gov.hk/en/terms-and-conditions",
        "attribution": "Intellectual property rights in the source Data belong to the Government of the Hong Kong SAR and the Hong Kong Observatory. Extracted conversion facts are used with attribution under DATA.GOV.HK Terms of Use.",
        "retrieved_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "timezone": "UTC+08:00 (Hong Kong); compare Taiwan civil calendar dates, not historical local clock offsets",
        "coverage": "1911-11-01..2051-02-28",
        "days": len(result),
        "fixture_sha256": hashlib.sha256(payload).hexdigest(),
        "normalization": "v1; chronological month-boundary labels; lunar year changes only at non-leap month 1; numeric conversion facts and solar-term names only",
        "files": files,
    }
    (OUTPUT / "hko-manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Retrieved {len(result)} independent dates; SHA-256 {manifest['fixture_sha256']}")


if __name__ == "__main__":
    main()
