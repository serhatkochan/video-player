//! Dynamic libmpv client API. ABI: https://github.com/mpv-player/mpv/blob/master/include/mpv/client.h

use std::{
    collections::VecDeque,
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::Path,
    ptr,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, anyhow, bail};
use libloading::Library;

use crate::host::HostEvent;

#[derive(Debug, Clone, Default)]
pub struct Track {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub lang: String,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct PlaybackSnapshot {
    pub paused: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f64,
    pub mute: bool,
    pub speed: f64,
    pub subtitle_delay: f64,
    pub tracks: Vec<Track>,
    pub path: Option<String>,
    pub eof: bool,
    pub idle: bool,
    pub hwdec: String,
    pub video_codec: String,
    pub output_colorspace: String,
    pub error: Option<String>,
}

impl Default for PlaybackSnapshot {
    fn default() -> Self {
        Self {
            paused: true,
            position: 0.0,
            duration: 0.0,
            volume: 100.0,
            mute: false,
            speed: 1.0,
            subtitle_delay: 0.0,
            tracks: Vec::new(),
            path: None,
            eof: false,
            idle: true,
            hwdec: String::new(),
            video_codec: String::new(),
            output_colorspace: String::new(),
            error: None,
        }
    }
}

enum Request {
    Command(Vec<CString>),
    Property(CString, CString),
    Shutdown,
}

/// Commands return after enqueueing; asynchronous failures appear in `snapshot().error`.
pub struct Mpv {
    requests: mpsc::Sender<Request>,
    state: Arc<Mutex<PlaybackSnapshot>>,
    interactions: mpsc::Receiver<HostEvent>,
    worker: Option<JoinHandle<()>>,
    runtime_configuration: String,
}

impl Mpv {
    pub fn new(host: isize, runtime: &Path) -> Result<Self> {
        if host <= 0 {
            bail!("A valid native video window is required");
        }
        let runtime = runtime.to_path_buf();
        let state = Arc::new(Mutex::new(PlaybackSnapshot::default()));
        let worker_state = Arc::clone(&state);
        let (requests, receiver) = mpsc::channel();
        let (events, interactions) = mpsc::sync_channel(128);
        let (ready, startup) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("video-player-mpv".into())
            .spawn(move || {
                let engine = match Engine::new(host, &runtime) {
                    Ok(engine) => engine,
                    Err(error) => {
                        let _ = ready.send(Err(format!("{error:#}")));
                        return;
                    }
                };
                let configuration = engine.runtime_configuration();
                let _ = ready.send(Ok(configuration));
                engine.run(receiver, worker_state, events);
            })
            .context("Could not start playback thread")?;
        match startup.recv() {
            Ok(Ok(runtime_configuration)) => Ok(Self {
                requests,
                state,
                interactions,
                worker: Some(worker),
                runtime_configuration,
            }),
            result => {
                let _ = worker.join();
                Err(anyhow!(match result {
                    Ok(Err(message)) => message,
                    _ => "Playback initialization thread stopped".into(),
                }))
            }
        }
    }

    pub fn command(&self, args: &[&str]) -> Result<()> {
        if args.is_empty() {
            bail!("An mpv command cannot be empty");
        }
        let args = args
            .iter()
            .map(|arg| CString::new(*arg).context("Command contains a NUL byte"))
            .collect::<Result<Vec<_>>>()?;
        self.requests
            .send(Request::Command(args))
            .context("Playback thread is unavailable")
    }

    pub fn set_property(&self, name: &str, value: &str) -> Result<()> {
        let name = CString::new(name).context("Property name contains a NUL byte")?;
        let value = CString::new(value).context("Property value contains a NUL byte")?;
        self.requests
            .send(Request::Property(name, value))
            .context("Playback thread is unavailable")
    }

    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn poll_event(&self) -> Option<HostEvent> {
        self.interactions.try_recv().ok()
    }

