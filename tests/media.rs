use calendar_rust::media;

#[test]
fn bitmap_formats_round_trip_with_unicode_paths_and_aspect_ratio() {
    let directory = tempfile::tempdir().unwrap();
    let rgba = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        32,
        16,
        image::Rgba([10, 180, 90, 128]),
    ));
    for (extension, format) in [
        ("png", image::ImageFormat::Png),
        ("bmp", image::ImageFormat::Bmp),
        ("ico", image::ImageFormat::Ico),
        ("jpg", image::ImageFormat::Jpeg),
    ] {
        let path = directory.path().join(format!("來源 空白.{extension}"));
        if extension == "jpg" {
            rgba.to_rgb8().save_with_format(&path, format).unwrap();
        } else {
            rgba.save_with_format(&path, format).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let converted = media::load(&path).unwrap();
        assert_eq!((converted.width, converted.height), (32, 16));
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[cfg(windows)]
#[test]
fn native_wmf_and_emf_convert_to_contained_raster_without_square_fallback() {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{Foundation::RECT, Graphics::Gdi::*};
    let directory = tempfile::tempdir().unwrap();
    unsafe {
        let dc = CreateMetaFileW(null());
        assert!(!dc.is_null());
        assert_ne!(SetMapMode(dc, MM_ANISOTROPIC), 0);
        assert_ne!(SetWindowExtEx(dc, 200, 100, null_mut()), 0);
        assert_ne!(Rectangle(dc, 0, 0, 200, 100), 0);
        let meta = CloseMetaFile(dc);
        assert!(!meta.is_null());
        let size = GetMetaFileBitsEx(meta, 0, null_mut());
        let mut bytes = vec![0; size as usize];
        assert_eq!(
            GetMetaFileBitsEx(meta, size, bytes.as_mut_ptr().cast()),
            size
        );
        DeleteMetaFile(meta);
        let path = directory.path().join("非 placeable.wmf");
        std::fs::write(&path, &bytes).unwrap();
        let image = media::load(&path).unwrap();
        assert_eq!((image.width, image.height), (1024, 512));
        let mut header = Vec::new();
        header.extend(0x9AC6CDD7_u32.to_le_bytes());
        header.extend(0_u16.to_le_bytes());
        for value in [0_i16, 0, 200, 100] {
            header.extend(value.to_le_bytes());
        }
        header.extend(100_u16.to_le_bytes());
        header.extend(0_u32.to_le_bytes());
        let checksum = header
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .fold(0, |sum, word| sum ^ word);
        header.extend(checksum.to_le_bytes());
        header.extend(bytes);
        let path = directory.path().join("placeable.wmf");
        std::fs::write(&path, header).unwrap();
        let image = media::load(&path).unwrap();
        assert_eq!((image.width, image.height), (192, 96));

        let bounds = RECT {
            left: 0,
            top: 0,
            right: 2000,
            bottom: 1000,
        };
        let dc = CreateEnhMetaFileW(null_mut(), null(), &bounds, null());
        assert!(!dc.is_null());
        assert_ne!(Rectangle(dc, 0, 0, 200, 100), 0);
        let meta = CloseEnhMetaFile(dc);
        assert!(!meta.is_null());
        let size = GetEnhMetaFileBits(meta, 0, null_mut());
        let mut bytes = vec![0; size as usize];
        assert_eq!(GetEnhMetaFileBits(meta, size, bytes.as_mut_ptr()), size);
        DeleteEnhMetaFile(meta);
        let path = directory.path().join("原生.emf");
        std::fs::write(&path, bytes).unwrap();
        let image = media::load(&path).unwrap();
        assert!((image.width as f64 / image.height as f64 - 2.0).abs() < 0.1);
        let path = directory.path().join("損壞.wmf");
        std::fs::write(&path, [0_u8; 20]).unwrap();
        assert!(media::load(&path).is_err());
    }
}
