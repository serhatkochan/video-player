use crate::{Image, MAX_DIMENSION, MAX_IMAGE_BYTES};
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::{
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        Com::{IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK, STREAM_SEEK_SET},
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
    },
};

pub const READ_CHUNK: usize = 32 * 1024;
pub const READ_BUDGET: u64 = 64 * 1024 * 1024;
pub const WORKER_TIMEOUT: Duration = Duration::from_secs(10);
pub const REQUEST_READ: u8 = 1;
pub const REQUEST_SEEK: u8 = 2;
pub const RESPONSE_IMAGE: u8 = 3;
pub const RESPONSE_ERROR: u8 = 4;

pub enum Request {
    Read(u32),
    Seek(i64, i32),
    Image(Image),
    Error(String),
}

fn read_u32(input: &mut impl Read) -> Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn read_request(input: &mut impl Read) -> Result<Request> {
    let mut opcode = [0];
    input.read_exact(&mut opcode)?;
    Ok(match opcode[0] {
        REQUEST_READ => {
            let count = read_u32(input)?;
            ensure!(
                count > 0 && count <= READ_CHUNK as u32,
                "Invalid worker read size"
            );
            Request::Read(count)
        }
        REQUEST_SEEK => {
            let mut bytes = [0; 12];
            input.read_exact(&mut bytes)?;
            Request::Seek(
                i64::from_le_bytes(bytes[..8].try_into()?),
                i32::from_le_bytes(bytes[8..].try_into()?),
            )
        }
        RESPONSE_IMAGE => {
            let width = read_u32(input)?;
            let height = read_u32(input)?;
            ensure!(
                width > 0 && width <= MAX_DIMENSION && height > 0 && height <= MAX_DIMENSION,
                "Invalid worker image dimensions"
            );
            let count = width as usize * height as usize * 4;
            ensure!(count <= MAX_IMAGE_BYTES, "Worker image exceeds limit");
            let mut pixels = vec![0; count];
            input.read_exact(&mut pixels)?;
            Request::Image(Image {
                width,
                height,
                pixels,
            })
        }
        RESPONSE_ERROR => {
            let length = read_u32(input)? as usize;
            ensure!(length <= 4096, "Worker error exceeds limit");
            let mut bytes = vec![0; length];
            input.read_exact(&mut bytes)?;
            Request::Error(String::from_utf8_lossy(&bytes).into_owned())
        }
        _ => bail!("Invalid worker protocol opcode"),
    })
}

