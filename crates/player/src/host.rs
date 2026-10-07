//! Native video surface. Keep the HWND alive until `Mpv` has been dropped.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::{Context, Result, bail};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, POINTL, WPARAM},
        Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH},
        System::{
            Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, TYMED_HGLOBAL},
            LibraryLoader::GetModuleHandleW,
            Ole::{
                CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget,
                IDropTarget_Impl, OleInitialize, OleUninitialize, RegisterDragDrop,
                ReleaseStgMedium, RevokeDragDrop,
            },
            SystemServices::MODIFIERKEYS_FLAGS,
        },
        UI::{
            Shell::{DragQueryFileW, HDROP},
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, FindWindowExW, GWLP_USERDATA, GetWindowLongPtrW, IDC_ARROW,
                IsWindow, LoadCursorW, MSG, PM_REMOVE, PeekMessageW, PostMessageW, PostQuitMessage,
                RegisterClassW, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOZORDER,
                SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WM_CHAR, WM_CLOSE, WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
                WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCCREATE, WM_QUIT, WNDCLASSW, WS_CHILD,
                WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_OVERLAPPEDWINDOW,
            },
        },
    },
    core::{Error, Ref, implement, w},
};

#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    TogglePause,
    ToggleFullscreen,
    ExitFullscreen,
    ShowControls,
    Seek(f64),
    OpenFile,
    DropFile(PathBuf),
}

struct HostState {
    parent: HWND,
    events: Arc<Mutex<VecDeque<HostEvent>>>,
}

pub struct VideoHost {
    window: HWND,
    _state: Box<HostState>,
    drop_target: IDropTarget,
    drop_child: RefCell<Option<HWND>>,
    _ole: OleApartment,
}

