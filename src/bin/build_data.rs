use anyhow::{Context, Result};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("用法：build-data <LunarData 來源目錄> <輸出目錄>")?,
    );
    let output = PathBuf::from(args.next().context("缺少輸出目錄")?);
    let fixture = PathBuf::from(args.next().context("缺少獨立 HKO fixture.gz 路徑")?);
    let manifest = calendar_rust::data::build_package(&source, &output, &fixture)?;
    println!(
        "已封裝 {} 日；採用範圍 raw 節氣異常 {} 筆（採用值改讀 HKO）；全快照 {} 筆異常；來源 {}",
        manifest.days,
        manifest.invalid_solar_terms.len(),
        manifest.full_snapshot_invalid_solar_terms,
        manifest.snapshot_sha256
    );
    Ok(())
}