    pub fn runtime_configuration(&self) -> &str {
        &self.runtime_configuration
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Shutdown);
        // Destroy libmpv before its embedding HWND. Its window thread still needs
        // the GUI thread to process cross-thread Win32 messages during teardown.
        if let Some(worker) = self.worker.take() {
            while !worker.is_finished() {
                crate::host::pump_messages();
                thread::sleep(std::time::Duration::from_millis(2));
            }
            let _ = worker.join();
        }
    }
}

const STRING: c_int = 1;
const FLAG: c_int = 3;
const INT64: c_int = 4;
const DOUBLE: c_int = 5;
const NODE: c_int = 6;
const NODE_ARRAY: c_int = 7;
const NODE_MAP: c_int = 8;

#[repr(C)]
struct Event {
    id: c_int,
    error: c_int,
    userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct EventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
}

#[repr(C)]
struct ClientMessage {
    count: c_int,
    args: *const *const c_char,
}

#[repr(C)]
struct LogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    log_level: c_int,
}

#[repr(C)]
union NodeValue {
    string: *const c_char,
    flag: c_int,
    int64: i64,
    double: f64,
    list: *const NodeList,
}

#[repr(C)]
struct Node {
    value: NodeValue,
    format: c_int,
}

#[repr(C)]
struct NodeList {
    count: c_int,
    values: *const Node,
    keys: *const *const c_char,
}

type Create = unsafe extern "C" fn() -> *mut c_void;
type Initialize = unsafe extern "C" fn(*mut c_void) -> c_int;
type Destroy = unsafe extern "C" fn(*mut c_void);
type SetOption = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type Command = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int;
type CommandAsync = unsafe extern "C" fn(*mut c_void, u64, *const *const c_char) -> c_int;
type SetPropertyAsync =
    unsafe extern "C" fn(*mut c_void, u64, *const c_char, c_int, *mut c_void) -> c_int;
type Observe = unsafe extern "C" fn(*mut c_void, u64, *const c_char, c_int) -> c_int;
type WaitEvent = unsafe extern "C" fn(*mut c_void, f64) -> *const Event;
type ErrorString = unsafe extern "C" fn(c_int) -> *const c_char;
type RequestLogs = unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int;
type ApiVersion = unsafe extern "C" fn() -> u32;
type GetPropertyString = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
type Free = unsafe extern "C" fn(*mut c_void);

struct Api {
    create: Create,
    initialize: Initialize,
    destroy: Destroy,
    set_option: SetOption,
    command: Command,
    command_async: CommandAsync,
    set_property_async: SetPropertyAsync,
    observe: Observe,
    wait_event: WaitEvent,
    error_string: ErrorString,
    request_logs: RequestLogs,
    get_property_string: GetPropertyString,
    free: Free,
    _library: Library,
}

impl Api {
    fn load(runtime: &Path) -> Result<Self> {
        let library_path = ["libmpv-2.dll", "mpv-2.dll", "libmpv.dll"]
            .into_iter()
            .map(|name| runtime.join(name))
            .find(|path| path.is_file())
            .ok_or_else(|| {
                anyhow!(
                    "libmpv-2.dll is missing from {}. Install the bundled LGPL media runtime.",
                    runtime.display()
                )
            })?
            .canonicalize()
            .context("Could not resolve media runtime path")?;
        // Absolute path + DLL directory/System32 search excludes the working
        // directory and PATH. https://learn.microsoft.com/windows/win32/dlls/dynamic-link-library-security
        let library: Library = unsafe {
            libloading::os::windows::Library::load_with_flags(&library_path, 0x100 | 0x800)
                .context("Could not load the media runtime or its dependencies")?
                .into()
        };
        unsafe {
            let version = *library.get::<ApiVersion>(b"mpv_client_api_version\0")?;
            if version() >> 16 != 2 {
                bail!("The media runtime must expose libmpv client API version 2");
            }
            Ok(Self {
                create: *library.get(b"mpv_create\0")?,
                initialize: *library.get(b"mpv_initialize\0")?,
                destroy: *library.get(b"mpv_terminate_destroy\0")?,
                set_option: *library.get(b"mpv_set_option_string\0")?,
                command: *library.get(b"mpv_command\0")?,
                command_async: *library.get(b"mpv_command_async\0")?,
                set_property_async: *library.get(b"mpv_set_property_async\0")?,
                observe: *library.get(b"mpv_observe_property\0")?,
                wait_event: *library.get(b"mpv_wait_event\0")?,
                error_string: *library.get(b"mpv_error_string\0")?,
                request_logs: *library.get(b"mpv_request_log_messages\0")?,
                get_property_string: *library.get(b"mpv_get_property_string\0")?,
                free: *library.get(b"mpv_free\0")?,
                _library: library,
            })
        }
    }

