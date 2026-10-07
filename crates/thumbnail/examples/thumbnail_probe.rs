//! Registration-free acceptance probe: thumbnail_probe DLL MEDIA OUTPUT.bmp [DIMENSION] [--expect-failure]
//! thumbnail_probe --timeout-worker WORKER MEDIA exercises hard timeout and job cleanup without registration.

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::os::windows::ffi::OsStrExt;
    use std::{
        ffi::c_void,
        io::Write,
        path::PathBuf,
        time::{Duration, Instant},
    };
    use video_player_thumbnail::CLSID_THUMBNAIL_PROVIDER;
    use windows::{
        Win32::{
            Foundation::{S_FALSE, S_OK},
            Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW, HBITMAP, HGDIOBJ},
            System::Com::{
                COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, IClassFactory, STGM_READ,
                STGM_SHARE_DENY_NONE,
            },
            UI::Shell::{
                IThumbnailProvider, PropertiesSystem::IInitializeWithStream,
                SHCreateStreamOnFileEx, WTS_ALPHATYPE, WTSAT_RGB,
            },
        },
        core::{GUID, HRESULT, Interface, PCWSTR},
    };

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() >= 3,
        "Usage: thumbnail_probe DLL MEDIA OUTPUT.bmp [DIMENSION] [--expect-failure]\n       thumbnail_probe --timeout-worker WORKER MEDIA"
    );
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _apartment = Apartment;
    let path = PathBuf::from(&args[if args[0] == "--timeout-worker" { 2 } else { 1 }]);
    let path_wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let stream = unsafe {
        SHCreateStreamOnFileEx(
            PCWSTR(path_wide.as_ptr()),
            (STGM_READ | STGM_SHARE_DENY_NONE).0,
            0,
            false,
            None,
        )?
    };
    if args[0] == "--timeout-worker" {
        let start = Instant::now();
        let result = video_player_thumbnail::get_thumbnail_with_worker(
            &stream,
            256,
            &PathBuf::from(&args[1]),
            Duration::from_millis(1000),
        );
        ensure!(
            result.is_err(),
            "Unresponsive worker unexpectedly succeeded"
        );
        ensure!(
            start.elapsed() < Duration::from_secs(3),
            "Worker timeout exceeded its bound"
        );
        println!(
            "PASS: worker terminated after {:.3}s; {}",
            start.elapsed().as_secs_f64(),
            result.unwrap_err()
        );
        return Ok(());
    }
    let dimension = args
        .get(3)
        .filter(|arg| *arg != "--expect-failure")
        .map(|arg| arg.to_string_lossy().parse::<u32>())
        .transpose()?
        .unwrap_or(256);
    let expect_failure = args.iter().any(|arg| arg == "--expect-failure");
    let dll_path = PathBuf::from(&args[0]).canonicalize()?;
    let dll = unsafe { libloading::Library::new(&dll_path)? };
    type GetFactory =
        unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    type CanUnload = unsafe extern "system" fn() -> HRESULT;
    let get_factory = unsafe { *dll.get::<GetFactory>(b"DllGetClassObject\0")? };
    let can_unload = unsafe { *dll.get::<CanUnload>(b"DllCanUnloadNow\0")? };
    ensure!(
        unsafe { can_unload() } == S_OK,
        "DLL unexpectedly retained objects before probe"
    );
    let mut raw = std::ptr::null_mut();
    unsafe {
        get_factory(&CLSID_THUMBNAIL_PROVIDER, &IClassFactory::IID, &mut raw).ok()?;
    }
    let factory = unsafe { IClassFactory::from_raw(raw) };
    ensure!(
        unsafe { can_unload() } == S_FALSE,
        "Factory failed to pin its DLL"
    );
    let provider: IThumbnailProvider = unsafe { factory.CreateInstance(None)? };
    let initializer: IInitializeWithStream = provider.cast()?;
    unsafe {
        initializer.Initialize(&stream, STGM_READ.0)?;
    }
    ensure!(
        unsafe { initializer.Initialize(&stream, STGM_READ.0) }.is_err(),
        "Repeated initialization should be rejected"
    );
    let mut bitmap = HBITMAP::default();
    let mut alpha = WTS_ALPHATYPE(0);
    let start = Instant::now();
    let result = unsafe { provider.GetThumbnail(dimension, &mut bitmap, &mut alpha) };
    if expect_failure {
        ensure!(
            result.is_err(),
            "Invalid media unexpectedly produced a thumbnail"
        );
        ensure!(bitmap.is_invalid(), "Failed thumbnail retained a bitmap");
        ensure!(
            start.elapsed() < Duration::from_secs(12),
            "Corrupt media processing exceeded timeout"
        );
        println!(
            "PASS: invalid media rejected after {:.3}s: {}",
            start.elapsed().as_secs_f64(),
            result.unwrap_err()
        );
    } else {
        result.context("COM GetThumbnail")?;
        struct BitmapGuard(HBITMAP);
        impl Drop for BitmapGuard {
            fn drop(&mut self) {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(self.0.0));
                }
            }
        }
        let _bitmap = BitmapGuard(bitmap);
        let mut metadata = BITMAP::default();
        let count = unsafe {
            GetObjectW(
                HGDIOBJ(bitmap.0),
                size_of::<BITMAP>() as i32,
                Some((&mut metadata as *mut BITMAP).cast()),
            )
        };
        ensure!(
            count > 0
                && metadata.bmWidth > 0
                && metadata.bmHeight > 0
                && metadata.bmBitsPixel == 32
                && !metadata.bmBits.is_null(),
            "COM returned an invalid DIB"
        );
        ensure!(
            metadata.bmWidth as u32 <= dimension && metadata.bmHeight as u32 <= dimension,
            "Thumbnail exceeded requested dimensions"
        );
        ensure!(alpha == WTSAT_RGB, "Unexpected alpha mode");
        let length = metadata.bmWidthBytes as usize * metadata.bmHeight as usize;
        let pixels = unsafe { std::slice::from_raw_parts(metadata.bmBits.cast::<u8>(), length) };
        ensure!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255),
            "Thumbnail alpha is not opaque"
        );
        ensure!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] > 8 || pixel[1] > 8 || pixel[2] > 8),
            "Thumbnail is completely black"
        );
        let output = PathBuf::from(&args[2]);
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
        file.write_all(pixels)?;
        println!(
            "PASS: {}x{} opaque BGRA thumbnail in {:.3}s -> {}",
            metadata.bmWidth,
            metadata.bmHeight,
            start.elapsed().as_secs_f64(),
            output.display()
        );
    }
    drop(initializer);
    drop(provider);
    drop(factory);
    ensure!(
        unsafe { can_unload() } == S_OK,
        "DLL leaked COM references after probe"
    );
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The thumbnail provider requires Windows.");
}
