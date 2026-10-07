use crate::{
    broker::{READ_CHUNK, REQUEST_READ, REQUEST_SEEK, RESPONSE_ERROR, RESPONSE_IMAGE},
    ffmpeg::{self, MediaStream},
};
use anyhow::{Context, Result, ensure};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

struct PipeStream {
    input: io::Stdin,
    output: io::Stdout,
}
impl MediaStream for PipeStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.len() > READ_CHUNK {
            return Err(io::Error::other("Read request exceeds limit"));
        }
        self.output.write_all(&[REQUEST_READ])?;
        self.output.write_all(&(bytes.len() as u32).to_le_bytes())?;
        self.output.flush()?;
        let mut length = [0; 4];
        self.input.read_exact(&mut length)?;
        let length = i32::from_le_bytes(length);
        if length < 0 || length as usize > bytes.len() {
            return Err(io::Error::other("Invalid stream read response"));
        }
        self.input.read_exact(&mut bytes[..length as usize])?;
        Ok(length as usize)
    }
    fn seek(&mut self, offset: i64, whence: i32) -> io::Result<i64> {
        self.output.write_all(&[REQUEST_SEEK])?;
        self.output.write_all(&offset.to_le_bytes())?;
        self.output.write_all(&whence.to_le_bytes())?;
        self.output.flush()?;
        let mut position = [0; 8];
        self.input.read_exact(&mut position)?;
        let position = i64::from_le_bytes(position);
        if position < 0 {
            return Err(io::Error::other("Stream seek failed"));
        }
        Ok(position)
    }
}

fn runtime_directory() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let directory = executable.parent().context("Missing worker directory")?;
    let mut candidates = vec![directory.join("runtime"), directory.to_path_buf()];
    if executable
        .file_name()
        .is_some_and(|name| name == "thumbnail_worker.exe")
    {
        for parent in directory.ancestors().take(5) {
            candidates.push(parent.join("runtime"));
        }
    }
    candidates
        .into_iter()
        .find(|path| path.join("avformat-62.dll").is_file())
        .context("Missing shipped FFmpeg runtime beside thumbnail worker")
}

pub fn worker_main() -> Result<()> {
    ensure!(
        std::env::args().nth(1).as_deref() == Some("--worker"),
        "This executable is a private Video Player thumbnail worker"
    );
    let mut stream = PipeStream {
        input: io::stdin(),
        output: io::stdout(),
    };
    let mut startup = [0; 8];
    stream.input.read_exact(&mut startup)?;
    ensure!(&startup[..4] == b"VPT1", "Invalid worker protocol version");
    let dimension = u32::from_le_bytes(startup[4..].try_into()?);
    match runtime_directory().and_then(|runtime| ffmpeg::decode(&mut stream, dimension, &runtime)) {
        Ok(image) => {
            stream.output.write_all(&[RESPONSE_IMAGE])?;
            stream.output.write_all(&image.width.to_le_bytes())?;
            stream.output.write_all(&image.height.to_le_bytes())?;
            stream.output.write_all(&image.pixels)?;
        }
        Err(error) => {
            let mut error = error.to_string().into_bytes();
            error.truncate(4096);
            stream.output.write_all(&[RESPONSE_ERROR])?;
            stream
                .output
                .write_all(&(error.len() as u32).to_le_bytes())?;
            stream.output.write_all(&error)?;
        }
    }
    stream.output.flush()?;
    Ok(())
}
