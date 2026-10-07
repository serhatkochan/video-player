//! Windows Explorer thumbnails using a stream-only COM provider and a bounded native decoder process.

#[cfg(windows)]
mod broker;
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod ffmpeg;
#[cfg(windows)]
mod worker;

pub const MAX_DIMENSION: u32 = 1024;
pub const MAX_IMAGE_BYTES: usize = MAX_DIMENSION as usize * MAX_DIMENSION as usize * 4;

#[derive(Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[cfg(windows)]
pub use broker::get_thumbnail_with_worker;
#[cfg(windows)]
pub use com::{CLSID_THUMBNAIL_PROVIDER, DllCanUnloadNow, DllGetClassObject};
#[cfg(windows)]
pub use worker::worker_main;
