//! Limited public FFmpeg 8 ABI prefixes, copied as type declarations from its LGPL headers.
//! No struct is allocated here: libav* allocators own every native object. Major versions are checked before field access.

use crate::{
    Image, MAX_DIMENSION,
    broker::{READ_BUDGET, READ_CHUNK, WORKER_TIMEOUT},
};
use anyhow::{Context, Result, bail, ensure};
use libloading::os::windows::Library;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    ptr,
    time::Instant,
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rational {
    num: i32,
    den: i32,
}
#[repr(C)]
struct Interrupt {
    callback: Option<unsafe extern "C" fn(*mut c_void) -> i32>,
    opaque: *mut c_void,
}
#[repr(C)]
struct Format {
    av_class: *const c_void,
    iformat: *const c_void,
    oformat: *const c_void,
    priv_data: *mut c_void,
    pb: *mut Avio,
    ctx_flags: i32,
    nb_streams: u32,
    streams: *mut *mut Stream,
    nb_stream_groups: u32,
    stream_groups: *mut c_void,
    nb_chapters: u32,
    chapters: *mut c_void,
    url: *mut c_char,
    start_time: i64,
    duration: i64,
    bit_rate: i64,
    packet_size: u32,
    max_delay: i32,
    flags: i32,
    probesize: i64,
    max_analyze_duration: i64,
    key: *const u8,
    keylen: i32,
    nb_programs: u32,
    programs: *mut c_void,
    video_codec_id: i32,
    audio_codec_id: i32,
    subtitle_codec_id: i32,
    data_codec_id: i32,
    metadata: *mut c_void,
    start_time_realtime: i64,
    fps_probe_size: i32,
    error_recognition: i32,
    interrupt_callback: Interrupt,
}
#[repr(C)]
struct Stream {
    av_class: *const c_void,
    index: i32,
    id: i32,
    codecpar: *const CodecParameters,
    priv_data: *mut c_void,
    time_base: Rational,
    start_time: i64,
    duration: i64,
    nb_frames: i64,
    disposition: i32,
    discard: i32,
    sample_aspect_ratio: Rational,
}
#[repr(C)]
struct CodecParameters {
    codec_type: i32,
    codec_id: i32,
    codec_tag: u32,
    extradata: *mut u8,
    extradata_size: i32,
    coded_side_data: *const PacketSideData,
    nb_coded_side_data: i32,
}
#[repr(C)]
struct PacketSideData {
    data: *mut u8,
    size: usize,
    kind: i32,
}
#[repr(C)]
struct Avio {
    av_class: *const c_void,
    buffer: *mut u8,
}
#[repr(C)]
struct Packet {
    buf: *mut c_void,
    pts: i64,
    dts: i64,
    data: *mut u8,
    size: i32,
    stream_index: i32,
}
#[repr(C)]
struct Frame {
    data: [*mut u8; 8],
    linesize: [i32; 8],
    extended_data: *mut *mut u8,
    width: i32,
    height: i32,
    nb_samples: i32,
    format: i32,
    pict_type: i32,
    sample_aspect_ratio: Rational,
    pts: i64,
    pkt_dts: i64,
    time_base: Rational,
    quality: i32,
    opaque: *mut c_void,
    repeat_pict: i32,
    sample_rate: i32,
    buf: [*mut c_void; 8],
    extended_buf: *mut c_void,
    nb_extended_buf: i32,
    side_data: *mut c_void,
    nb_side_data: i32,
    flags: i32,
    color_range: i32,
    color_primaries: i32,
    color_trc: i32,
    colorspace: i32,
    chroma_location: i32,
    best_effort_timestamp: i64,
}
#[repr(C)]
struct FrameSideData {
    kind: i32,
    data: *mut u8,
    size: usize,
}
#[repr(C)]
struct FilterInOut {
    name: *mut c_char,
    filter_ctx: *mut c_void,
    pad_idx: i32,
    next: *mut FilterInOut,
}

