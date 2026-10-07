#![allow(non_snake_case)]

use crate::broker;
use std::{
    cell::RefCell,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use windows::{
    Win32::{
        Foundation::{
            CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_POINTER,
            S_FALSE, S_OK,
        },
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, HBITMAP,
        },
        System::{
            Com::{IClassFactory, IClassFactory_Impl, IStream},
            Diagnostics::Debug::OutputDebugStringW,
            LibraryLoader::{
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
                GetModuleHandleExW,
            },
        },
        UI::Shell::{
            IThumbnailProvider, IThumbnailProvider_Impl,
            PropertiesSystem::{IInitializeWithStream, IInitializeWithStream_Impl},
            WTS_ALPHATYPE, WTSAT_RGB,
        },
    },
    core::{BOOL, Error, GUID, HRESULT, IUnknown, Interface, Ref, Result, implement},
};

pub const CLSID_THUMBNAIL_PROVIDER: GUID = GUID::from_u128(0x8c1d9ed4_6900_4d31_9ebb_570623a87973);
static OBJECTS: AtomicUsize = AtomicUsize::new(0);
static LOCKS: AtomicUsize = AtomicUsize::new(0);

struct ComLifetime;
impl ComLifetime {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}
impl Drop for ComLifetime {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[implement(IInitializeWithStream, IThumbnailProvider)]
struct ThumbnailProvider {
    stream: RefCell<Option<IStream>>,
    _lifetime: ComLifetime,
}

impl IInitializeWithStream_Impl for ThumbnailProvider_Impl {
    fn Initialize(&self, pstream: Ref<'_, IStream>, _grfmode: u32) -> Result<()> {
        let stream = pstream
            .as_ref()
            .ok_or_else(|| Error::from_hresult(E_POINTER))?;
        let mut slot = self
            .stream
            .try_borrow_mut()
            .map_err(|_| Error::from_hresult(E_FAIL))?;
        if slot.is_some() {
            return Err(Error::from_hresult(HRESULT(0x800704df_u32 as i32)));
        }
        *slot = Some(stream.clone());
        Ok(())
    }
}

impl IThumbnailProvider_Impl for ThumbnailProvider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdwalpha: *mut WTS_ALPHATYPE,
    ) -> Result<()> {
        if phbmp.is_null() || pdwalpha.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe {
            *phbmp = HBITMAP::default();
            *pdwalpha = WTS_ALPHATYPE(0);
        }
        if cx == 0 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        // Shell may request a larger source bitmap than the displayed thumbnail.
        let dimension = cx.min(crate::MAX_DIMENSION);
        catch_unwind(AssertUnwindSafe(|| {
            let slot = self
                .stream
                .try_borrow()
                .map_err(|_| Error::from_hresult(E_FAIL))?;
            let stream = slot.as_ref().ok_or_else(|| Error::from_hresult(E_FAIL))?;
            let image = broker::get_thumbnail(stream, dimension, &module_directory()?).map_err(
                |error| {
                    let message = format!("{error:#}");
                    diagnostic(&message);
                    Error::new(E_FAIL, message)
                },
            )?;
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: image.width as i32,
                    biHeight: -(image.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: image.pixels.len() as u32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap =
                unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
            if bits.is_null() {
                return Err(Error::from_hresult(E_FAIL));
            }
            unsafe {
                std::ptr::copy_nonoverlapping(
                    image.pixels.as_ptr(),
                    bits.cast::<u8>(),
                    image.pixels.len(),
                );
                *phbmp = bitmap;
                *pdwalpha = WTSAT_RGB;
            }
            Ok(())
        }))
        .unwrap_or_else(|_| Err(Error::from_hresult(E_FAIL)))
    }
}

fn diagnostic(message: &str) {
    let wide: Vec<u16> = format!("[Video Player thumbnail] {message}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe { OutputDebugStringW(windows::core::PCWSTR(wide.as_ptr())) };
}

#[implement(IClassFactory)]
struct ClassFactory {
    _lifetime: ComLifetime,
}
impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<'_, IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        if ppvobject.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe {
            *ppvobject = std::ptr::null_mut();
        }
        if riid.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        if punkouter.as_ref().is_some() {
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        let provider: IThumbnailProvider = ThumbnailProvider {
            stream: RefCell::new(None),
            _lifetime: ComLifetime::new(),
        }
        .into();
        unsafe {
            (provider.vtable().base__.QueryInterface)(provider.as_raw(), riid, ppvobject).ok()
        }
    }
    fn LockServer(&self, flock: BOOL) -> Result<()> {
        if flock.as_bool() {
            LOCKS.fetch_add(1, Ordering::SeqCst);
        } else if LOCKS
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                count.checked_sub(1)
            })
            .is_err()
        {
            return Err(Error::from_hresult(E_FAIL));
        }
        Ok(())
    }
}

/// Returns an owned COM interface reference; querying adds a reference before the temporary factory is dropped.
///
/// # Safety
/// Non-null GUID pointers must identify readable, aligned GUID values and `ppv` must be writable.
/// The caller must release the returned interface using COM `Release`; null inputs are rejected.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = std::ptr::null_mut();
    }
    if rclsid.is_null() || riid.is_null() {
        return E_POINTER;
    }
    if unsafe { *rclsid } != CLSID_THUMBNAIL_PROVIDER {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let factory: IClassFactory = ClassFactory {
            _lifetime: ComLifetime::new(),
        }
        .into();
        unsafe { (factory.vtable().base__.QueryInterface)(factory.as_raw(), riid, ppv) }
    }))
    .unwrap_or(E_FAIL)
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::SeqCst) == 0 && LOCKS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

fn module_directory() -> Result<PathBuf> {
    let mut module = Default::default();
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            windows::core::PCWSTR(DllGetClassObject as *const () as *const u16),
            &mut module,
        )?;
        let mut path = vec![0u16; 32768];
        let count = GetModuleFileNameW(Some(module), &mut path) as usize;
        if count == 0 || count >= path.len() {
            return Err(Error::from_hresult(E_FAIL));
        }
        use std::os::windows::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_wide(&path[..count]));
        path.parent()
            .map(PathBuf::from)
            .ok_or_else(|| Error::from_hresult(E_FAIL))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn factory_holds_module_until_all_references_and_locks_are_released() {
        unsafe {
            assert_eq!(DllCanUnloadNow(), S_OK);
            let mut raw = std::ptr::null_mut();
            DllGetClassObject(&CLSID_THUMBNAIL_PROVIDER, &IClassFactory::IID, &mut raw)
                .ok()
                .unwrap();
            let factory = IClassFactory::from_raw(raw);
            assert_eq!(DllCanUnloadNow(), S_FALSE);
            let provider: IThumbnailProvider = factory.CreateInstance(None).unwrap();
            let initializer: IInitializeWithStream = provider.cast().unwrap();
            drop(initializer);
            factory.LockServer(true).unwrap();
            drop(provider);
            factory.LockServer(false).unwrap();
            drop(factory);
            assert_eq!(DllCanUnloadNow(), S_OK);
        }
    }
}