    fn check(&self, code: c_int, operation: &str) -> Result<()> {
        if code < 0 {
            bail!("{operation}: {}", unsafe {
                c_string((self.error_string)(code))
            });
        }
        Ok(())
    }
}

struct Engine {
    api: Api,
    handle: *mut c_void,
}

impl Engine {
    fn new(host: isize, runtime: &Path) -> Result<Self> {
        let api = Api::load(runtime)?;
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            bail!("libmpv could not allocate a playback context");
        }
        let engine = Self { api, handle };
        for (name, value) in [
            ("config", "no"),
            ("load-scripts", "no"),
            ("ytdl", "no"),
            ("access-references", "no"),
            ("terminal", "no"),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("osc", "no"),
            ("osd-level", "0"),
            ("input-default-bindings", "no"),
            ("drag-and-drop", "no"),
            ("input-vo-keyboard", "yes"),
            ("input-cursor", "yes"),
            ("input-media-keys", "yes"),
            ("vo", "gpu-next"),
            ("gpu-api", "d3d11"),
            ("gpu-context", "d3d11"),
            ("hwdec", "auto-safe"),
            ("hwdec-software-fallback", "yes"),
            ("target-colorspace-hint", "auto"),
        ] {
            engine.option(name, value)?;
        }
        engine.option("wid", &host.to_string())?;
        unsafe {
            engine
                .api
                .check((engine.api.initialize)(handle), "Initialize playback")?;
            (engine.api.request_logs)(handle, c"error".as_ptr());
        }
        for (id, name, format) in [
            (1, "pause", FLAG),
            (2, "time-pos", DOUBLE),
            (3, "duration", DOUBLE),
            (4, "volume", DOUBLE),
            (5, "mute", FLAG),
            (6, "speed", DOUBLE),
            (7, "sub-delay", DOUBLE),
            (8, "track-list", NODE),
            (9, "path", STRING),
            (10, "eof-reached", FLAG),
            (11, "idle-active", FLAG),
            (12, "hwdec-current", STRING),
            (13, "current-tracks/video/codec", STRING),
            (14, "video-target-params", NODE),
        ] {
            let property = CString::new(name)?;
            unsafe {
                engine.api.check(
                    (engine.api.observe)(handle, id, property.as_ptr(), format),
                    &format!("Observe {name}"),
                )?;
            }
        }
        // The embedded Win32 VO owns keyboard focus. Route its input back to
        // the application, including mouse movement used to reveal controls.
        // https://mpv.io/manual/master/#command-interface-keybind
        for (key, message) in [
            ("SPACE", "vp-toggle-pause"),
            ("MBTN_LEFT", "vp-toggle-pause"),
            ("MBTN_LEFT_DBL", "vp-toggle-fullscreen"),
            ("f", "vp-toggle-fullscreen"),
            ("ESC", "vp-exit-fullscreen"),
            ("LEFT", "vp-seek -5"),
            ("RIGHT", "vp-seek 5"),
            ("Ctrl+o", "vp-open-file"),
            ("MOUSE_MOVE", "vp-show-controls"),
        ] {
            let binding = format!("script-message {message}");
            engine.sync_command(&["keybind", key, &binding])?;
        }
        Ok(engine)
    }

    fn option(&self, name: &str, value: &str) -> Result<()> {
        let disabled_script = matches!(name, "ytdl" | "osc") && value == "no";
        let name = CString::new(name)?;
        let value = CString::new(value)?;
        unsafe {
            let result = (self.api.set_option)(self.handle, name.as_ptr(), value.as_ptr());
            // These built-in scripts are absent in runtimes compiled without Lua.
            if disabled_script && result == -5 {
                return Ok(());
            }
            self.api
                .check(result, &format!("Configure {}", name.to_string_lossy()))
        }
    }

    fn sync_command(&self, args: &[&str]) -> Result<()> {
        let strings = args
            .iter()
            .map(|s| CString::new(*s))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let pointers = command_pointers(&strings);
        unsafe {
            self.api.check(
                (self.api.command)(self.handle, pointers.as_ptr()),
                "Configure input",
            )
        }
    }

    fn runtime_configuration(&self) -> String {
        unsafe {
            let configuration =
                (self.api.get_property_string)(self.handle, c"mpv-configuration".as_ptr());
            let value = c_string(configuration);
            if !configuration.is_null() {
                (self.api.free)(configuration.cast());
            }
            value
        }
    }

    fn run(
        self,
        requests: mpsc::Receiver<Request>,
        state: Arc<Mutex<PlaybackSnapshot>>,
        interactions: mpsc::SyncSender<HostEvent>,
    ) {
        let mut snapshot = PlaybackSnapshot::default();
        let mut next_request = 1u64;
        let mut pending = VecDeque::new();
        let mut in_flight = None;
        loop {
            for request in requests.try_iter().take(128) {
                if matches!(request, Request::Shutdown) {
                    return;
                }
                pending.push_back(request);
            }
            // libmpv explicitly permits reordering async calls. Wait for the
            // previous request's reply before submitting another operation.
            if in_flight.is_none()
                && let Some(request) = pending.pop_front()
            {
                let result = match request {
                    Request::Shutdown => return,
                    Request::Command(args) => {
                        let pointers = command_pointers(&args);
                        unsafe {
                            (self.api.command_async)(self.handle, next_request, pointers.as_ptr())
                        }
                    }
                    Request::Property(name, value) => {
                        let mut pointer = value.as_ptr();
                        unsafe {
                            (self.api.set_property_async)(
                                self.handle,
                                next_request,
                                name.as_ptr(),
                                STRING,
                                (&mut pointer as *mut *const c_char).cast(),
                            )
                        }
                    }
                };
                if result >= 0 {
                    in_flight = Some(next_request);
                }
                next_request = next_request.wrapping_add(1).max(1);
                if let Err(error) = self.api.check(result, "Playback request") {
                    snapshot.error = Some(error.to_string());
                }
            }
            // At most 20 ms command latency, with no synchronous file IO on the GUI thread.
            let event = unsafe { &*(self.api.wait_event)(self.handle, 0.02) };
            match event.id {
                1 => {
                    snapshot.idle = true;
                    *state.lock().unwrap_or_else(|e| e.into_inner()) = snapshot;
                    return;
                }
                2 if !event.data.is_null() => {
                    let log = unsafe { &*event.data.cast::<LogMessage>() };
                    let prefix = unsafe { c_string(log.prefix) };
                    if matches!(
                        prefix.as_str(),
                        "cplayer" | "vo/gpu-next" | "ffmpeg/demuxer"
                    ) {
                        snapshot.error = Some(unsafe { c_string(log.text) }.trim().to_owned());
                    }
                }
                4 | 5 => {
                    if in_flight == Some(event.userdata) {
                        in_flight = None;
                    }
                    if event.error < 0 {
                        snapshot.error =
                            Some(unsafe { c_string((self.api.error_string)(event.error)) });
                    }
                }
                6 => {
                    snapshot.error = None;
                    snapshot.position = 0.0;
                    snapshot.duration = 0.0;
                    snapshot.eof = false;
                    snapshot.tracks.clear();
                    snapshot.hwdec.clear();
                    snapshot.video_codec.clear();
                    snapshot.output_colorspace.clear();
                }
                7 if !event.data.is_null() => {
                    let end = unsafe { &*event.data.cast::<EndFile>() };
                    if end.reason == 4 {
                        snapshot.error =
                            Some(unsafe { c_string((self.api.error_string)(end.error)) });
                    }
                }
                16 if !event.data.is_null() => {
                    if let Some(event) =
                        unsafe { interaction(&*event.data.cast::<ClientMessage>()) }
                    {
                        let _ = interactions.try_send(event);
                    }
                }
                22 if !event.data.is_null() => unsafe {
                    apply_property(
                        &mut snapshot,
                        event.userdata,
                        &*event.data.cast::<EventProperty>(),
                    );
                },
                _ => {}
            }
            *state.lock().unwrap_or_else(|e| e.into_inner()) = snapshot.clone();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { (self.api.destroy)(self.handle) };
    }
}