type ReadCallback = Option<unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int>;
type WriteCallback = Option<unsafe extern "C" fn(*mut c_void, *const u8, c_int) -> c_int>;
type SeekCallback = Option<unsafe extern "C" fn(*mut c_void, i64, c_int) -> i64>;

struct Api {
    _libraries: Vec<Library>,
    malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
    strdup: unsafe extern "C" fn(*const c_char) -> *mut c_char,
    opt_int: unsafe extern "C" fn(*mut c_void, *const c_char, i64, i32) -> i32,
    opt_str: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, i32) -> i32,
    dict_set: unsafe extern "C" fn(*mut *mut c_void, *const c_char, *const c_char, i32) -> i32,
    dict_free: unsafe extern "C" fn(*mut *mut c_void),
    strerror: unsafe extern "C" fn(i32, *mut c_char, usize) -> i32,
    frame_alloc: unsafe extern "C" fn() -> *mut Frame,
    frame_free: unsafe extern "C" fn(*mut *mut Frame),
    frame_unref: unsafe extern "C" fn(*mut Frame),
    frame_side: unsafe extern "C" fn(*const Frame, i32) -> *mut FrameSideData,
    rotation: unsafe extern "C" fn(*const i32) -> f64,
    alloc_format: unsafe extern "C" fn() -> *mut Format,
    alloc_io: unsafe extern "C" fn(
        *mut u8,
        i32,
        i32,
        *mut c_void,
        ReadCallback,
        WriteCallback,
        SeekCallback,
    ) -> *mut Avio,
    free_io: unsafe extern "C" fn(*mut *mut Avio),
    open_input: unsafe extern "C" fn(
        *mut *mut Format,
        *const c_char,
        *const c_void,
        *mut *mut c_void,
    ) -> i32,
    stream_info: unsafe extern "C" fn(*mut Format, *mut *mut c_void) -> i32,
    best_stream: unsafe extern "C" fn(*mut Format, i32, i32, i32, *mut *const c_void, i32) -> i32,
    seek_frame: unsafe extern "C" fn(*mut Format, i32, i64, i32) -> i32,
    read_frame: unsafe extern "C" fn(*mut Format, *mut Packet) -> i32,
    close_input: unsafe extern "C" fn(*mut *mut Format),
    codec_alloc: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    codec_parameters: unsafe extern "C" fn(*mut c_void, *const c_void) -> i32,
    codec_open: unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    codec_send: unsafe extern "C" fn(*mut c_void, *const Packet) -> i32,
    codec_receive: unsafe extern "C" fn(*mut c_void, *mut Frame) -> i32,
    codec_free: unsafe extern "C" fn(*mut *mut c_void),
    codec_flush: unsafe extern "C" fn(*mut c_void),
    packet_side: unsafe extern "C" fn(*const PacketSideData, i32, i32) -> *const PacketSideData,
    packet_alloc: unsafe extern "C" fn() -> *mut Packet,
    packet_free: unsafe extern "C" fn(*mut *mut Packet),
    packet_unref: unsafe extern "C" fn(*mut Packet),
    filter_get: unsafe extern "C" fn(*const c_char) -> *const c_void,
    graph_alloc: unsafe extern "C" fn() -> *mut c_void,
    graph_free: unsafe extern "C" fn(*mut *mut c_void),
    filter_create: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_void,
        *const c_char,
        *const c_char,
        *mut c_void,
        *mut c_void,
    ) -> i32,
    graph_parse: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *mut *mut FilterInOut,
        *mut *mut FilterInOut,
        *mut c_void,
    ) -> i32,
    graph_config: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32,
    inout_alloc: unsafe extern "C" fn() -> *mut FilterInOut,
    inout_free: unsafe extern "C" fn(*mut *mut FilterInOut),
    filter_send: unsafe extern "C" fn(*mut c_void, *mut Frame, i32) -> i32,
    filter_receive: unsafe extern "C" fn(*mut c_void, *mut Frame) -> i32,
}

