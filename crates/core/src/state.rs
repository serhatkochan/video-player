use crate::MediaFile;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::Path;

const MAX_SETTINGS_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESUME_ENTRIES: usize = 250;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Language {
    Turkish,
    #[default]
    English,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ResumeEntry {
    pub size: u64,
    pub modified_ns: u128,
    pub position: f64,
    pub duration: f64,
    pub saved_at: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppState {
    pub language: Language,
    pub volume: f64,
    pub speed: f64,
    pub muted: bool,
    pub resume: BTreeMap<String, ResumeEntry>,
    #[serde(skip)]
    persistence: RefCell<Persistence>,
}

#[derive(Clone, Debug)]
struct Snapshot {
    language: Language,
    volume: f64,
    speed: f64,
    muted: bool,
    resume: BTreeMap<String, ResumeEntry>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            language: Language::English,
            volume: 75.0,
            speed: 1.0,
            muted: false,
            resume: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Persistence {
    baseline: Snapshot,
    touched_resume: BTreeSet<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            language: Language::English,
            volume: 75.0,
            speed: 1.0,
            muted: false,
            resume: BTreeMap::new(),
            persistence: RefCell::default(),
        }
    }
}

impl AppState {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        if std::fs::metadata(path)
            .context("Cannot read player settings")?
            .len()
            > MAX_SETTINGS_BYTES as u64
        {
            anyhow::bail!("Settings file is too large");
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .context("Cannot read player settings")?
            .take(MAX_SETTINGS_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .context("Cannot read player settings")?;
        if bytes.len() > MAX_SETTINGS_BYTES {
            anyhow::bail!("Settings file is too large");
        }
        let mut state: Self =
            serde_json::from_slice(&bytes).context("Player settings are damaged")?;
        if !state.volume.is_finite() || !state.speed.is_finite() {
            anyhow::bail!("Invalid playback settings");
        }
        state.volume = state.volume.clamp(0.0, 100.0);
        state.speed = state.speed.clamp(0.25, 4.0);
        trim_resume(&mut state.resume);
        state.persistence.get_mut().baseline = state.snapshot();
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .context("Settings path needs a parent directory")?;
        std::fs::create_dir_all(parent)?;
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(Path::new(&lock_path))
            .context("Cannot open player settings lock")?;
        lock.lock().context("Cannot lock player settings")?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = (|| -> Result<()> {
            if !self.volume.is_finite() || !self.speed.is_finite() {
                anyhow::bail!("Invalid playback settings");
            }
            let mut merged = Self::load(path)?;
            let persistence = self.persistence.borrow();
            let baseline = &persistence.baseline;
            if self.language != baseline.language {
                merged.language = self.language;
            }
            if self.volume != baseline.volume {
                merged.volume = self.volume;
            }
            if self.speed != baseline.speed {
                merged.speed = self.speed;
            }
            if self.muted != baseline.muted {
                merged.muted = self.muted;
            }
            let keys: BTreeSet<_> = baseline
                .resume
                .keys()
                .chain(self.resume.keys())
                .chain(persistence.touched_resume.iter())
                .collect();
            for key in keys {
                if persistence.touched_resume.contains(key)
                    || self.resume.get(key) != baseline.resume.get(key)
                {
                    if let Some(entry) = self.resume.get(key) {
                        merged.resume.insert(key.clone(), entry.clone());
                    } else {
                        merged.resume.remove(key);
                    }
                }
            }
            drop(persistence);
            trim_resume(&mut merged.resume);
            let bytes = serde_json::to_vec_pretty(&merged)?;
            if bytes.len() > MAX_SETTINGS_BYTES {
                anyhow::bail!("Settings file is too large");
            }
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)?;
            let mut persistence = self.persistence.borrow_mut();
            persistence.baseline = self.snapshot();
            persistence.touched_resume.clear();
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result.context("Cannot save player settings")
    }

    pub fn resume_position(&self, file: &MediaFile) -> Option<f64> {
        let entry = self.resume.get(&file.key())?;
        if entry.size != file.size
            || entry.modified_ns != file.modified_ns
            || !entry.position.is_finite()
            || !entry.duration.is_finite()
            || entry.position < 5.0
            || entry.duration <= 0.0
            || entry.position >= entry.duration - 10.0
        {
            return None;
        }
        Some(entry.position)
    }

    pub fn record_position(&mut self, file: &MediaFile, position: f64, duration: f64) {
        if !position.is_finite() || !duration.is_finite() || duration <= 0.0 {
            return;
        }
        let key = file.key();
        if position < 5.0 || position >= duration - 10.0 {
            self.resume.remove(&key);
            self.persistence.get_mut().touched_resume.insert(key);
            return;
        }
        if self.resume.get(&key).is_some_and(|entry| {
            entry.size == file.size
                && entry.modified_ns == file.modified_ns
                && entry.position == position
                && entry.duration == duration
        }) {
            return;
        }
        self.persistence
            .get_mut()
            .touched_resume
            .insert(key.clone());
        let saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.resume.insert(
            key,
            ResumeEntry {
                size: file.size,
                modified_ns: file.modified_ns,
                position,
                duration,
                saved_at,
            },
        );
        trim_resume(&mut self.resume);
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            language: self.language,
            volume: self.volume,
            speed: self.speed,
            muted: self.muted,
            resume: self.resume.clone(),
        }
    }
}

fn trim_resume(resume: &mut BTreeMap<String, ResumeEntry>) {
    while resume.len() > MAX_RESUME_ENTRIES {
        if let Some(oldest) = resume
            .iter()
            .min_by_key(|(_, entry)| entry.saved_at)
            .map(|(key, _)| key.clone())
        {
            resume.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MediaKind;
    use std::path::PathBuf;

    fn file(size: u64, modified_ns: u128) -> MediaFile {
        MediaFile {
            path: PathBuf::from("C:/Videos/movie.mkv"),
            kind: MediaKind::Video,
            size,
            modified_ns,
        }
    }

    fn named_file(name: &str) -> MediaFile {
        MediaFile {
            path: PathBuf::from(format!("C:/Videos/{name}.mkv")),
            ..file(42, 123)
        }
    }

    #[test]
    fn resume_only_matches_the_same_file_and_unfinished_playback() {
        let mut state = AppState::default();
        state.record_position(&file(42, 123), 25.0, 120.0);
        assert_eq!(state.resume_position(&file(42, 123)), Some(25.0));
        assert_eq!(state.resume_position(&file(43, 123)), None);
        assert_eq!(state.resume_position(&file(42, 124)), None);
        state.record_position(&file(42, 123), 115.0, 120.0);
        assert_eq!(state.resume_position(&file(42, 123)), None);
    }

    #[test]
    fn invalid_positions_never_enter_the_resume_store() {
        let mut state = AppState::default();
        state.record_position(&file(1, 1), f64::NAN, 100.0);
        state.record_position(&file(1, 1), 20.0, f64::INFINITY);
        assert!(state.resume.is_empty());
    }

    #[test]
    fn settings_can_be_replaced_atomically_and_corruption_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut state = AppState {
            language: Language::Turkish,
            ..Default::default()
        };
        state.record_position(&file(42, 123), 25.0, 120.0);
        state.save(&path).unwrap();
        state.volume = 37.0;
        state.save(&path).unwrap();
        let loaded = AppState::load(&path).unwrap();
        assert_eq!(loaded.language, Language::Turkish);
        assert_eq!(loaded.volume, 37.0);
        assert_eq!(loaded.resume_position(&file(42, 123)), Some(25.0));
        std::fs::write(&path, b"{broken json").unwrap();
        assert!(AppState::load(&path).is_err());
    }

    #[test]
    fn stale_windows_merge_only_their_own_resume_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut first = AppState::load(&path).unwrap();
        let mut second = AppState::load(&path).unwrap();
        first.record_position(&named_file("first"), 25.0, 120.0);
        first.save(&path).unwrap();
        second.record_position(&named_file("second"), 35.0, 120.0);
        second.save(&path).unwrap();
        first.volume = 31.0;
        first.save(&path).unwrap();
        let merged = AppState::load(&path).unwrap();
        assert_eq!(merged.resume_position(&named_file("first")), Some(25.0));
        assert_eq!(merged.resume_position(&named_file("second")), Some(35.0));
        assert_eq!(merged.volume, 31.0);
    }

    #[test]
    fn completion_is_not_resurrected_by_a_stale_autosave() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut initial = AppState::default();
        initial.record_position(&file(42, 123), 25.0, 120.0);
        initial.save(&path).unwrap();
        let mut completed = AppState::load(&path).unwrap();
        let mut stale = AppState::load(&path).unwrap();
        completed.record_position(&file(42, 123), 115.0, 120.0);
        completed.save(&path).unwrap();
        stale.record_position(&file(42, 123), 25.0, 120.0);
        stale.save(&path).unwrap();
        stale.record_position(&named_file("other"), 35.0, 120.0);
        stale.save(&path).unwrap();
        stale.speed = 1.5;
        stale.save(&path).unwrap();
        let merged = AppState::load(&path).unwrap();
        assert_eq!(merged.resume_position(&file(42, 123)), None);
        assert_eq!(merged.resume_position(&named_file("other")), Some(35.0));
    }

    #[test]
    fn completion_deletes_a_record_created_after_this_window_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut playing = AppState::load(&path).unwrap();
        let mut completed = AppState::load(&path).unwrap();
        playing.record_position(&file(42, 123), 25.0, 120.0);
        playing.save(&path).unwrap();
        completed.record_position(&file(42, 123), 115.0, 120.0);
        completed.save(&path).unwrap();
        playing.save(&path).unwrap();
        assert_eq!(
            AppState::load(&path)
                .unwrap()
                .resume_position(&file(42, 123)),
            None
        );
    }

    #[test]
    fn unchanged_preferences_do_not_revert_another_windows_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        AppState::default().save(&path).unwrap();
        let mut first = AppState::load(&path).unwrap();
        let mut second = AppState::load(&path).unwrap();
        first.language = Language::Turkish;
        first.volume = 37.0;
        first.save(&path).unwrap();
        second.record_position(&named_file("second"), 25.0, 120.0);
        second.save(&path).unwrap();
        let merged = AppState::load(&path).unwrap();
        assert_eq!(merged.language, Language::Turkish);
        assert_eq!(merged.volume, 37.0);
        second.muted = true;
        second.save(&path).unwrap();
        first.speed = 1.5;
        first.save(&path).unwrap();
        let merged = AppState::load(&path).unwrap();
        assert_eq!(merged.language, Language::Turkish);
        assert_eq!(merged.volume, 37.0);
        assert_eq!(merged.speed, 1.5);
        assert!(merged.muted);
    }

    #[test]
    fn simultaneous_writers_preserve_each_others_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let threads: Vec<_> = (0..4)
            .map(|index| {
                let mut state = AppState::load(&path).unwrap();
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let media = named_file(&format!("window-{index}"));
                    barrier.wait();
                    for position in 20..30 {
                        state.record_position(&media, f64::from(position), 120.0);
                        state.save(&path).unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let merged = AppState::load(&path).unwrap();
        for index in 0..4 {
            assert_eq!(
                merged.resume_position(&named_file(&format!("window-{index}"))),
                Some(29.0)
            );
        }
    }
}