fn command_pointers(args: &[CString]) -> Vec<*const c_char> {
    args.iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(ptr::null()))
        .collect()
}

unsafe fn c_string(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer).to_string_lossy().into_owned() }
}

unsafe fn interaction(message: &ClientMessage) -> Option<HostEvent> {
    if message.count <= 0 || message.args.is_null() {
        return None;
    }
    let name = unsafe { c_string(*message.args) };
    match name.as_str() {
        "vp-toggle-pause" => Some(HostEvent::TogglePause),
        "vp-toggle-fullscreen" => Some(HostEvent::ToggleFullscreen),
        "vp-exit-fullscreen" => Some(HostEvent::ExitFullscreen),
        "vp-show-controls" => Some(HostEvent::ShowControls),
        "vp-open-file" => Some(HostEvent::OpenFile),
        "vp-seek" if message.count > 1 => unsafe { c_string(*message.args.add(1)) }
            .parse()
            .ok()
            .map(HostEvent::Seek),
        _ => None,
    }
}

unsafe fn apply_property(snapshot: &mut PlaybackSnapshot, id: u64, property: &EventProperty) {
    if property.format == 0 || property.data.is_null() {
        match id {
            8 => snapshot.tracks.clear(),
            9 => snapshot.path = None,
            12 => snapshot.hwdec.clear(),
            13 => snapshot.video_codec.clear(),
            14 => snapshot.output_colorspace.clear(),
            _ => {}
        }
        return;
    }
    match (id, property.format) {
        (1, FLAG) => snapshot.paused = unsafe { *property.data.cast::<c_int>() != 0 },
        (2, DOUBLE) => snapshot.position = unsafe { *property.data.cast::<f64>() },
        (3, DOUBLE) => snapshot.duration = unsafe { *property.data.cast::<f64>() },
        (4, DOUBLE) => snapshot.volume = unsafe { *property.data.cast::<f64>() },
        (5, FLAG) => snapshot.mute = unsafe { *property.data.cast::<c_int>() != 0 },
        (6, DOUBLE) => snapshot.speed = unsafe { *property.data.cast::<f64>() },
        (7, DOUBLE) => snapshot.subtitle_delay = unsafe { *property.data.cast::<f64>() },
        (8, NODE) => snapshot.tracks = unsafe { parse_tracks(&*property.data.cast::<Node>()) },
        (9, STRING) => {
            snapshot.path = Some(unsafe { c_string(*property.data.cast::<*const c_char>()) })
        }
        (10, FLAG) => snapshot.eof = unsafe { *property.data.cast::<c_int>() != 0 },
        (11, FLAG) => snapshot.idle = unsafe { *property.data.cast::<c_int>() != 0 },
        (12, STRING) => {
            snapshot.hwdec = unsafe { c_string(*property.data.cast::<*const c_char>()) }
        }
        (13, STRING) => {
            snapshot.video_codec = unsafe { c_string(*property.data.cast::<*const c_char>()) }
        }
        (14, NODE) => {
            let node = unsafe { &*property.data.cast::<Node>() };
            snapshot.output_colorspace = ["primaries", "gamma", "pixelformat"]
                .iter()
                .filter_map(|key| unsafe { map_get(node, key).and_then(|node| node_string(node)) })
                .collect::<Vec<_>>()
                .join(" / ");
        }
        _ => {}
    }
}