impl Api {
    fn load(directory: &Path) -> Result<Self> {
        // Never search PATH or the current directory for native dependencies.
        let flags = 0x00000100 | 0x00000800; // LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | SYSTEM32
        let names = [
            "avutil-60.dll",
            "avcodec-62.dll",
            "avformat-62.dll",
            "avfilter-11.dll",
        ];
        let mut libraries = Vec::new();
        for name in names {
            let path = directory
                .join(name)
                .canonicalize()
                .with_context(|| format!("Missing LGPL runtime {name}"))?;
            libraries.push(unsafe { Library::load_with_flags(&path, flags)? });
        }
        for (index, symbol, major) in [
            (0, c"avutil_version", 60),
            (1, c"avcodec_version", 62),
            (2, c"avformat_version", 62),
            (3, c"avfilter_version", 11),
        ] {
            let version = unsafe {
                *libraries[index]
                    .get::<unsafe extern "C" fn() -> u32>(symbol.to_bytes_with_nul())?
            };
            ensure!(
                unsafe { version() } >> 16 == major,
                "Unsupported FFmpeg ABI; use the shipped FFmpeg 8 runtime"
            );
        }
        for (index, symbol) in [
            (0, c"avutil_configuration"),
            (1, c"avcodec_configuration"),
            (2, c"avformat_configuration"),
            (3, c"avfilter_configuration"),
        ] {
            let configuration = unsafe {
                *libraries[index]
                    .get::<unsafe extern "C" fn() -> *const c_char>(symbol.to_bytes_with_nul())?
            };
            let configuration = unsafe { CStr::from_ptr(configuration()) }.to_string_lossy();
            ensure!(
                !configuration.contains("--enable-gpl")
                    && !configuration.contains("--enable-nonfree"),
                "FFmpeg runtime must use the LGPL build"
            );
        }
        macro_rules! symbol {
            ($library:expr, $name:literal) => {
                unsafe { *libraries[$library].get(concat!($name, "\0").as_bytes())? }
            };
        }
        Ok(Self {
            malloc: symbol!(0, "av_malloc"),
            free: symbol!(0, "av_free"),
            strdup: symbol!(0, "av_strdup"),
            opt_int: symbol!(0, "av_opt_set_int"),
            opt_str: symbol!(0, "av_opt_set"),
            dict_set: symbol!(0, "av_dict_set"),
            dict_free: symbol!(0, "av_dict_free"),
            strerror: symbol!(0, "av_strerror"),
            frame_alloc: symbol!(0, "av_frame_alloc"),
            frame_free: symbol!(0, "av_frame_free"),
            frame_unref: symbol!(0, "av_frame_unref"),
            frame_side: symbol!(0, "av_frame_get_side_data"),
            rotation: symbol!(0, "av_display_rotation_get"),
            alloc_format: symbol!(2, "avformat_alloc_context"),
            alloc_io: symbol!(2, "avio_alloc_context"),
            free_io: symbol!(2, "avio_context_free"),
            open_input: symbol!(2, "avformat_open_input"),
            stream_info: symbol!(2, "avformat_find_stream_info"),
            best_stream: symbol!(2, "av_find_best_stream"),
            seek_frame: symbol!(2, "av_seek_frame"),
            read_frame: symbol!(2, "av_read_frame"),
            close_input: symbol!(2, "avformat_close_input"),
            codec_alloc: symbol!(1, "avcodec_alloc_context3"),
            codec_parameters: symbol!(1, "avcodec_parameters_to_context"),
            codec_open: symbol!(1, "avcodec_open2"),
            codec_send: symbol!(1, "avcodec_send_packet"),
            codec_receive: symbol!(1, "avcodec_receive_frame"),
            codec_free: symbol!(1, "avcodec_free_context"),
            codec_flush: symbol!(1, "avcodec_flush_buffers"),
            packet_side: symbol!(1, "av_packet_side_data_get"),
            packet_alloc: symbol!(1, "av_packet_alloc"),
            packet_free: symbol!(1, "av_packet_free"),
            packet_unref: symbol!(1, "av_packet_unref"),
            filter_get: symbol!(3, "avfilter_get_by_name"),
            graph_alloc: symbol!(3, "avfilter_graph_alloc"),
            graph_free: symbol!(3, "avfilter_graph_free"),
            filter_create: symbol!(3, "avfilter_graph_create_filter"),
            graph_parse: symbol!(3, "avfilter_graph_parse_ptr"),
            graph_config: symbol!(3, "avfilter_graph_config"),
            inout_alloc: symbol!(3, "avfilter_inout_alloc"),
            inout_free: symbol!(3, "avfilter_inout_free"),
            filter_send: symbol!(3, "av_buffersrc_add_frame_flags"),
            filter_receive: symbol!(3, "av_buffersink_get_frame"),
            _libraries: libraries,
        })
    }
    fn check(&self, result: i32, action: &str) -> Result<i32> {
        if result < 0 {
            let mut error = [0i8; 256];
            unsafe {
                (self.strerror)(result, error.as_mut_ptr(), error.len());
            }
            bail!(
                "{action}: {} ({result})",
                unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy()
            );
        }
        Ok(result)
    }
}