impl VideoHost {
    /// `parent == 0` creates a standalone test window. GUI callers pass the
    /// eframe HWND and keep this surface inside their video content rectangle.
    pub fn new(parent: isize) -> Result<Self> {
        register_class()?;
        let ole = OleApartment::new()?;
        let instance = unsafe { GetModuleHandleW(None) }.context("Get application module")?;
        let parent = HWND(parent as *mut _);
        let mut state = Box::new(HostState {
            parent,
            events: Arc::new(Mutex::new(VecDeque::new())),
        });
        let style = if parent.is_invalid() {
            WS_OVERLAPPEDWINDOW
        } else {
            WS_CHILD
        } | WS_CLIPCHILDREN
            | WS_CLIPSIBLINGS;
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("VideoPlayer.VideoSurface.1"),
                w!("Video Player"),
                style,
                0,
                0,
                960,
                540,
                if parent.is_invalid() {
                    None
                } else {
                    Some(parent)
                },
                None,
                Some(HINSTANCE(instance.0)),
                Some((&mut *state as *mut HostState).cast()),
            )
        }
        .context("Create the native video surface")?;
        let drop_target: IDropTarget = FileDropTarget {
            events: Arc::clone(&state.events),
            accepted: Cell::new(false),
        }
        .into();
        if let Err(error) = unsafe { RegisterDragDrop(window, &drop_target) } {
            unsafe {
                let _ = DestroyWindow(window);
            }
            return Err(error).context("Register native file drop target");
        }
        Ok(Self {
            window,
            _state: state,
            drop_target,
            drop_child: RefCell::new(None),
            _ole: ole,
        })
    }

    pub fn hwnd(&self) -> isize {
        self.window.0 as isize
    }

    pub fn place(&self, x: i32, y: i32, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            self.show(false);
            return;
        }
        unsafe {
            let _ = SetWindowPos(
                self.window,
                None,
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// Hide the native surface while egui draws dialogs/menus that overlap it;
    /// Windows child HWNDs compose above the parent's GPU content.
    pub fn show(&self, visible: bool) {
        unsafe {
            let _ = ShowWindow(self.window, if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }

    pub fn poll_event(&self) -> Option<HostEvent> {
        self.sync_drop_target();
        self._state
            .events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    pub fn closed(&self) -> bool {
        !unsafe { IsWindow(Some(self.window)) }.as_bool()
    }

    pub fn native_drop_target_ready(&self) -> bool {
        self.drop_child
            .borrow()
            .is_some_and(|window| unsafe { IsWindow(Some(window)) }.as_bool())
    }

    fn sync_drop_target(&self) {
        // mpv registers its own target even in embedding mode. Replace it so
        // every drop follows the application's local-file validation/resume path.
        let child = unsafe { FindWindowExW(Some(self.window), None, w!("mpv"), None) }.ok();
        if let Some(child) = child {
            let mut registered = self.drop_child.borrow_mut();
            if *registered != Some(child) {
                unsafe {
                    let _ = RevokeDragDrop(child);
                    if RegisterDragDrop(child, &self.drop_target).is_ok() {
                        *registered = Some(child);
                    }
                }
            }
        } else {
            *self.drop_child.borrow_mut() = None;
        }
    }
}

impl Drop for VideoHost {
    fn drop(&mut self) {
        unsafe {
            if let Some(child) = self.drop_child.take() {
                let _ = RevokeDragDrop(child);
            }
            let _ = RevokeDragDrop(self.window);
        }
        if !self.closed() {
            unsafe {
                let _ = DestroyWindow(self.window);
            }
        }
    }
}

struct OleApartment;
impl OleApartment {
    fn new() -> Result<Self> {
        unsafe { OleInitialize(None) }.context("Initialize native drag and drop")?;
        Ok(Self)
    }
}
impl Drop for OleApartment {
    fn drop(&mut self) {
        unsafe {
            OleUninitialize();
        }
    }
}

fn file_drop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
        ..Default::default()
    }
}

struct Medium(STGMEDIUM);
impl Drop for Medium {
    fn drop(&mut self) {
        unsafe {
            ReleaseStgMedium(&mut self.0);
        }
    }
}

#[implement(IDropTarget)]
struct FileDropTarget {
    events: Arc<Mutex<VecDeque<HostEvent>>>,
    accepted: Cell<bool>,
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for FileDropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.accepted
            .set(data.as_ref().and_then(dropped_path).is_some());
        self.apply_effect(effect)
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        _point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.apply_effect(effect)
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.accepted.set(false);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        if effect.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe {
            *effect = DROPEFFECT_NONE;
        }
        self.accepted.set(false);
        let Some(data) = data.as_ref() else {
            return Ok(());
        };
        // CF_HDROP is the only accepted representation; URL and text drops
        // cannot invoke libmpv's network/protocol handling.
        if let Some(path) = dropped_path(data) {
            let mut events = self
                .events
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if events.len() < 128 {
                events.push_back(HostEvent::DropFile(path));
                unsafe {
                    *effect = DROPEFFECT_COPY;
                }
            }
        }
        Ok(())
    }
}

impl FileDropTarget_Impl {
    fn apply_effect(&self, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        if effect.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe {
            *effect = if self.accepted.get() && effect.read().0 & DROPEFFECT_COPY.0 != 0 {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            };
        }
        Ok(())
    }
}

fn dropped_path(data: &IDataObject) -> Option<PathBuf> {
    let medium = Medium(unsafe { data.GetData(&file_drop_format()) }.ok()?);
    if medium.0.tymed != TYMED_HGLOBAL.0 as u32 {
        return None;
    }
    first_dropped_file(HDROP(unsafe { medium.0.u.hGlobal }.0))
}

fn first_dropped_file(drop: HDROP) -> Option<PathBuf> {
    if drop.is_invalid() || unsafe { DragQueryFileW(drop, u32::MAX, None) } == 0 {
        return None;
    }
    let length = unsafe { DragQueryFileW(drop, 0, None) } as usize;
    if length == 0 || length > 32767 {
        return None;
    }
    let mut buffer = vec![0u16; length + 1];
    if unsafe { DragQueryFileW(drop, 0, Some(&mut buffer)) } as usize != length {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..length],
    )))
}

