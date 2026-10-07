use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mkv", "mov", "webm", "avi", "wmv"];
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "aac", "m4a", "flac", "wav"];
pub const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "vtt"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Audio,
    Subtitle,
}

#[derive(Clone, Debug)]
pub struct MediaFile {
    pub path: PathBuf,
    pub kind: MediaKind,
    pub size: u64,
    pub modified_ns: u128,
}

impl MediaFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let kind = if VIDEO_EXTENSIONS.contains(&extension.as_str()) {
            MediaKind::Video
        } else if AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            MediaKind::Audio
        } else if SUBTITLE_EXTENSIONS.contains(&extension.as_str()) {
            MediaKind::Subtitle
        } else {
            bail!("Unsupported file extension: {extension}");
        };
        let metadata = std::fs::metadata(path).context("Cannot read the selected file")?;
        if !metadata.is_file() || metadata.len() == 0 {
            bail!("The selected file is empty or is not a regular file");
        }
        let path = path
            .canonicalize()
            .context("Cannot resolve the selected file")?;
        if path.to_str().is_none() {
            bail!("This file name cannot be represented as Unicode");
        }
        let modified_ns = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
        Ok(Self {
            path,
            kind,
            size: metadata.len(),
            modified_ns,
        })
    }

    pub fn key(&self) -> String {
        self.path.to_string_lossy().to_lowercase()
    }

    pub fn display_name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_special_characters_are_paths_not_commands() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("İstanbul 'deneme' $video.MKV");
        std::fs::write(&path, b"media bytes").unwrap();
        let file = MediaFile::open(&path).unwrap();
        assert_eq!(file.kind, MediaKind::Video);
        assert_eq!(file.display_name(), "İstanbul 'deneme' $video.MKV");
        assert_eq!(file.size, 11);
    }

    #[test]
    fn rejects_missing_empty_and_unsupported_files() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.mp4");
        std::fs::write(&empty, []).unwrap();
        assert!(MediaFile::open(&empty).is_err());
        assert!(MediaFile::open(dir.path().join("missing.mp4")).is_err());
        let executable = dir.path().join("malware.exe");
        std::fs::write(&executable, b"something").unwrap();
        assert!(MediaFile::open(executable).is_err());
    }
}