pub trait MediaStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize>;
    fn seek(&mut self, offset: i64, whence: i32) -> io::Result<i64>;
}
struct IoBridge<'a> {
    stream: &'a mut dyn MediaStream,
    deadline: Instant,
    bytes_read: u64,
}
unsafe extern "C" fn read_stream(opaque: *mut c_void, buffer: *mut u8, size: i32) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        let io = unsafe { &mut *opaque.cast::<IoBridge<'_>>() };
        if size <= 0 || buffer.is_null() || Instant::now() >= io.deadline {
            return -5;
        }
        let count = (size as usize).min(READ_CHUNK);
        if io.bytes_read + count as u64 > READ_BUDGET {
            return -5;
        }
        let bytes = unsafe { std::slice::from_raw_parts_mut(buffer, count) };
        match io.stream.read(bytes) {
            Ok(0) => -541478725, // AVERROR_EOF: zero must never be returned to AVIO.
            Ok(actual) if actual <= count => {
                io.bytes_read += actual as u64;
                actual as i32
            }
            _ => -5,
        }
    }))
    .unwrap_or(-5)
}
unsafe extern "C" fn seek_stream(opaque: *mut c_void, offset: i64, whence: i32) -> i64 {
    catch_unwind(AssertUnwindSafe(|| {
        let io = unsafe { &mut *opaque.cast::<IoBridge<'_>>() };
        if Instant::now() >= io.deadline {
            return -5;
        }
        io.stream.seek(offset, whence).unwrap_or(-5)
    }))
    .unwrap_or(-5)
}
unsafe extern "C" fn interrupted(opaque: *mut c_void) -> i32 {
    let io = unsafe { &*opaque.cast::<IoBridge<'_>>() };
    i32::from(Instant::now() >= io.deadline)
}
struct Decoder<'a> {
    api: &'a Api,
    format: *mut Format,
    io: *mut Avio,
    codec: *mut c_void,
    packet: *mut Packet,
    frame: *mut Frame,
}
impl Drop for Decoder<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.frame_free)(&mut self.frame);
            (self.api.packet_free)(&mut self.packet);
            (self.api.codec_free)(&mut self.codec);
            (self.api.close_input)(&mut self.format);
            if !self.io.is_null() {
                (self.api.free)((*self.io).buffer.cast());
                (self.api.free_io)(&mut self.io);
            }
        }
    }
}