fn register_class() -> Result<()> {
    static REGISTERED: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    match REGISTERED.get_or_init(|| unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(instance.0),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            lpszClassName: w!("VideoPlayer.VideoSurface.1"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread().to_string());
        }
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(message) => bail!("Register video surface: {message}"),
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const HostState;
        if let Some(state) = pointer.as_ref() {
            let interaction = match message {
                WM_MOUSEMOVE => Some(HostEvent::ShowControls),
                WM_LBUTTONDOWN => Some(HostEvent::TogglePause),
                WM_LBUTTONDBLCLK => Some(HostEvent::ToggleFullscreen),
                WM_KEYDOWN => match wparam.0 {
                    0x20 => Some(HostEvent::TogglePause),
                    0x46 => Some(HostEvent::ToggleFullscreen),
                    0x1b => Some(HostEvent::ExitFullscreen),
                    0x25 => Some(HostEvent::Seek(-5.0)),
                    0x27 => Some(HostEvent::Seek(5.0)),
                    _ => None,
                },
                _ => None,
            };
            if let Some(event) = interaction {
                let mut queue = state.events.lock().unwrap_or_else(|e| e.into_inner());
                if queue.len() < 128
                    && !(event == HostEvent::ShowControls && queue.back() == Some(&event))
                {
                    queue.push_back(event);
                }
                return LRESULT(0);
            }
            // Preserve keyboard input for the egui window when the empty
            // video surface has focus. Only scalar keyboard messages cross.
            if !state.parent.is_invalid() && matches!(message, WM_KEYDOWN | WM_KEYUP | WM_CHAR) {
                let _ = PostMessageW(Some(state.parent), message, wparam, lparam);
                return LRESULT(0);
            }
            if message == WM_CLOSE && state.parent.is_invalid() {
                let _ = DestroyWindow(window);
                return LRESULT(0);
            }
        }
        if message == WM_ERASEBKGND {
            // The native mpv child supplies video pixels; avoid flicker during resize.
            return LRESULT(1);
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

/// Used by the standalone decoder probe and while shutting down mpv, whose
/// native video thread may be waiting for a GUI-thread window message.
pub fn pump_messages() {
    unsafe {
        let mut message = MSG::default();
        for _ in 0..128 {
            if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                break;
            }
            if message.message == WM_QUIT {
                PostQuitMessage(message.wParam.0 as i32);
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{
        Foundation::POINT,
        System::{
            Com::STGMEDIUM_0,
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
        UI::Shell::{DROPFILES, SHCreateDataObject},
    };

    unsafe fn data_object(format: u16, payload: &[u8]) -> IDataObject {
        unsafe {
            let object: IDataObject = SHCreateDataObject(None, None, None).unwrap();
            let memory = GlobalAlloc(GMEM_MOVEABLE, payload.len()).unwrap();
            let pointer = GlobalLock(memory);
            assert!(!pointer.is_null());
            std::ptr::copy_nonoverlapping(payload.as_ptr(), pointer.cast::<u8>(), payload.len());
            let _ = GlobalUnlock(memory);
            let medium = STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: memory },
                ..Default::default()
            };
            let description = FORMATETC {
                cfFormat: format,
                ..file_drop_format()
            };
            object.SetData(&description, &medium, true).unwrap();
            object
        }
    }

    #[test]
    fn native_drop_routes_unicode_files_and_rejects_url_data() {
        let _ole = OleApartment::new().unwrap();
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let target: IDropTarget = FileDropTarget {
            events: Arc::clone(&queue),
            accepted: Cell::new(false),
        }
        .into();
        let path = r"C:\Türkçe\İstanbul; $video.mkv";
        let header = DROPFILES {
            pFiles: size_of::<DROPFILES>() as u32,
            pt: POINT::default(),
            fNC: false.into(),
            fWide: true.into(),
        };
        let mut payload = unsafe {
            std::slice::from_raw_parts(
                (&header as *const DROPFILES).cast::<u8>(),
                size_of::<DROPFILES>(),
            )
        }
        .to_vec();
        for character in path.encode_utf16().chain([0, 0]) {
            payload.extend(character.to_le_bytes());
        }
        unsafe {
            let file = data_object(CF_HDROP.0, &payload);
            let mut effect = DROPEFFECT_COPY;
            target
                .DragEnter(&file, MODIFIERKEYS_FLAGS(0), POINTL::default(), &mut effect)
                .unwrap();
            assert_eq!(effect, DROPEFFECT_COPY);
            target
                .Drop(&file, MODIFIERKEYS_FLAGS(0), POINTL::default(), &mut effect)
                .unwrap();
            assert_eq!(
                queue.lock().unwrap().pop_front(),
                Some(HostEvent::DropFile(PathBuf::from(path)))
            );
            let text: Vec<u8> = "https://example.com/movie.mp4\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            let url = data_object(13, &text);
            effect = DROPEFFECT_COPY;
            target
                .DragEnter(&url, MODIFIERKEYS_FLAGS(0), POINTL::default(), &mut effect)
                .unwrap();
            assert_eq!(effect, DROPEFFECT_NONE);
            target
                .Drop(&url, MODIFIERKEYS_FLAGS(0), POINTL::default(), &mut effect)
                .unwrap();
            assert_eq!(effect, DROPEFFECT_NONE);
            assert!(queue.lock().unwrap().is_empty());
        }
    }
}
