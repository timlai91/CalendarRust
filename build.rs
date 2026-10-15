use image::{
    ExtendedColorType,
    codecs::ico::{IcoEncoder, IcoFrame},
    imageops::{FilterType, resize},
};
use std::{error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    let source_path = PathBuf::from("assets").join("icons").join("512.png");
    println!("cargo:rerun-if-changed={}", source_path.display());
    if std::env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }

    let source = image::open(source_path)?.into_rgba8();
    let mut frames = Vec::new();
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let scaled = resize(&source, size, size, FilterType::Lanczos3);
        frames.push(IcoFrame::as_png(
            &scaled,
            size,
            size,
            ExtendedColorType::Rgba8,
        )?);
    }
    let output = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Missing OUT_DIR")?);
    let icon_path = output.join("app.ico");
    IcoEncoder::new(fs::File::create(&icon_path)?).encode_images(&frames)?;
    let resource_path = output.join("app.rc");
    fs::write(
        &resource_path,
        format!(
            "#pragma code_page(65001)\n1 ICON \"{}\"\n",
            icon_path.display().to_string().replace('\\', "\\\\")
        ),
    )?;
    embed_resource::compile_for(&resource_path, ["CalendarRust"], embed_resource::NONE)
        .manifest_required()?;
    Ok(())
}
