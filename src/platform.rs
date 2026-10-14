use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, Local};
use std::path::PathBuf;

pub trait Clock {
    fn now(&self) -> DateTime<FixedOffset>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<FixedOffset> {
        Local::now().fixed_offset()
    }
}

pub struct FixedClock(pub DateTime<FixedOffset>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<FixedOffset> {
        self.0
    }
}

pub fn data_directory() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").context("找不到 LOCALAPPDATA，無法確保持久保存")?;
    Ok(PathBuf::from(base).join("CalendarRust"))
}

pub fn resources() -> Result<PathBuf> {
    Ok(std::env::current_exe()
        .context("無法取得執行檔位置")?
        .parent()
        .context("執行檔沒有父目錄")?
        .to_path_buf())
}

#[cfg(windows)]
pub fn visible_position(position: [f32; 2]) -> Option<[f32; 2]> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
    if !position.iter().all(|value| value.is_finite()) {
        return None;
    }
    let point = POINT {
        x: position[0] as i32 + 32,
        y: position[1] as i32 + 32,
    };
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) };
    (!monitor.is_null()).then_some(position)
}

#[cfg(not(windows))]
pub fn visible_position(_: [f32; 2]) -> Option<[f32; 2]> {
    None
}

pub fn install_fonts(ctx: &eframe::egui::Context) -> Result<()> {
    use eframe::egui::{FontData, FontDefinitions, FontFamily};
    let windows = std::env::var_os("WINDIR").context("找不到 Windows 字型目錄")?;
    let directory = PathBuf::from(windows).join("Fonts");
    let mut fonts = FontDefinitions::default();
    let mut chinese = false;
    for (name, file, primary) in [
        ("zh-TW", "msjh.ttc", true),
        ("zh-TW-fallback", "mingliu.ttc", false),
        ("emoji", "seguiemj.ttf", false),
    ] {
        if name == "zh-TW-fallback" && chinese {
            continue;
        }
        let path = directory.join(file);
        if path.is_file() {
            let bytes = std::fs::read(&path).context("無法載入系統中文字型")?;
            fonts.font_data.insert(
                name.into(),
                std::sync::Arc::new(FontData::from_owned(bytes)),
            );
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                let list = fonts.families.entry(family).or_default();
                if primary {
                    list.insert(0, name.into());
                } else {
                    list.push(name.into());
                }
            }
            chinese |= name.starts_with("zh-TW");
        }
    }
    if !chinese {
        bail!("未找到微軟正黑體或新細明體；請安裝 Windows 繁體中文補充字型後重試");
    }
    ctx.set_fonts(fonts);
    Ok(())
}

#[cfg(windows)]
pub fn open_document(path: &std::path::Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!(
        result as isize > 32,
        "無法使用預設關聯開啟檔案（Windows 錯誤 {}）；XLSX 已保存，可自行選擇應用程式開啟",
        result as isize
    );
    Ok(())
}

#[cfg(not(windows))]
pub fn open_document(_: &std::path::Path) -> Result<()> {
    bail!("此平台沒有 Windows 檔案關聯")
}

#[cfg(windows)]
pub fn show_fatal(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "CalendarRust".encode_utf16().chain(Some(0)).collect();
    // Both null-terminated buffers remain alive for the synchronous dialog.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
pub fn show_fatal(message: &str) {
    eprintln!("{message}");
}
