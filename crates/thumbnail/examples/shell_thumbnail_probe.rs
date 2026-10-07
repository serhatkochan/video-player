//! Installed-provider acceptance: shell_thumbnail_probe MEDIA OUTPUT.bmp [DIMENSION].
//! Requires an existing Video Player installation; never changes COM registration or file associations.

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::{io::Write, os::windows::ffi::OsStrExt, path::PathBuf, time::Instant};
    use windows::{
        Win32::{
            Graphics::Gdi::{
                BI_RGB, BITMAP, BITMAPINFO, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
                GetDIBits, GetObjectW, HDC, HGDIOBJ,
            },
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
                    CoInitializeEx, CoUninitialize,
                },
                LibraryLoader::GetModuleHandleW,
            },
            UI::Shell::{
                ISharedBitmap, IShellItem, IThumbnailCache, LocalThumbnailCache,
                SHCreateItemFromParsingName, WTS_CACHEFLAGS, WTS_FORCEEXTRACTION,
                WTS_REQUIRESURROGATE,
            },
        },
        core::{PCWSTR, w},
    };

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "Usage: shell_thumbnail_probe MEDIA OUTPUT.bmp [DIMENSION]"
    );
    let dimension = args
        .get(2)
        .map(|value| value.to_string_lossy().parse::<u32>())
        .transpose()?
        .unwrap_or(256);
    ensure!(
        (1..=1024).contains(&dimension),
        "Probe size must be 1 to 1024"
    );
    let path = PathBuf::from(&args[0]).canonicalize()?;
    let absolute = path.to_string_lossy();
    // The Shell parsing API requires a normal DOS/UNC path rather than Rust's verbatim prefix.
    let shell_path = if let Some(unc) = absolute.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        absolute
            .strip_prefix(r"\\?\")
            .unwrap_or(&absolute)
            .to_owned()
    };
    let wide: Vec<_> = std::ffi::OsStr::new(&shell_path)
        .encode_wide()
        .chain(Some(0))
        .collect();

    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }
        .context("SHCreateItemFromParsingName")?;
    let cache: IThumbnailCache =
        unsafe { CoCreateInstance(&LocalThumbnailCache, None, CLSCTX_INPROC_SERVER) }
            .context("CoCreateInstance(LocalThumbnailCache)")?;
    let mut shared: Option<ISharedBitmap> = None;
    let mut cache_flags = WTS_CACHEFLAGS::default();
    let start = Instant::now();
    unsafe {
        cache.GetThumbnail(
            &item,
            dimension,
            WTS_FORCEEXTRACTION | WTS_REQUIRESURROGATE,
            Some(&mut shared),
            Some(&mut cache_flags),
            None,
        )
    }
    .context("IThumbnailCache::GetThumbnail with a required Windows surrogate")?;
    let elapsed = start.elapsed();
    ensure!(
        unsafe { GetModuleHandleW(w!("video_player_thumbnail.dll")) }.is_err(),
        "The provider loaded in the caller; process isolation was lost"
    );

    let shared = shared.context("Shell returned no bitmap")?;
    // ISharedBitmap owns this handle; retain the interface and do not delete its borrowed bitmap.
    let handle = unsafe { shared.GetSharedBitmap()? };
    let mut metadata = BITMAP::default();
    ensure!(
        unsafe {
            GetObjectW(
                HGDIOBJ(handle.0),
                size_of::<BITMAP>() as i32,
                Some((&mut metadata as *mut BITMAP).cast()),
            )
        } != 0,
        "GetObjectW failed"
    );
    ensure!(
        metadata.bmWidth > 0
            && metadata.bmHeight > 0
            && metadata.bmWidth <= 4096
            && metadata.bmHeight <= 4096,
        "Shell returned an invalid or oversized bitmap"
    );
    let length = metadata.bmWidth as usize * metadata.bmHeight as usize * 4;
    let mut pixels = vec![0u8; length];
    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = size_of_val(&info.bmiHeader) as u32;
    info.bmiHeader.biWidth = metadata.bmWidth;
    info.bmiHeader.biHeight = -metadata.bmHeight;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;
    struct DeviceContext(HDC);
    impl Drop for DeviceContext {
        fn drop(&mut self) {
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
    let dc = DeviceContext(unsafe { CreateCompatibleDC(None) });
    ensure!(!dc.0.is_invalid(), "CreateCompatibleDC failed");
    let lines = unsafe {
        GetDIBits(
            dc.0,
            handle,
            0,
            metadata.bmHeight as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    ensure!(
        lines == metadata.bmHeight,
        "GetDIBits returned incomplete pixels"
    );
    let colored = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0].abs_diff(pixel[1]) > 24 || pixel[1].abs_diff(pixel[2]) > 24)
        .count();
    ensure!(
        colored > pixels.len() / 40,
        "Expected colorful synthetic video content; use a test-pattern fixture"
    );
    let output = PathBuf::from(&args[1]);
    let mut file = std::fs::File::create(&output)?;
    file.write_all(b"BM")?;
    file.write_all(&((54 + length) as u32).to_le_bytes())?;
    file.write_all(&[0; 4])?;
    file.write_all(&54u32.to_le_bytes())?;
    file.write_all(&40u32.to_le_bytes())?;
    file.write_all(&metadata.bmWidth.to_le_bytes())?;
    file.write_all(&(-metadata.bmHeight).to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&32u16.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(&(length as u32).to_le_bytes())?;
    file.write_all(&[0; 16])?;
    file.write_all(&pixels)?;
    println!(
        "PASS: Shell cache {}x{}, {}/{} colored pixels, {:.3}s, flags=0x{:X}, provider outside caller -> {}",
        metadata.bmWidth,
        metadata.bmHeight,
        colored,
        length / 4,
        elapsed.as_secs_f64(),
        cache_flags.0,
        output.display()
    );
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The Shell thumbnail acceptance probe requires Windows.");
    std::process::exit(1);
}
