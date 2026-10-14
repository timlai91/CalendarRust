use anyhow::{Context, Result, ensure};
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader};
use std::{io::Cursor, path::Path};

pub const MAX_FILE: u64 = 20 * 1024 * 1024;
pub const MAX_EDGE: u32 = 8192;
pub const MAX_PIXELS: u64 = 32_000_000;

pub struct ValidatedImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn validate_dimensions(width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0
            && height > 0
            && width <= MAX_EDGE
            && height <= MAX_EDGE
            && u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "圖片尺寸超限（8192 px／3200 萬像素）"
    );
    Ok(())
}

pub fn load(path: &Path) -> Result<ValidatedImage> {
    let metadata = std::fs::metadata(path).context("無法讀取選取的圖片")?;
    ensure!(metadata.len() <= MAX_FILE, "圖片檔案超過 20 MiB");
    let bytes = std::fs::read(path)?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE,
        "圖片檔案在讀取時超過 20 MiB"
    );
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let decoded = if extension == "wmf" || extension == "emf" {
        metafile(&bytes, extension == "wmf")?
    } else {
        let reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        ensure!(
            matches!(
                reader.format(),
                Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp | ImageFormat::Ico)
            ),
            "僅支援 JPEG、BMP、ICO、PNG、WMF、EMF"
        );
        let (width, height) = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()?
            .into_dimensions()?;
        validate_dimensions(width, height)?;
        let mut reader = reader;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_EDGE);
        limits.max_image_height = Some(MAX_EDGE);
        limits.max_alloc = Some(160 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().context("圖片損壞或超過解碼上限")?
    };
    let (width, height) = decoded.dimensions();
    validate_dimensions(width, height)?;
    let mut output = Cursor::new(Vec::new());
    decoded.write_to(&mut output, ImageFormat::Png)?;
    Ok(ValidatedImage {
        png: output.into_inner(),
        width,
        height,
    })
}

#[cfg(not(windows))]
fn metafile(_: &[u8], _: bool) -> Result<DynamicImage> {
    anyhow::bail!("WMF／EMF 僅在 Windows 進行靜態轉點陣")
}

