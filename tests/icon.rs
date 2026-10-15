#![cfg(windows)]

use anyhow::{Result, ensure};
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::LibraryLoader::{
        FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW,
        LoadResource, LockResource, SizeofResource,
    },
};

struct Resources(HMODULE);

impl Resources {
    fn get(&self, name: u16, kind: u16) -> Result<&[u8]> {
        unsafe {
            let resource = FindResourceW(
                self.0,
                usize::from(name) as *const u16,
                usize::from(kind) as *const u16,
            );
            ensure!(
                !resource.is_null(),
                "Missing resource {kind}/{name}: {}",
                std::io::Error::last_os_error()
            );
            let size = SizeofResource(self.0, resource);
            let loaded = LoadResource(self.0, resource);
            ensure!(!loaded.is_null(), "Failed to load resource {kind}/{name}");
            let bytes = LockResource(loaded);
            ensure!(!bytes.is_null() && size > 0, "Empty resource {kind}/{name}");
            // Resource memory remains valid until this module is unloaded.
            Ok(std::slice::from_raw_parts(bytes.cast(), size as usize))
        }
    }
}

impl Drop for Resources {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}

#[test]
fn executable_icon_contains_all_sizes_from_source_png() -> Result<()> {
    let executable: Vec<u16> = OsStr::new(env!("CARGO_BIN_EXE_CalendarRust"))
        .encode_wide()
        .chain(Some(0))
        .collect();
    let module = unsafe {
        LoadLibraryExW(
            executable.as_ptr(),
            null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    };
    ensure!(
        !module.is_null(),
        "Cannot read executable resources: {}",
        std::io::Error::last_os_error()
    );
    let resources = Resources(module);
    let sizes = [16_u32, 24, 32, 48, 64, 128, 256];
    let group = resources.get(1, 14)?;
    assert_eq!(group.len(), 6 + 14 * sizes.len());
    assert_eq!(&group[..4], &[0, 0, 1, 0]);
    assert_eq!(
        usize::from(u16::from_le_bytes([group[4], group[5]])),
        sizes.len()
    );
    let source =
        image::load_from_memory(include_bytes!("..\\assets\\icons\\512.png"))?.into_rgba8();
    for (entry, size) in group[6..].chunks_exact(14).zip(sizes) {
        let dimension = if size == 256 { 0 } else { size as u8 };
        assert_eq!((entry[0], entry[1]), (dimension, dimension));
        assert_eq!(u16::from_le_bytes([entry[6], entry[7]]), 32);
        let name = u16::from_le_bytes([entry[12], entry[13]]);
        let png = resources.get(name, 3)?;
        assert_eq!(
            png.len(),
            u32::from_le_bytes(entry[8..12].try_into()?) as usize
        );
        let actual = image::load_from_memory(png)?.into_rgba8();
        let expected =
            image::imageops::resize(&source, size, size, image::imageops::FilterType::Lanczos3);
        assert_eq!(actual, expected, "EXE icon differs at {size}px");
    }
    Ok(())
}