pub fn decode(stream: &mut dyn MediaStream, dimension: u32, runtime: &Path) -> Result<Image> {
    ensure!(
        dimension > 0 && dimension <= MAX_DIMENSION,
        "Invalid thumbnail dimension"
    );
    let api = Api::load(runtime)?;
    let mut bridge = IoBridge {
        stream,
        deadline: Instant::now() + WORKER_TIMEOUT,
        bytes_read: 0,
    };
    let opaque = (&mut bridge as *mut IoBridge<'_>).cast::<c_void>();
    let mut decoder = Decoder {
        api: &api,
        format: ptr::null_mut(),
        io: ptr::null_mut(),
        codec: ptr::null_mut(),
        packet: ptr::null_mut(),
        frame: ptr::null_mut(),
    };
    unsafe {
        let buffer = (api.malloc)(READ_CHUNK).cast::<u8>();
        ensure!(!buffer.is_null(), "Allocate FFmpeg IO buffer");
        decoder.io = (api.alloc_io)(
            buffer,
            READ_CHUNK as i32,
            0,
            opaque,
            Some(read_stream),
            None,
            Some(seek_stream),
        );
        if decoder.io.is_null() {
            (api.free)(buffer.cast());
            bail!("Allocate FFmpeg custom IO");
        }
        decoder.format = (api.alloc_format)();
        ensure!(!decoder.format.is_null(), "Allocate FFmpeg format");
        (*decoder.format).pb = decoder.io;
        (*decoder.format).flags |= 0x0080 | 0x0100; // custom IO and discard corrupt packets.
        (*decoder.format).interrupt_callback = Interrupt {
            callback: Some(interrupted),
            opaque,
        };
        api.check(
            (api.opt_int)(
                decoder.format.cast(),
                c"probesize".as_ptr(),
                2 * 1024 * 1024,
                0,
            ),
            "Limit probing",
        )?;
        api.check(
            (api.opt_int)(
                decoder.format.cast(),
                c"analyzeduration".as_ptr(),
                1_000_000,
                0,
            ),
            "Limit analysis",
        )?;
        api.check(
            (api.opt_int)(decoder.format.cast(), c"max_streams".as_ptr(), 128, 0),
            "Limit streams",
        )?;
        api.check(
            (api.opt_str)(
                decoder.format.cast(),
                c"protocol_whitelist".as_ptr(),
                c"".as_ptr(),
                0,
            ),
            "Disable protocols",
        )?;
        api.check(
            (api.opt_str)(
                decoder.format.cast(),
                c"format_whitelist".as_ptr(),
                c"mov,matroska,webm,avi,asf".as_ptr(),
                0,
            ),
            "Limit media containers",
        )?;
        let mut options = ptr::null_mut();
        (api.dict_set)(&mut options, c"enable_drefs".as_ptr(), c"0".as_ptr(), 0);
        (api.dict_set)(
            &mut options,
            c"use_absolute_path".as_ptr(),
            c"0".as_ptr(),
            0,
        );
        let result = (api.open_input)(&mut decoder.format, ptr::null(), ptr::null(), &mut options);
        (api.dict_free)(&mut options);
        api.check(result, "Open media stream")?;
        api.check(
            (api.stream_info)(decoder.format, ptr::null_mut()),
            "Read stream information",
        )?;
        let mut codec = ptr::null();
        let index = api.check(
            (api.best_stream)(decoder.format, 0, -1, -1, &mut codec, 0),
            "Find video stream",
        )?;
        ensure!(
            (index as u32) < (*decoder.format).nb_streams
                && (*decoder.format).nb_streams <= 128
                && !(*decoder.format).streams.is_null(),
            "Invalid FFmpeg stream index"
        );
        let stream = *(*decoder.format).streams.add(index as usize);
        ensure!(
            !stream.is_null() && !(*stream).codecpar.is_null(),
            "Missing codec parameters"
        );
        decoder.codec = (api.codec_alloc)(codec);
        ensure!(!decoder.codec.is_null(), "Allocate decoder");
        api.check(
            (api.codec_parameters)(decoder.codec, (*stream).codecpar.cast()),
            "Configure decoder",
        )?;
        api.check(
            (api.opt_int)(decoder.codec, c"threads".as_ptr(), 2, 0),
            "Limit decoder threads",
        )?;
        api.check(
            (api.opt_int)(decoder.codec, c"max_pixels".as_ptr(), 70_000_000, 0),
            "Limit decoder image size",
        )?;
        api.check(
            (api.codec_open)(decoder.codec, codec, ptr::null_mut()),
            "Open video decoder",
        )?;
        decoder.packet = (api.packet_alloc)();
        decoder.frame = (api.frame_alloc)();
        ensure!(
            !decoder.packet.is_null() && !decoder.frame.is_null(),
            "Allocate decode buffers"
        );
        let duration = (*decoder.format).duration;
        let target_us = if duration > 2_000_000 {
            (duration / 10).clamp(1_000_000, 30_000_000)
        } else {
            0
        };
        let start_us = if (*decoder.format).start_time == i64::MIN {
            0
        } else {
            (*decoder.format).start_time
        };
        let seek_succeeded = target_us > 0
            && (api.seek_frame)(decoder.format, -1, target_us.saturating_add(start_us), 1) >= 0;
        if seek_succeeded {
            (api.codec_flush)(decoder.codec);
        }
        let time_base = (*stream).time_base;
        let mut decoded_frames = 0;
        let mut draining = false;
        for _ in 0..8192 {
            ensure!(
                Instant::now() < bridge.deadline,
                "Thumbnail decoding timed out"
            );
            let receive = (api.codec_receive)(decoder.codec, decoder.frame);
            if receive >= 0 {
                decoded_frames += 1;
                let frame = &*decoder.frame;
                ensure!(
                    frame.width > 0
                        && frame.height > 0
                        && frame.width <= 16384
                        && frame.height <= 16384
                        && i64::from(frame.width) * i64::from(frame.height) <= 70_000_000,
                    "Decoded frame dimensions exceed limit"
                );
                let timestamp = frame.best_effort_timestamp;
                let before_target = seek_succeeded
                    && timestamp != i64::MIN
                    && time_base.den > 0
                    && time_base.num > 0
                    && ((timestamp as f64 * time_base.num as f64 / time_base.den as f64)
                        * 1_000_000.0)
                        < target_us.saturating_add(start_us) as f64;
                if before_target && decoded_frames < 180 {
                    (api.frame_unref)(decoder.frame);
                    continue;
                }
                let parameters = &*(*stream).codecpar;
                let stream_display =
                    (api.packet_side)(parameters.coded_side_data, parameters.nb_coded_side_data, 5);
                let stream_rotation = if !stream_display.is_null()
                    && (*stream_display).size >= 9 * size_of::<i32>()
                    && !(*stream_display).data.is_null()
                {
                    (api.rotation)((*stream_display).data.cast::<i32>())
                } else {
                    0.0
                };
                return render_frame(
                    &api,
                    decoder.frame,
                    (*stream).sample_aspect_ratio,
                    stream_rotation,
                    dimension,
                );
            }
            if receive == -541478725 || draining {
                break;
            }
            ensure!(receive == -11, "Video decoder rejected a frame ({receive})");
            let read = (api.read_frame)(decoder.format, decoder.packet);
            if read < 0 {
                api.check(
                    (api.codec_send)(decoder.codec, ptr::null()),
                    "Drain video decoder",
                )?;
                draining = true;
                continue;
            }
            if (*decoder.packet).stream_index == index {
                let sent = (api.codec_send)(decoder.codec, decoder.packet);
                (api.packet_unref)(decoder.packet);
                api.check(sent, "Decode video packet")?;
            } else {
                (api.packet_unref)(decoder.packet);
            }
        }
    }
    bail!("No decodable video frame within the thumbnail budget")
}

struct Graph<'a> {
    api: &'a Api,
    graph: *mut c_void,
    inputs: *mut FilterInOut,
    outputs: *mut FilterInOut,
    output_frame: *mut Frame,
}
impl Drop for Graph<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.inout_free)(&mut self.inputs);
            (self.api.inout_free)(&mut self.outputs);
            (self.api.frame_free)(&mut self.output_frame);
            (self.api.graph_free)(&mut self.graph);
        }
    }
}