unsafe fn node_string(node: &Node) -> Option<String> {
    (node.format == STRING).then(|| unsafe { c_string(node.value.string) })
}

unsafe fn map_get<'a>(node: &'a Node, key: &str) -> Option<&'a Node> {
    if node.format != NODE_MAP {
        return None;
    }
    let list = unsafe { node.value.list.as_ref()? };
    if list.count <= 0 || list.count > 16_384 || list.keys.is_null() || list.values.is_null() {
        return None;
    }
    for index in 0..list.count as usize {
        if unsafe { c_string(*list.keys.add(index)) } == key {
            return Some(unsafe { &*list.values.add(index) });
        }
    }
    None
}

unsafe fn parse_tracks(node: &Node) -> Vec<Track> {
    if node.format != NODE_ARRAY {
        return Vec::new();
    }
    let Some(list) = (unsafe { node.value.list.as_ref() }) else {
        return Vec::new();
    };
    if list.count <= 0 || list.count > 16_384 || list.values.is_null() {
        return Vec::new();
    }
    (0..list.count as usize)
        .filter_map(|index| unsafe {
            let map = &*list.values.add(index);
            let id = map_get(map, "id")?;
            if id.format != INT64 {
                return None;
            }
            Some(Track {
                id: id.value.int64,
                kind: map_get(map, "type")
                    .and_then(|node| node_string(node))
                    .unwrap_or_default(),
                title: map_get(map, "title")
                    .and_then(|node| node_string(node))
                    .unwrap_or_default(),
                lang: map_get(map, "lang")
                    .and_then(|node| node_string(node))
                    .unwrap_or_default(),
                selected: map_get(map, "selected")
                    .is_some_and(|n| n.format == FLAG && n.value.flag != 0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_track_map_with_unicode_and_no_title() {
        let keys = [
            c"id".as_ptr(),
            c"type".as_ptr(),
            c"lang".as_ptr(),
            c"selected".as_ptr(),
        ];
        let language = CString::new("Türkçe").unwrap();
        let values = [
            Node {
                value: NodeValue { int64: 3 },
                format: INT64,
            },
            Node {
                value: NodeValue {
                    string: c"sub".as_ptr(),
                },
                format: STRING,
            },
            Node {
                value: NodeValue {
                    string: language.as_ptr(),
                },
                format: STRING,
            },
            Node {
                value: NodeValue { flag: 1 },
                format: FLAG,
            },
        ];
        let map_list = NodeList {
            count: 4,
            values: values.as_ptr(),
            keys: keys.as_ptr(),
        };
        let map = Node {
            value: NodeValue { list: &map_list },
            format: NODE_MAP,
        };
        let array_list = NodeList {
            count: 1,
            values: &map,
            keys: ptr::null(),
        };
        let array = Node {
            value: NodeValue { list: &array_list },
            format: NODE_ARRAY,
        };
        let tracks = unsafe { parse_tracks(&array) };
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, 3);
        assert_eq!(tracks[0].lang, "Türkçe");
        assert!(tracks[0].selected);
        assert_eq!(tracks[0].title, "");
    }

    #[test]
    fn unavailable_properties_clear_previous_file_metadata() {
        let mut snapshot = PlaybackSnapshot {
            path: Some("old.mkv".into()),
            hwdec: "d3d11va".into(),
            ..Default::default()
        };
        let missing = EventProperty {
            name: ptr::null(),
            format: 0,
            data: ptr::null_mut(),
        };
        unsafe {
            apply_property(&mut snapshot, 9, &missing);
            apply_property(&mut snapshot, 12, &missing);
        }
        assert!(snapshot.path.is_none());
        assert!(snapshot.hwdec.is_empty());
    }

    #[test]
    fn command_arguments_are_independent_null_terminated_values() {
        let args = [
            CString::new("loadfile").unwrap(),
            CString::new("C:\\Türkçe\\a; b.mkv").unwrap(),
        ];
        let pointers = command_pointers(&args);
        assert_eq!(pointers.len(), 3);
        assert!(pointers[2].is_null());
        assert_eq!(unsafe { c_string(pointers[1]) }, "C:\\Türkçe\\a; b.mkv");
    }
}