#[cfg(windows)]
fn metafile(bytes: &[u8], wmf: bool) -> Result<DynamicImage> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::DataExchange::{METAFILEPICT, SetWinMetaFileBits};

    struct Meta(HENHMETAFILE);
    impl Drop for Meta {
        fn drop(&mut self) {
            unsafe {
                DeleteEnhMetaFile(self.0);
            }
        }
    }
    struct Dc(HDC);
    impl Drop for Dc {
        fn drop(&mut self) {
            unsafe {
                DeleteDC(self.0);
            }
        }
    }
    struct Bitmap(HBITMAP);
    impl Drop for Bitmap {
        fn drop(&mut self) {
            unsafe {
                DeleteObject(self.0);
            }
        }
    }

    // GDI handles are scoped, source buffers are bounded, and no external application is invoked.
    unsafe {
        let (meta, requested) = if wmf {
            let placeable = bytes.len() >= 22 && bytes[..4] == [0xD7, 0xCD, 0xC6, 0x9A];
            let size;
            let mut offset = 0;
            if placeable {
                let read = |i: usize| i16::from_le_bytes([bytes[i], bytes[i + 1]]) as i32;
                let inch = u16::from_le_bytes([bytes[14], bytes[15]]);
                ensure!(inch > 0, "WMF 每英吋單位無效");
                let w = read(10) - read(6);
                let h = read(12) - read(8);
                ensure!(w > 0 && h > 0, "WMF 邊界無效");
                size = (
                    (w as f64 * 96.0 / f64::from(inch)).ceil() as u32,
                    (h as f64 * 96.0 / f64::from(inch)).ceil() as u32,
                );
                offset = 22;
            } else {
                size = wmf_extent(bytes)?;
            }
            validate_dimensions(size.0, size.1)?;
            ensure!(bytes.len() >= offset + 18, "WMF 標頭不足");
            let dc = Dc(CreateCompatibleDC(null_mut()));
            ensure!(!dc.0.is_null(), "無法建立 WMF 轉換裝置");
            let mapping = METAFILEPICT {
                mm: MM_ANISOTROPIC,
                xExt: (size.0 as f64 * 2540.0 / 96.0).round() as i32,
                yExt: (size.1 as f64 * 2540.0 / 96.0).round() as i32,
                hMF: null_mut(),
            };
            (
                Meta(SetWinMetaFileBits(
                    (bytes.len() - offset) as u32,
                    bytes[offset..].as_ptr(),
                    dc.0,
                    &mapping,
                )),
                Some(size),
            )
        } else {
            ensure!(bytes.len() >= 88, "EMF 標頭不足");
            (
                Meta(SetEnhMetaFileBits(bytes.len() as u32, bytes.as_ptr())),
                None,
            )
        };
        ensure!(!meta.0.is_null(), "Windows 無法解析 WMF／EMF");
        let mut header: ENHMETAHEADER = std::mem::zeroed();
        ensure!(
            GetEnhMetaFileHeader(
                meta.0,
                std::mem::size_of::<ENHMETAHEADER>() as u32,
                &mut header
            ) != 0,
            "無法讀取 EMF 邊界"
        );
        let (width, height) = requested.unwrap_or((
            (header.rclBounds.right as i64 - header.rclBounds.left as i64).max(0) as u32,
            (header.rclBounds.bottom as i64 - header.rclBounds.top as i64).max(0) as u32,
        ));
        validate_dimensions(width, height)?;
        let dc = Dc(CreateCompatibleDC(null_mut()));
        ensure!(!dc.0.is_null(), "無法建立圖片裝置");
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width as i32;
        info.bmiHeader.biHeight = -(height as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = null_mut();
        let bitmap = Bitmap(CreateDIBSection(
            dc.0,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            null_mut(),
            0,
        ));
        ensure!(
            !bitmap.0.is_null() && !bits.is_null(),
            "無法配置圖片轉換記憶體"
        );
        let previous = SelectObject(dc.0, bitmap.0);
        let buffer =
            std::slice::from_raw_parts_mut(bits as *mut u8, width as usize * height as usize * 4);
        buffer.fill(255);
        let bounds = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        let played = PlayEnhMetaFile(dc.0, meta.0, &bounds);
        SelectObject(dc.0, previous);
        ensure!(played != 0, "WMF／EMF 繪製失敗");
        for pixel in buffer.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }

        #[cfg(windows)]
        fn wmf_extent(bytes: &[u8]) -> Result<(u32, u32)> {
            ensure!(bytes.len() >= 18, "WMF 標頭不足");
            let mut cursor = 18_usize;
            while cursor + 6 <= bytes.len() {
                let words = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into()?);
                let length = (words as usize)
                    .checked_mul(2)
                    .context("WMF 記錄長度溢位")?;
                ensure!(
                    length >= 6
                        && cursor
                            .checked_add(length)
                            .is_some_and(|end| end <= bytes.len()),
                    "WMF 記錄長度無效"
                );
                let function = u16::from_le_bytes(bytes[cursor + 4..cursor + 6].try_into()?);
                if function == 0x020C && length >= 10 {
                    let height = i16::from_le_bytes(bytes[cursor + 6..cursor + 8].try_into()?)
                        .unsigned_abs() as u32;
                    let width = i16::from_le_bytes(bytes[cursor + 8..cursor + 10].try_into()?)
                        .unsigned_abs() as u32;
                    ensure!(width > 0 && height > 0, "WMF 視窗範圍無效");
                    let scale = 1024.0 / f64::from(width.max(height));
                    return Ok((
                        (f64::from(width) * scale).round().max(1.0) as u32,
                        (f64::from(height) * scale).round().max(1.0) as u32,
                    ));
                }
                if function == 0 {
                    break;
                }
                cursor += length;
            }
            anyhow::bail!(
                "此 WMF 沒有可驗證的長寬比；請改用 placeable WMF／EMF 或 PNG，未以正方形拉伸"
            )
        }
        let rgba = image::RgbaImage::from_raw(width, height, buffer.to_vec())
            .context("轉換點陣長度無效")?;
        Ok(DynamicImage::ImageRgba8(rgba))
    }
}