struct Job(HANDLE);
impl Job {
    fn new(child: &Child) -> Result<Self> {
        let job =
            Self(unsafe { CreateJobObjectW(None, None) }.context("Create thumbnail worker job")?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = 2 * 1024 * 1024 * 1024;
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .context("Set thumbnail worker job limits")?;
            AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle()))
                .context("Assign thumbnail worker to bounded job")?;
        }
        Ok(job)
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct WorkerGuard {
    child: Child,
    job: Option<Job>,
    requests: Receiver<Result<Request>>,
    responses: Option<SyncSender<Vec<u8>>>,
    reader: Option<thread::JoinHandle<()>>,
    writer: Option<thread::JoinHandle<()>>,
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.responses.take();
        let (_, empty_receiver) = mpsc::sync_channel(1);
        drop(std::mem::replace(&mut self.requests, empty_receiver));
        self.job.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn worker_path(directory: &Path) -> Result<PathBuf> {
    let installed = directory.join("thumbnail-worker.exe");
    if installed.is_file() {
        return Ok(installed);
    }
    let example = directory.join("examples").join("thumbnail_worker.exe");
    if example.is_file() {
        return Ok(example);
    }
    bail!("Missing thumbnail-worker.exe beside the thumbnail provider")
}

fn spawn_pipes(child: Child) -> Result<WorkerGuard> {
    let (tx, requests) = mpsc::sync_channel(1);
    let (writer_tx, writer_rx) = mpsc::sync_channel::<Vec<u8>>(1);
    let mut guard = WorkerGuard {
        child,
        job: None,
        requests,
        responses: Some(writer_tx),
        reader: None,
        writer: None,
    };
    guard.job = Some(Job::new(&guard.child)?);
    let mut stdout = guard
        .child
        .stdout
        .take()
        .context("Missing worker output pipe")?;
    let mut stdin = guard
        .child
        .stdin
        .take()
        .context("Missing worker input pipe")?;
    guard.reader = Some(
        thread::Builder::new()
            .name("thumbnail-worker-reader".into())
            .spawn(move || {
                loop {
                    let request = read_request(&mut stdout);
                    let done = !matches!(request, Ok(Request::Read(_) | Request::Seek(_, _)));
                    if tx.send(request).is_err() || done {
                        break;
                    }
                }
            })?,
    );
    guard.writer = Some(
        thread::Builder::new()
            .name("thumbnail-worker-writer".into())
            .spawn(move || {
                while let Ok(bytes) = writer_rx.recv() {
                    if stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_err() {
                        break;
                    }
                }
            })?,
    );
    Ok(guard)
}
pub fn get_thumbnail(stream: &IStream, dimension: u32, directory: &Path) -> Result<Image> {
    let worker = worker_path(directory)?;
    get_thumbnail_with_worker(stream, dimension, &worker, WORKER_TIMEOUT)
}

pub fn get_thumbnail_with_worker(
    stream: &IStream,
    dimension: u32,
    worker: &Path,
    timeout: Duration,
) -> Result<Image> {
    ensure!(
        dimension > 0 && dimension <= MAX_DIMENSION,
        "Thumbnail dimension is out of range"
    );
    let deadline = Instant::now() + timeout;
    // Keep COM on the caller's apartment; only worker pipes cross threads.
    let stream = unsafe { stream.Clone() }.unwrap_or_else(|_| stream.clone());
    unsafe {
        stream
            .Seek(0, STREAM_SEEK_SET, None)
            .context("Rewind thumbnail stream")?;
    }
    let child = Command::new(worker)
        .arg("--worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .context("Start thumbnail worker")?;
    let mut guard = spawn_pipes(child)?;
    let mut startup = b"VPT1".to_vec();
    startup.extend(dimension.to_le_bytes());
    guard
        .responses
        .as_ref()
        .context("Worker response pipe closed")?
        .try_send(startup)?;
    let mut bytes_read = 0u64;
    let mut requests_handled = 0usize;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .context("Thumbnail decoding exceeded its timeout")?;
        let request = guard
            .requests
            .recv_timeout(remaining)
            .context("Thumbnail worker timed out or disconnected")??;
        requests_handled += 1;
        ensure!(
            requests_handled <= 16384,
            "Thumbnail worker exceeded its request budget"
        );
        match request {
            Request::Read(count) => {
                ensure!(
                    bytes_read + count as u64 <= READ_BUDGET,
                    "Thumbnail exceeded its read budget"
                );
                let mut bytes = vec![0; count as usize];
                let mut actual = 0;
                let result =
                    unsafe { stream.Read(bytes.as_mut_ptr().cast(), count, Some(&mut actual)) };
                ensure!(actual <= count, "Invalid stream read length");
                let mut response;
                if result.is_err() {
                    response = (-5i32).to_le_bytes().to_vec();
                } else {
                    bytes_read += actual as u64;
                    response = (actual as i32).to_le_bytes().to_vec();
                    response.extend_from_slice(&bytes[..actual as usize]);
                }
                guard
                    .responses
                    .as_ref()
                    .context("Worker response pipe closed")?
                    .try_send(response)?;
            }
            Request::Seek(offset, whence) => {
                let whence = whence & !0x20000;
                let position = if whence == 0x10000 {
                    let mut stat = STATSTG::default();
                    unsafe { stream.Stat(&mut stat, STATFLAG_NONAME) }
                        .ok()
                        .and_then(|_| i64::try_from(stat.cbSize).ok())
                        .unwrap_or(-5)
                } else if (0..=2).contains(&whence) {
                    let mut position = 0;
                    unsafe { stream.Seek(offset, STREAM_SEEK(whence as u32), Some(&mut position)) }
                        .ok()
                        .and_then(|_| i64::try_from(position).ok())
                        .unwrap_or(-5)
                } else {
                    -22
                };
                guard
                    .responses
                    .as_ref()
                    .context("Worker response pipe closed")?
                    .try_send(position.to_le_bytes().to_vec())?;
            }
            Request::Image(image) => {
                ensure!(
                    image.width <= dimension && image.height <= dimension,
                    "Worker exceeded the requested thumbnail size"
                );
                // Wait only inside the global deadline, including native-library destruction.
                loop {
                    if let Some(status) = guard.child.try_wait()? {
                        ensure!(status.success(), "Worker exited unsuccessfully");
                        return Ok(image);
                    }
                    if Instant::now() >= deadline {
                        return Err(anyhow!("Thumbnail worker cleanup timed out"));
                    }
                    thread::sleep(Duration::from_millis(2));
                }
            }
            Request::Error(error) => bail!("Thumbnail decoder: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_worker_allocations_before_allocating_or_reading_payload() {
        let mut bytes = vec![RESPONSE_IMAGE];
        bytes.extend((MAX_DIMENSION + 1).to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        assert!(read_request(&mut &bytes[..]).is_err());
        let mut bytes = vec![REQUEST_READ];
        bytes.extend((READ_CHUNK as u32 + 1).to_le_bytes());
        assert!(read_request(&mut &bytes[..]).is_err());
        assert!(read_request(&mut &[99][..]).is_err());
    }
}