fn fit_dimensions(width: i32, height: i32, sar: Rational, dimension: u32) -> (u32, u32) {
    let sar = if sar.num > 0 && sar.den > 0 && sar.num <= 65535 && sar.den <= 65535 {
        sar.num as f64 / sar.den as f64
    } else {
        1.0
    };
    let width = width as f64 * sar;
    let height = height as f64;
    let scale = (dimension as f64 / width.max(height)).min(1.0);
    (
        ((width * scale).round() as u32).clamp(1, dimension),
        ((height * scale).round() as u32).clamp(1, dimension),
    )
}

fn render_frame(
    api: &Api,
    frame: *mut Frame,
    stream_sar: Rational,
    stream_rotation: f64,
    dimension: u32,
) -> Result<Image> {
    let mut graph = Graph {
        api,
        graph: unsafe { (api.graph_alloc)() },
        inputs: ptr::null_mut(),
        outputs: ptr::null_mut(),
        output_frame: ptr::null_mut(),
    };
    ensure!(!graph.graph.is_null(), "Allocate thumbnail filter graph");
    let mut source = ptr::null_mut();
    let mut sink = ptr::null_mut();
    unsafe {
        api.check(
            (api.opt_int)(graph.graph, c"threads".as_ptr(), 1, 0),
            "Limit filter threads",
        )?;
        let f = &*frame;
        let sar = if f.sample_aspect_ratio.num > 0 && f.sample_aspect_ratio.den > 0 {
            f.sample_aspect_ratio
        } else {
            stream_sar
        };
        let (width, height) = fit_dimensions(f.width, f.height, sar, dimension);
        let arguments = CString::new(format!(
            "video_size={}x{}:pix_fmt={}:time_base=1/1000000:pixel_aspect={}/{}",
            f.width,
            f.height,
            f.format,
            sar.num.max(1),
            sar.den.max(1)
        ))?;
        let buffer = (api.filter_get)(c"buffer".as_ptr());
        let buffersink = (api.filter_get)(c"buffersink".as_ptr());
        ensure!(
            !buffer.is_null() && !buffersink.is_null(),
            "Required FFmpeg thumbnail filters missing"
        );
        api.check(
            (api.filter_create)(
                &mut source,
                buffer,
                c"in".as_ptr(),
                arguments.as_ptr(),
                ptr::null_mut(),
                graph.graph,
            ),
            "Create thumbnail source",
        )?;
        api.check(
            (api.filter_create)(
                &mut sink,
                buffersink,
                c"out".as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                graph.graph,
            ),
            "Create thumbnail sink",
        )?;
        graph.inputs = (api.inout_alloc)();
        graph.outputs = (api.inout_alloc)();
        ensure!(
            !graph.inputs.is_null() && !graph.outputs.is_null(),
            "Allocate filter endpoints"
        );
        (*graph.outputs).name = (api.strdup)(c"in".as_ptr());
        (*graph.outputs).filter_ctx = source;
        (*graph.outputs).pad_idx = 0;
        (*graph.outputs).next = ptr::null_mut();
        (*graph.inputs).name = (api.strdup)(c"out".as_ptr());
        (*graph.inputs).filter_ctx = sink;
        (*graph.inputs).pad_idx = 0;
        (*graph.inputs).next = ptr::null_mut();
        ensure!(
            !(*graph.inputs).name.is_null() && !(*graph.outputs).name.is_null(),
            "Allocate filter endpoint names"
        );
        let mut filters = if matches!(f.color_trc, 16 | 18) {
            ensure!(
                !(api.filter_get)(c"zscale".as_ptr()).is_null()
                    && !(api.filter_get)(c"tonemap".as_ptr()).is_null(),
                "LGPL runtime needs HDR tone mapping filters"
            );
            format!(
                "zscale=w={width}:h={height}:t=linear:npl=100,format=gbrpf32le,tonemap=tonemap=hable:desat=0,zscale=t=bt709:p=bt709:m=bt709:r=full,format=bgra,setsar=1"
            )
        } else {
            format!("scale=w={width}:h={height}:flags=bicubic,format=bgra,setsar=1")
        };
        let display = (api.frame_side)(frame, 6);
        let rotation = if !display.is_null()
            && (*display).size >= 9 * size_of::<i32>()
            && !(*display).data.is_null()
        {
            (api.rotation)((*display).data.cast::<i32>())
        } else {
            stream_rotation
        };
        if (rotation + 90.0).abs() < 1.0 {
            filters.push_str(",transpose=clock");
        } else if (rotation - 90.0).abs() < 1.0 {
            filters.push_str(",transpose=cclock");
        } else if (rotation.abs() - 180.0).abs() < 1.0 {
            filters.push_str(",hflip,vflip");
        }
        let filters = CString::new(filters)?;
        api.check(
            (api.graph_parse)(
                graph.graph,
                filters.as_ptr(),
                &mut graph.inputs,
                &mut graph.outputs,
                ptr::null_mut(),
            ),
            "Configure thumbnail filters",
        )?;
        api.check(
            (api.graph_config)(graph.graph, ptr::null_mut()),
            "Initialize thumbnail filters",
        )?;
        api.check((api.filter_send)(source, frame, 8), "Send thumbnail frame")?;
        graph.output_frame = (api.frame_alloc)();
        ensure!(
            !graph.output_frame.is_null(),
            "Allocate thumbnail output frame"
        );
        api.check(
            (api.filter_receive)(sink, graph.output_frame),
            "Receive thumbnail frame",
        )?;
        let output = &*graph.output_frame;
        ensure!(
            output.width > 0
                && output.height > 0
                && output.width <= dimension as i32
                && output.height <= dimension as i32,
            "Invalid filtered image dimensions"
        );
        ensure!(
            !output.data[0].is_null()
                && output.linesize[0].unsigned_abs() >= output.width as u32 * 4,
            "Invalid thumbnail pixel buffer"
        );
        let row_bytes = output.width as usize * 4;
        let mut pixels = vec![0; row_bytes * output.height as usize];
        for row in 0..output.height as usize {
            ptr::copy_nonoverlapping(
                output.data[0].offset(row as isize * output.linesize[0] as isize),
                pixels.as_mut_ptr().add(row * row_bytes),
                row_bytes,
            );
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        Ok(Image {
            width: output.width as u32,
            height: output.height as u32,
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_prefix_offsets_match_the_shipped_x64_ffmpeg8_headers() {
        assert_eq!(std::mem::offset_of!(Format, pb), 32);
        assert_eq!(std::mem::offset_of!(Format, streams), 48);
        assert_eq!(std::mem::offset_of!(Format, duration), 104);
        assert_eq!(std::mem::offset_of!(Format, interrupt_callback), 216);
        assert_eq!(std::mem::offset_of!(Stream, codecpar), 16);
        assert_eq!(std::mem::offset_of!(Stream, time_base), 32);
        assert_eq!(std::mem::offset_of!(CodecParameters, coded_side_data), 32);
        assert_eq!(
            std::mem::offset_of!(CodecParameters, nb_coded_side_data),
            40
        );
        assert_eq!(std::mem::offset_of!(Frame, width), 104);
        assert_eq!(std::mem::offset_of!(Frame, color_trc), 288);
        assert_eq!(std::mem::offset_of!(Frame, best_effort_timestamp), 304);
        assert_eq!(std::mem::offset_of!(Packet, stream_index), 36);
    }
    #[test]
    fn fits_square_wide_portrait_and_non_square_pixels() {
        assert_eq!(
            fit_dimensions(1920, 1080, Rational { num: 1, den: 1 }, 256),
            (256, 144)
        );
        assert_eq!(
            fit_dimensions(1080, 1920, Rational { num: 1, den: 1 }, 256),
            (144, 256)
        );
        assert_eq!(
            fit_dimensions(720, 576, Rational { num: 16, den: 15 }, 256),
            (256, 192)
        );
    }
}
