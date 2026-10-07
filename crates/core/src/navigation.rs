use crate::{MediaFile, MediaKind};
use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaNeighbors {
    pub previous: Option<PathBuf>,
    pub next: Option<PathBuf>,
}

impl MediaNeighbors {
    pub fn scan(current: &MediaFile) -> Result<Self> {
        if current.kind == MediaKind::Subtitle {
            return Ok(Self::default());
        }
        let Some(parent) = current.path.parent() else {
            return Ok(Self::default());
        };
        let entries = std::fs::read_dir(parent).context("Cannot read the media folder")?;
        let mut files = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let Ok(media) = MediaFile::open(entry.path()) else {
                continue;
            };
            if media.kind == current.kind && media.path.parent() == Some(parent) {
                let name = media.display_name();
                files.push((name.to_lowercase(), name, media.path));
            }
        }
        files.sort_unstable_by(|left, right| {
            natural_compare(&left.0, &right.0)
                .then_with(|| left.0.cmp(&right.0))
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        let Some(index) = files.iter().position(|file| file.2 == current.path) else {
            return Ok(Self::default());
        };
        Ok(Self {
            previous: index.checked_sub(1).map(|index| files[index].2.clone()),
            next: files.get(index + 1).map(|file| file.2.clone()),
        })
    }
}

fn natural_compare(left: &str, right: &str) -> Ordering {
    let mut left = left.as_bytes();
    let mut right = right.as_bytes();
    while let (Some(&left_byte), Some(&right_byte)) = (left.first(), right.first()) {
        if left_byte.is_ascii_digit() && right_byte.is_ascii_digit() {
            let left_len = left.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let right_len = right
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            let left_digits = &left[..left_len];
            let right_digits = &right[..right_len];
            let left_significant = &left_digits[left_digits
                .iter()
                .position(|byte| *byte != b'0')
                .unwrap_or(left_len)..];
            let right_significant = &right_digits[right_digits
                .iter()
                .position(|byte| *byte != b'0')
                .unwrap_or(right_len)..];
            let ordering = left_significant
                .len()
                .cmp(&right_significant.len())
                .then_with(|| left_significant.cmp(right_significant));
            if ordering != Ordering::Equal {
                return ordering;
            }
            left = &left[left_len..];
            right = &right[right_len..];
        } else {
            let ordering = left_byte.cmp(&right_byte);
            if ordering != Ordering::Equal {
                return ordering;
            }
            left = &left[1..];
            right = &right[1..];
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn media(dir: &Path, name: &str) -> MediaFile {
        let path = dir.join(name);
        std::fs::write(&path, b"test media").unwrap();
        MediaFile::open(path).unwrap()
    }

    #[test]
    fn unicode_filenames_and_numbers_have_natural_order() {
        let dir = tempfile::tempdir().unwrap();
        let previous = media(dir.path(), "İstanbul 2.MKV");
        let current = media(dir.path(), "İstanbul 10.mp4");
        let next = media(dir.path(), "İstanbul 11.WEBM");
        media(dir.path(), "İstanbul 1.mov");
        media(dir.path(), "İstanbul 100.avi");

        assert_eq!(
            MediaNeighbors::scan(&current).unwrap(),
            MediaNeighbors {
                previous: Some(previous.path),
                next: Some(next.path),
            }
        );
    }

    #[test]
    fn excludes_other_media_kinds_empty_files_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        let current = media(dir.path(), "video1.mp4");
        let next = media(dir.path(), "video9.MOV");
        media(dir.path(), "video2.mp3");
        media(dir.path(), "video3.SRT");
        std::fs::write(dir.path().join("video4.mp4"), []).unwrap();
        std::fs::create_dir(dir.path().join("video5.mkv")).unwrap();
        std::fs::write(dir.path().join("video6.exe"), b"unrelated").unwrap();
        media(&dir.path().join("video5.mkv"), "video7.mp4");

        assert_eq!(
            MediaNeighbors::scan(&current).unwrap(),
            MediaNeighbors {
                previous: None,
                next: Some(next.path),
            }
        );
    }

    #[test]
    fn does_not_wrap_at_folder_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let first = media(dir.path(), "01.mp4");
        let middle = media(dir.path(), "02.mp4");
        let last = media(dir.path(), "03.mp4");

        assert_eq!(
            MediaNeighbors::scan(&first).unwrap(),
            MediaNeighbors {
                previous: None,
                next: Some(middle.path.clone()),
            }
        );
        assert_eq!(
            MediaNeighbors::scan(&last).unwrap(),
            MediaNeighbors {
                previous: Some(middle.path),
                next: None,
            }
        );
    }

    #[test]
    fn audio_navigation_never_selects_video_or_subtitle() {
        let dir = tempfile::tempdir().unwrap();
        let previous = media(dir.path(), "track1.FLAC");
        let current = media(dir.path(), "track2.mp3");
        let next = media(dir.path(), "track3.m4a");
        media(dir.path(), "track2a.mp4");
        media(dir.path(), "track2b.ass");

        assert_eq!(
            MediaNeighbors::scan(&current).unwrap(),
            MediaNeighbors {
                previous: Some(previous.path),
                next: Some(next.path),
            }
        );
    }

    #[test]
    fn unavailable_current_file_has_no_navigation() {
        let dir = tempfile::tempdir().unwrap();
        let current = media(dir.path(), "video2.mp4");
        media(dir.path(), "video1.mp4");
        media(dir.path(), "video3.mp4");
        std::fs::remove_file(&current.path).unwrap();

        assert_eq!(
            MediaNeighbors::scan(&current).unwrap(),
            MediaNeighbors::default()
        );
    }

    #[test]
    fn equal_numbers_have_stable_filename_ties() {
        let dir = tempfile::tempdir().unwrap();
        let previous = media(dir.path(), "clip02.mp4");
        let current = media(dir.path(), "clip2.mp4");
        let next = media(dir.path(), "clip3.mp4");

        assert_eq!(
            MediaNeighbors::scan(&current).unwrap(),
            MediaNeighbors {
                previous: Some(previous.path),
                next: Some(next.path),
            }
        );
    }

    #[test]
    fn long_filename_numbers_do_not_overflow() {
        let dir = tempfile::tempdir().unwrap();
        let previous = media(dir.path(), "clip100000000000000000000000000002.mp4");
        let current = media(dir.path(), "clip100000000000000000000000000010.mp4");
        let next = media(dir.path(), "clip100000000000000000000000000011.mp4");

        assert_eq!(
            MediaNeighbors::scan(&current).unwrap(),
            MediaNeighbors {
                previous: Some(previous.path),
                next: Some(next.path),
            }
        );
    }
}
