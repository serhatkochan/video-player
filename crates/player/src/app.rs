use crate::{
    diagnostics,
    host::{HostEvent, VideoHost},
    i18n,
    mpv::{Mpv, PlaybackSnapshot},
    updates,
};
use eframe::egui::{self, Color32, RichText};
#[path = "ui.rs"]
mod player_ui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use video_player_core::{AppState, Language, MediaFile, MediaKind, MediaNeighbors, format_time};

enum BackgroundResult {
    File(u64, Result<MediaFile, String>),
    Update(Result<Option<String>, String>),
    Neighbors(u64, String, Result<MediaNeighbors, String>),
    DialogPath(u64, PathBuf),
}

pub struct PlayerApp {
    settings: AppState,
    settings_path: PathBuf,
    settings_can_save: bool,
    runtime: PathBuf,
    engine: Option<Mpv>,
    host: Option<VideoHost>,
    initialized: bool,
    current: Option<MediaFile>,
    snapshot: PlaybackSnapshot,
    sender: mpsc::Sender<BackgroundResult>,
    receiver: mpsc::Receiver<BackgroundResult>,
    generation: u64,
    settings_open: bool,
    fullscreen: bool,
    last_interaction: Instant,
    last_save: Instant,
    seek_position: f64,
    neighbors: MediaNeighbors,
    neighbors_generation: u64,
    neighbors_loading: bool,
    opening_file: bool,
    seeking: bool,
    error: Option<String>,
    last_engine_error: Option<String>,
    update_message: Option<String>,
    checking_update: bool,
    diagnostics: Vec<diagnostics::Diagnostic>,
    initial_file: Option<PathBuf>,
    smoke_report: Option<PathBuf>,
    started: Instant,
}

impl PlayerApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial_file: Option<PathBuf>,
        smoke_report: Option<PathBuf>,
    ) -> Self {
        player_ui::apply_theme(&cc.egui_ctx);
        let settings_path = directories::BaseDirs::new()
            .map(|dirs| dirs.data_local_dir().join("Video Player/settings.json"))
            .unwrap_or_else(|| PathBuf::from("settings.json"));
        let first_run = !settings_path.exists();
        let (mut settings, error, settings_can_save) = match AppState::load(&settings_path) {
            Ok(settings) => (settings, None, true),
            Err(error) => {
                let backup =
                    settings_path.with_extension(format!("corrupt-{}.json", std::process::id()));
                let preserved = std::fs::rename(&settings_path, &backup);
                let can_save = preserved.is_ok();
                (
                    AppState::default(),
                    Some(format!(
                        "{error:#}; {}",
                        preserved
                            .map(|_| backup.display().to_string())
                            .unwrap_or_else(|e| e.to_string())
                    )),
                    can_save,
                )
            }
        };
        if first_run {
            settings.language = i18n::system_language();
        }
        let (sender, receiver) = mpsc::channel();
        let now = Instant::now();
        Self {
            settings,
            settings_path,
            settings_can_save,
            runtime: runtime_directory(),
            engine: None,
            host: None,
            initialized: false,
            current: None,
            snapshot: PlaybackSnapshot::default(),
            sender,
            receiver,
            generation: 0,
            settings_open: false,
            fullscreen: false,
            last_interaction: now,
            last_save: now,
            seek_position: 0.0,
            neighbors: MediaNeighbors::default(),
            neighbors_generation: 0,
            neighbors_loading: false,
            opening_file: false,
            seeking: false,
            error,
            last_engine_error: None,
            update_message: None,
            checking_update: false,
            diagnostics: Vec::new(),
            initial_file,
            smoke_report,
            started: now,
        }
    }

    fn initialize(&mut self, frame: &eframe::Frame) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        let result = (|| -> anyhow::Result<(VideoHost, Mpv)> {
            let handle = frame.window_handle()?;
            let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                anyhow::bail!("Windows 11 is required")
            };
            let host = VideoHost::new(handle.hwnd.get())?;
            let engine = Mpv::new(host.hwnd(), &self.runtime)?;
            Ok((host, engine))
        })();
        match result {
            Ok((host, engine)) => {
                self.host = Some(host);
                self.engine = Some(engine);
                self.property("volume", &self.settings.volume.to_string());
                self.property("mute", if self.settings.muted { "yes" } else { "no" });
                self.property("speed", &self.settings.speed.to_string());
                if let Some(path) = self.initial_file.take() {
                    self.open_path(path);
                }
            }
            Err(error) => {
                self.error = Some(format!(
                    "{}\n{error:#}\n{}",
                    self.tr(
                        "Playback engine could not start.",
                        "Oynatma motoru başlatılamadı."
                    ),
                    self.tr(
                        "Install the complete package, including the runtime folder.",
                        "runtime klasörünü içeren tam kurulum paketini yükleyin."
                    )
                ))
            }
        }
    }

    fn tr(&self, english: &'static str, turkish: &'static str) -> &'static str {
        i18n::text(self.settings.language, english, turkish)
    }

    fn command(&mut self, args: &[&str]) {
        if let Some(engine) = &self.engine
            && let Err(error) = engine.command(args)
        {
            self.error = Some(error.to_string());
        }
    }

    fn property(&mut self, name: &str, value: &str) {
        if let Some(engine) = &self.engine
            && let Err(error) = engine.set_property(name, value)
        {
            self.error = Some(error.to_string());
        }
    }

    fn begin_media_open(&mut self) {
        self.opening_file = true;
        self.neighbors_loading = true;
        self.neighbors_generation += 1;
        self.neighbors = MediaNeighbors::default();
    }

    fn accept_neighbors(
        &mut self,
        generation: u64,
        key: &str,
        result: Result<MediaNeighbors, String>,
    ) {
        if generation != self.neighbors_generation
            || self.current.as_ref().is_none_or(|file| file.key() != key)
        {
            return;
        }
        self.neighbors = result.unwrap_or_default();
        self.neighbors_loading = false;
    }

    fn refresh_neighbors(&mut self) {
        self.neighbors_generation += 1;
        self.neighbors = MediaNeighbors::default();
        let Some(file) = self.current.clone() else {
            self.neighbors_loading = false;
            return;
        };
        self.neighbors_loading = true;
        let generation = self.neighbors_generation;
        let key = file.key();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = MediaNeighbors::scan(&file).map_err(|error| format!("{error:#}"));
            let _ = sender.send(BackgroundResult::Neighbors(generation, key, result));
        });
    }

    fn navigate_file(&mut self, previous: bool) {
        if self.opening_file || self.neighbors_loading {
            return;
        }
        let target = if previous {
            &self.neighbors.previous
        } else {
            &self.neighbors.next
        };
        if let Some(path) = target.clone() {
            self.open_path(path);
        }
    }

    fn navigation_ui(&mut self, ui: &mut egui::Ui, visible: bool) -> egui::Rect {
        let (video, buttons) = player_ui::navigation_layout(ui.max_rect(), visible);
        if !buttons[0].is_positive() {
            return video;
        }
        for (previous, rect) in [(true, buttons[0]), (false, buttons[1])] {
            let target = if previous {
                &self.neighbors.previous
            } else {
                &self.neighbors.next
            };
            let action = if previous {
                self.tr("Previous file", "Önceki dosya")
            } else {
                self.tr("Next file", "Sonraki dosya")
            };
            let label = match target {
                Some(path) => format!(
                    "{action}: {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
                None if self.opening_file || self.neighbors_loading => {
                    format!("{action} · {}", self.tr("Loading…", "Yükleniyor…"))
                }
                None => format!(
                    "{action} · {}",
                    self.tr("No more files in this folder", "Bu yönde başka dosya yok")
                ),
            };
            let enabled = target.is_some() && !self.opening_file && !self.neighbors_loading;
            let response = player_ui::file_button(ui, rect, previous, enabled, &label);
            if response.hovered() || response.contains_pointer() {
                self.last_interaction = Instant::now();
            }
            if response.clicked() {
                self.navigate_file(previous);
            }
        }
        video
    }

    fn open_path(&mut self, path: PathBuf) {
        let subtitle = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| {
                video_player_core::SUBTITLE_EXTENSIONS
                    .contains(&value.to_ascii_lowercase().as_str())
            });
        if !subtitle {
            self.begin_media_open();
        } else if self.opening_file {
            self.opening_file = false;
            self.refresh_neighbors();
        }
        self.generation += 1;
        let generation = self.generation;
        let sender = self.sender.clone();
        thread::spawn(move || {
            let file = MediaFile::open(path).map_err(|e| format!("{e:#}"));
            let _ = sender.send(BackgroundResult::File(generation, file));
        });
    }

    fn file_dialog(&mut self, subtitle: bool) {
        let generation = self.generation;
        let sender = self.sender.clone();
        let title = if subtitle {
            self.tr("Load subtitles", "Altyazı yükle")
        } else {
            self.tr("Open media", "Medya aç")
        };
        thread::spawn(move || {
            let dialog = rfd::FileDialog::new().set_title(title);
            let dialog = if subtitle {
                dialog.add_filter("Subtitles", &["srt", "ass", "ssa", "vtt"])
            } else {
                dialog.add_filter(
                    "Media",
                    &[
                        "mp4", "m4v", "mkv", "mov", "webm", "avi", "wmv", "mp3", "aac", "m4a",
                        "flac", "wav",
                    ],
                )
            };
            if let Some(path) = dialog.pick_file() {
                let _ = sender.send(BackgroundResult::DialogPath(generation, path));
            }
        });
    }

    fn load_file(&mut self, file: MediaFile) {
        if self.engine.is_none() {
            self.error.get_or_insert_with(|| {
                "The playback engine is unavailable. Install the complete package.".into()
            });
            return;
        }
        let Some(path) = file.path.to_str() else {
            self.error = Some(
                self.tr(
                    "The file path is not valid Unicode.",
                    "Dosya yolu geçerli Unicode değil.",
                )
                .into(),
            );
            return;
        };
        if file.kind == MediaKind::Subtitle {
            if self.current.is_some() {
                self.command(&["sub-add", path, "select"]);
            } else {
                self.error = Some(
                    self.tr(
                        "Open a video before loading subtitles.",
                        "Altyazı yüklemeden önce bir video açın.",
                    )
                    .into(),
                );
            }
            return;
        }
        self.remember_position();
        let position = self.settings.resume_position(&file).unwrap_or(0.0);
        self.error = None;
        self.last_engine_error = self.snapshot.error.clone();
        self.command(&[
            "loadfile",
            path,
            "replace",
            "-1",
            &format!("start={position},pause=no"),
        ]);
        self.current = Some(file);
        self.refresh_neighbors();
        self.settings_open = false;
        self.seeking = false;
        self.last_interaction = Instant::now();
    }

    fn remember_position(&mut self) {
        if let Some(file) = &self.current
            && snapshot_matches(&self.snapshot, &file.path)
        {
            self.settings.record_position(
                file,
                if self.snapshot.eof {
                    self.snapshot.duration
                } else {
                    self.snapshot.position
                },
                self.snapshot.duration,
            );
        }
    }

    fn save(&mut self) {
        self.remember_position();
        if self.settings_can_save
            && let Err(error) = self.settings.save(&self.settings_path)
        {
            self.error = Some(format!("{error:#}"));
        }
        self.last_save = Instant::now();
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        self.last_interaction = Instant::now();
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
    }

    fn handle_event(&mut self, event: HostEvent, ctx: &egui::Context) {
        self.last_interaction = Instant::now();
        match event {
            HostEvent::TogglePause => self.toggle_pause(),
            HostEvent::ToggleFullscreen => self.toggle_fullscreen(ctx),
            HostEvent::ExitFullscreen => {
                if self.fullscreen {
                    self.toggle_fullscreen(ctx);
                }
            }
            HostEvent::ShowControls => {}
            HostEvent::Seek(seconds) => {
                self.command(&["seek", &seconds.to_string(), "relative+exact"])
            }
            HostEvent::OpenFile => self.file_dialog(false),
            HostEvent::DropFile(path) => self.open_path(path),
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        if let Some(engine) = &self.engine {
            self.snapshot = engine.snapshot();
        }
        let mut interactions = Vec::new();
        if let Some(engine) = &self.engine {
            while let Some(event) = engine.poll_event() {
                interactions.push(event);
            }
        }
        if let Some(host) = &self.host {
            while let Some(event) = host.poll_event() {
                interactions.push(event);
            }
        }
        for event in interactions {
            self.handle_event(event, ctx);
        }
        while let Ok(result) = self.receiver.try_recv() {
            match result {
                BackgroundResult::File(generation, result) if generation == self.generation => {
                    self.opening_file = false;
                    match result {
                        Ok(file) => self.load_file(file),
                        Err(error) => {
                            self.error = Some(error);
                            self.refresh_neighbors();
                        }
                    }
                }
                BackgroundResult::File(_, _) => {}
                BackgroundResult::Neighbors(generation, key, result) => {
                    self.accept_neighbors(generation, &key, result);
                }
                BackgroundResult::DialogPath(generation, path) if generation == self.generation => {
                    self.open_path(path);
                }
                BackgroundResult::DialogPath(_, _) => {}
                BackgroundResult::Update(result) => {
                    self.checking_update = false;
                    self.update_message = Some(match result {
                        Ok(Some(version))
                            if updates::newer_than(&version, env!("CARGO_PKG_VERSION")) =>
                        {
                            format!("{} {version}", self.tr("New version:", "Yeni sürüm:"))
                        }
                        Ok(Some(_)) => self
                            .tr(
                                "You have the latest release.",
                                "En yeni sürümü kullanıyorsunuz.",
                            )
                            .into(),
                        Ok(None) => self
                            .tr(
                                "No published release yet.",
                                "Henüz yayımlanmış bir sürüm yok.",
                            )
                            .into(),
                        Err(error) => format!(
                            "{} {error}",
                            self.tr("Update check failed:", "Güncelleme kontrolü başarısız:")
                        ),
                    });
                }
            }
        }
        let keyboard_input = ctx.egui_wants_keyboard_input();
        let events = ctx.input(|input| {
            let mut events = Vec::new();
            if input.pointer.delta() != egui::Vec2::ZERO || input.pointer.any_pressed() {
                events.push(HostEvent::ShowControls);
            }
            if !keyboard_input {
                for (key, event) in [
                    (egui::Key::Space, HostEvent::TogglePause),
                    (egui::Key::F, HostEvent::ToggleFullscreen),
                    (egui::Key::Escape, HostEvent::ExitFullscreen),
                    (egui::Key::ArrowLeft, HostEvent::Seek(-5.0)),
                    (egui::Key::ArrowRight, HostEvent::Seek(5.0)),
                ] {
                    if input.key_pressed(key) {
                        events.push(event);
                    }
                }
            }
            if input.modifiers.ctrl && input.key_pressed(egui::Key::O) {
                events.push(HostEvent::OpenFile);
            }
            events
        });
        for event in events {
            self.handle_event(event, ctx);
        }
        let dropped = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect::<Vec<_>>()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.open_path(path);
        }
        if self.snapshot.error != self.last_engine_error {
            if let Some(error) = &self.snapshot.error {
                self.error = Some(error.clone());
            }
            self.last_engine_error = self.snapshot.error.clone();
        }
        if self.last_save.elapsed() >= Duration::from_secs(5) {
            self.save();
        }
        if self.smoke_report.is_some() && self.started.elapsed() >= Duration::from_secs(6) {
            let report = self.smoke_report.take().unwrap();
            let output = serde_json::json!({"engine_started": self.engine.is_some(), "native_surface": self.host.is_some(),
                "position": self.snapshot.position, "duration": self.snapshot.duration, "hwdec": self.snapshot.hwdec,
                "codec": self.snapshot.video_codec, "output_colorspace": self.snapshot.output_colorspace, "error": self.error,
                "runtime_configuration": self.engine.as_ref().map(Mpv::runtime_configuration),
                "drop_target_ready": self.host.as_ref().is_some_and(VideoHost::native_drop_target_ready)});
            if let Err(error) = std::fs::write(report, output.to_string()) {
                eprintln!("{error}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_sized(
                    [108.0, 34.0],
                    egui::Button::new(self.tr("Open file", "Dosya aç")),
                )
                .clicked()
            {
                self.file_dialog(false);
            }
            let filename_width = (ui.available_width() - 108.0).max(0.0);
            ui.allocate_ui_with_layout(
                egui::vec2(filename_width, 34.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(filename_width);
                    if let Some(file) = &self.current {
                        ui.add(
                            egui::Label::new(
                                RichText::new(file.display_name()).color(player_ui::SECONDARY),
                            )
                            .truncate(),
                        );
                    }
                },
            );
            if ui
                .add_sized(
                    [92.0, 34.0],
                    egui::Button::new(self.tr("Settings", "Ayarlar")).selected(self.settings_open),
                )
                .clicked()
            {
                self.settings_open = !self.settings_open;
                if self.settings_open {
                    self.diagnostics = diagnostics::check(self.settings.language);
                }
            }
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        use player_ui::{Icon, icon_button};
        let available = self.current.is_some() && !self.snapshot.idle;
        ui.add_enabled_ui(available, |ui| {
            if !self.seeking {
                self.seek_position = self.snapshot.position;
            }
            let response = player_ui::timeline(ui, &mut self.seek_position, self.snapshot.duration);
            if response.drag_started() {
                self.seeking = true;
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                self.command(&["seek", &self.seek_position.to_string(), "absolute+exact"]);
                self.seeking = false;
            }
            ui.horizontal(|ui| {
                if icon_button(
                    ui,
                    Icon::Back,
                    self.tr("Back 5 seconds (Left)", "5 saniye geri (Sol ok)"),
                    false,
                )
                .clicked()
                {
                    self.command(&["seek", "-5", "relative+exact"]);
                }
                let paused = !available || self.snapshot.paused || self.snapshot.eof;
                if icon_button(
                    ui,
                    if paused { Icon::Play } else { Icon::Pause },
                    if paused {
                        self.tr("Play (Space)", "Oynat (Boşluk)")
                    } else {
                        self.tr("Pause (Space)", "Duraklat (Boşluk)")
                    },
                    true,
                )
                .clicked()
                {
                    self.toggle_pause();
                }
                if icon_button(
                    ui,
                    Icon::Forward,
                    self.tr("Forward 5 seconds (Right)", "5 saniye ileri (Sağ ok)"),
                    false,
                )
                .clicked()
                {
                    self.command(&["seek", "5", "relative+exact"]);
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "{} / {}",
                        format_time(self.seek_position),
                        format_time(self.snapshot.duration)
                    ))
                    .size(12.0)
                    .color(player_ui::SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(
                        ui,
                        if self.fullscreen {
                            Icon::ExitFullscreen
                        } else {
                            Icon::Fullscreen
                        },
                        self.tr("Fullscreen (F)", "Tam ekran (F)"),
                        false,
                    )
                    .clicked()
                    {
                        self.toggle_fullscreen(ui.ctx());
                    }
                    ui.add_space(8.0);
                    if ui
                        .add_sized(
                            [26.0, 30.0],
                            egui::Button::new("+").frame_when_inactive(false),
                        )
                        .on_hover_text(self.tr("Faster", "Hızlandır"))
                        .clicked()
                    {
                        self.change_speed(0.25);
                    }
                    ui.add_sized(
                        [48.0, 30.0],
                        egui::Label::new(
                            RichText::new(format!("{:.2}×", self.settings.speed)).size(12.0),
                        ),
                    );
                    if ui
                        .add_sized(
                            [26.0, 30.0],
                            egui::Button::new("−").frame_when_inactive(false),
                        )
                        .on_hover_text(self.tr("Slower", "Yavaşlat"))
                        .clicked()
                    {
                        self.change_speed(-0.25);
                    }
                    ui.add_space(12.0);
                    let volume_response = ui
                        .scope(|ui| {
                            ui.spacing_mut().slider_width = 80.0;
                            ui.spacing_mut().interact_size.y = 24.0;
                            ui.add(
                                egui::Slider::new(&mut self.settings.volume, 0.0..=100.0)
                                    .show_value(false),
                            )
                        })
                        .inner
                        .on_hover_text(self.tr("Volume", "Ses düzeyi"));
                    if volume_response.changed() {
                        self.property("volume", &self.settings.volume.to_string());
                    }
                    if icon_button(
                        ui,
                        if self.settings.muted {
                            Icon::Muted
                        } else {
                            Icon::Volume
                        },
                        if self.settings.muted {
                            self.tr("Unmute", "Sesi aç")
                        } else {
                            self.tr("Mute", "Sessize al")
                        },
                        false,
                    )
                    .clicked()
                    {
                        self.settings.muted = !self.settings.muted;
                        self.property("mute", if self.settings.muted { "yes" } else { "no" });
                    }
                });
            });
        });
    }

    fn toggle_pause(&mut self) {
        if self.snapshot.eof {
            self.command(&["seek", "0", "absolute+exact"]);
            self.property("pause", "no");
        } else {
            self.command(&["cycle", "pause"]);
        }
    }

    fn change_speed(&mut self, delta: f64) {
        self.settings.speed = (self.settings.speed + delta).clamp(0.25, 4.0);
        self.property("speed", &self.settings.speed.to_string());
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading(self.tr("Settings", "Ayarlar"));
            ui.horizontal(|ui| {
                ui.label(self.tr("Language", "Dil"));
                let changed = ui.selectable_value(&mut self.settings.language, Language::Turkish, "Türkçe").changed()
                    | ui.selectable_value(&mut self.settings.language, Language::English, "English").changed();
                if changed { self.diagnostics = diagnostics::check(self.settings.language); }
            });
            ui.separator();
            ui.heading(self.tr("Audio & subtitles", "Ses ve altyazı"));
            let tracks = self.snapshot.tracks.clone();
            for kind in ["audio", "sub"] {
                ui.label(if kind == "audio" { self.tr("Audio track", "Ses parçası") } else { self.tr("Subtitle track", "Altyazı parçası") });
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_label(!tracks.iter().any(|track| track.kind == kind && track.selected), self.tr("Off", "Kapalı")).clicked() {
                        self.property(if kind == "audio" { "aid" } else { "sid" }, "no");
                    }
                    for track in tracks.iter().filter(|track| track.kind == kind) {
                        let title = format!("{} · {} {}", track.id, track.lang, track.title);
                        if ui.selectable_label(track.selected, title.trim()).clicked() { self.property(if kind == "audio" { "aid" } else { "sid" }, &track.id.to_string()); }
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(self.current.is_some(), egui::Button::new(self.tr("Load subtitles…", "Altyazı yükle…"))).clicked() { self.file_dialog(true); }
                ui.label(self.tr("Subtitle delay", "Altyazı gecikmesi"));
                let mut delay = self.snapshot.subtitle_delay;
                if ui.add(egui::DragValue::new(&mut delay).speed(0.1).range(-120.0..=120.0).suffix(" s")).changed() { self.property("sub-delay", &delay.to_string()); }
            });
            ui.separator();
            ui.heading(self.tr("Windows integration", "Windows entegrasyonu"));
            if ui.button(self.tr("Choose default player in Windows", "Windows'ta varsayılan oynatıcıyı seç")).clicked()
                && let Err(error) = open::that("ms-settings:defaultapps") {
                self.error = Some(error.to_string());
            }
            if ui.button(self.tr("Check Explorer thumbnails", "Explorer küçük resimlerini kontrol et")).clicked() { self.diagnostics = diagnostics::check(self.settings.language); }
            for diagnostic in &self.diagnostics {
                ui.colored_label(if diagnostic.ok { Color32::from_rgb(112, 220, 160) } else { Color32::from_rgb(255, 192, 112) }, format!("{} {}", if diagnostic.ok { "✓" } else { "!" }, diagnostic.message));
            }
            ui.separator();
            ui.heading(self.tr("About", "Hakkında"));
            ui.label(format!("Video Player {} · MIT", env!("CARGO_PKG_VERSION")));
            ui.label(self.tr("Preferences and playback positions are saved on this computer.", "Tercihler ve devam etme bilgileri bu bilgisayarda saklanır."));
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.checking_update, egui::Button::new(self.tr("Check for updates", "Güncellemeleri kontrol et"))).clicked() {
                    self.checking_update = true;
                    self.update_message = Some(self.tr("Checking…", "Kontrol ediliyor…").into());
                    let sender = self.sender.clone();
                    thread::spawn(move || { let _ = sender.send(BackgroundResult::Update(updates::latest_version().map_err(|e| format!("{e:#}")))); });
                }
                if ui.button("GitHub Releases ↗").clicked()
                    && let Err(error) = open::that(updates::RELEASES_URL) {
                    self.error = Some(error.to_string());
                }
            });
            if let Some(message) = &self.update_message { ui.label(message); }
            if !self.snapshot.video_codec.is_empty() {
                ui.small(format!("{} · {} · {}", self.snapshot.video_codec, self.snapshot.hwdec, self.snapshot.output_colorspace));
            }
            ui.small(self.tr("Shortcuts: Space · pause, ← → · seek, F · fullscreen, Esc · exit fullscreen, Ctrl+O · open.",
                "Kısayollar: Boşluk · duraklat, ← → · sar, F · tam ekran, Esc · tam ekrandan çık, Ctrl+O · aç."));
        });
    }
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.initialize(frame);
        let ctx = ui.ctx().clone();
        self.tick(&ctx);
        let show_controls = !self.fullscreen
            || self.settings_open
            || self.snapshot.paused
            || self.snapshot.idle
            || self.error.is_some()
            || self.last_interaction.elapsed() < Duration::from_secs(3);
        if show_controls {
            egui::Panel::top("header")
                .frame(
                    egui::Frame::NONE
                        .fill(player_ui::PANEL)
                        .inner_margin(egui::Margin::symmetric(20, 10)),
                )
                .show(ui, |ui| self.header(ui));
            egui::Panel::bottom("controls")
                .frame(
                    egui::Frame::NONE
                        .fill(player_ui::PANEL)
                        .inner_margin(egui::Margin::symmetric(20, 12)),
                )
                .show(ui, |ui| self.controls(ui));
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(player_ui::BACKGROUND)
                    .inner_margin(16.0),
            )
            .show(ui, |ui| {
                if self.settings_open {
                    self.settings_ui(ui);
                } else if self.error.is_some() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(48.0);
                        ui.heading(self.tr("Something needs attention", "Bir sorun var"));
                        ui.label(self.error.as_deref().unwrap_or_default());
                        if ui.button(self.tr("Dismiss", "Kapat")).clicked() {
                            self.error = None;
                        }
                        if ui
                            .button(self.tr("Open another file", "Başka dosya aç"))
                            .clicked()
                        {
                            self.file_dialog(false);
                        }
                    });
                } else if self.current.is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space((ui.available_height() * 0.28).max(24.0));
                        let (icon_rect, _) =
                            ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                        player_ui::paint_icon(
                            ui.painter(),
                            icon_rect,
                            player_ui::Icon::Play,
                            player_ui::ACCENT,
                        );
                        ui.add_space(12.0);
                        ui.heading(self.tr("Open a video", "Bir video aç"));
                        ui.label(self.tr(
                            "Drop a video here, or choose a file.",
                            "Bir videoyu buraya sürükleyin veya dosya seçin.",
                        ));
                        ui.add_space(14.0);
                        if ui
                            .add_sized(
                                [180.0, 44.0],
                                egui::Button::new(self.tr("Open file", "Dosya aç")),
                            )
                            .clicked()
                        {
                            self.file_dialog(false);
                        }
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new("MP4 · MKV · MOV · WebM · AVI · WMV")
                                .size(12.0)
                                .color(player_ui::SECONDARY),
                        );
                    });
                } else if self
                    .current
                    .as_ref()
                    .is_some_and(|file| file.kind == MediaKind::Audio)
                {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.25);
                        let (icon_rect, _) =
                            ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                        player_ui::paint_icon(
                            ui.painter(),
                            icon_rect,
                            player_ui::Icon::Music,
                            player_ui::ACCENT,
                        );
                        ui.add_space(12.0);
                        ui.heading(self.current.as_ref().unwrap().display_name());
                        ui.label(self.tr("Audio playback", "Ses oynatılıyor"));
                    });
                }
                let rect = self.navigation_ui(
                    ui,
                    show_controls
                        && self.current.is_some()
                        && !self.settings_open
                        && self.error.is_none(),
                );
                if let Some(host) = &self.host {
                    let scale = ctx.pixels_per_point();
                    let left = (rect.left() * scale).round() as i32;
                    let top = (rect.top() * scale).round() as i32;
                    let right = (rect.right() * scale).round() as i32;
                    let bottom = (rect.bottom() * scale).round() as i32;
                    host.place(left, top, right - left, bottom - top);
                    host.show(
                        right > left
                            && bottom > top
                            && !self.settings_open
                            && self.error.is_none()
                            && self
                                .current
                                .as_ref()
                                .is_some_and(|file| file.kind == MediaKind::Video)
                            && !self.snapshot.idle
                            && !egui::Popup::is_any_open(&ctx),
                    );
                }
            });
    }
}

impl Drop for PlayerApp {
    fn drop(&mut self) {
        self.save();
        drop(self.engine.take());
        drop(self.host.take());
    }
}

fn runtime_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("VIDEO_PLAYER_RUNTIME") {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe() {
        let parent = exe.parent().unwrap_or(Path::new("."));
        if parent.join("libmpv-2.dll").is_file() {
            return parent.to_path_buf();
        }
        let path = parent.join("runtime");
        if path.is_dir() {
            return path;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime")
}

fn snapshot_matches(snapshot: &PlaybackSnapshot, path: &Path) -> bool {
    snapshot.path.as_deref().is_some_and(|playing| {
        let normalized = |path: &str| {
            path.trim_start_matches(r"\\?\")
                .replace('/', "\\")
                .to_lowercase()
        };
        normalized(playing) == normalized(&path.to_string_lossy())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_positions_never_transfer_to_a_different_file() {
        let snapshot = PlaybackSnapshot {
            path: Some(r"C:\Videos\İstanbul.mkv".into()),
            ..Default::default()
        };
        assert!(snapshot_matches(
            &snapshot,
            Path::new(r"\\?\C:\Videos\İstanbul.mkv")
        ));
        assert!(!snapshot_matches(
            &snapshot,
            Path::new(r"C:\Videos\another.mkv")
        ));
    }

    fn layout_app(language: Language) -> PlayerApp {
        let (sender, receiver) = mpsc::channel();
        let now = Instant::now();
        let mut settings = AppState::default();
        settings.language = language;
        PlayerApp {
            settings,
            settings_path: PathBuf::new(),
            settings_can_save: false,
            runtime: PathBuf::new(),
            engine: None,
            host: None,
            initialized: true,
            current: Some(MediaFile {
                path: PathBuf::from("İstanbul - çok uzun video dosyası adı - deneme.mkv"),
                kind: MediaKind::Video,
                size: 1,
                modified_ns: 0,
            }),
            snapshot: PlaybackSnapshot {
                duration: 36000.0,
                position: 3600.0,
                idle: false,
                ..Default::default()
            },
            sender,
            receiver,
            generation: 0,
            settings_open: false,
            fullscreen: false,
            last_interaction: now,
            last_save: now,
            seek_position: 0.0,
            neighbors: MediaNeighbors::default(),
            neighbors_generation: 0,
            neighbors_loading: false,
            opening_file: false,
            seeking: false,
            error: None,
            last_engine_error: None,
            update_message: None,
            checking_update: false,
            diagnostics: Vec::new(),
            initial_file: None,
            smoke_report: None,
            started: now,
        }
    }

    #[test]
    fn toolbar_and_controls_fit_minimum_window_in_both_languages() {
        for width in [640.0, 960.0, 1440.0] {
            for language in [Language::Turkish, Language::English] {
                let ctx = egui::Context::default();
                player_ui::apply_theme(&ctx);
                let mut app = layout_app(language);
                for _ in 0..2 {
                    ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 420.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            egui::Panel::top("header-test")
                                .frame(
                                    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(20, 10)),
                                )
                                .show(ui, |ui| {
                                    app.header(ui);
                                    assert!(
                                        ui.min_rect().right() <= width - 19.0,
                                        "header overflow at {width}"
                                    );
                                });
                            egui::Panel::bottom("controls-test")
                                .frame(
                                    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(20, 12)),
                                )
                                .show(ui, |ui| {
                                    app.controls(ui);
                                    assert!(
                                        ui.min_rect().right() <= width - 19.0,
                                        "controls overflow at {width}"
                                    );
                                });
                        },
                    )
                    .drop_without_applying_deltas();
                }
            }
        }
    }

    #[test]
    fn requesting_a_new_media_file_disables_old_neighbors_immediately() {
        let mut app = layout_app(Language::Turkish);
        app.neighbors.next = Some(PathBuf::from("next.mp4"));
        let old_generation = app.neighbors_generation;
        app.begin_media_open();
        assert!(app.opening_file);
        assert!(app.neighbors_loading);
        assert_eq!(app.neighbors, MediaNeighbors::default());
        assert!(app.neighbors_generation > old_generation);
    }

    #[test]
    fn current_folder_scan_enables_its_neighbors() {
        let mut app = layout_app(Language::Turkish);
        app.neighbors_loading = true;
        let key = app.current.as_ref().unwrap().key();
        let neighbors = MediaNeighbors {
            previous: None,
            next: Some(PathBuf::from("next.mp4")),
        };
        app.accept_neighbors(app.neighbors_generation, &key, Ok(neighbors.clone()));
        assert_eq!(app.neighbors, neighbors);
        assert!(!app.neighbors_loading);
    }

    #[test]
    fn stale_generation_and_wrong_file_scan_cannot_replace_neighbors() {
        let mut app = layout_app(Language::Turkish);
        app.neighbors_generation = 5;
        app.neighbors_loading = true;
        let key = app.current.as_ref().unwrap().key();
        let stale = MediaNeighbors {
            previous: None,
            next: Some(PathBuf::from("wrong.mp4")),
        };
        app.accept_neighbors(4, &key, Ok(stale.clone()));
        app.accept_neighbors(5, "another-file.mkv", Ok(stale));
        assert_eq!(app.neighbors, MediaNeighbors::default());
        assert!(app.neighbors_loading);
    }

    #[test]
    fn folder_scan_error_disables_navigation_without_interrupting_playback() {
        let mut app = layout_app(Language::Turkish);
        app.neighbors_loading = true;
        let key = app.current.as_ref().unwrap().key();
        app.accept_neighbors(
            app.neighbors_generation,
            &key,
            Err("Unreadable folder".into()),
        );
        assert!(!app.neighbors_loading);
        assert!(app.error.is_none());
        assert!(app.current.is_some());
    }

    #[test]
    fn subtitle_request_preserves_current_folder_navigation() {
        let mut app = layout_app(Language::Turkish);
        app.neighbors.next = Some(PathBuf::from("next.mp4"));
        let before = app.neighbors.clone();
        let generation = app.neighbors_generation;
        app.open_path(PathBuf::from("altyazı.SRT"));
        assert_eq!(app.neighbors, before);
        assert_eq!(app.neighbors_generation, generation);
        assert!(!app.opening_file);
    }

    #[test]
    fn selected_dialog_file_opens_only_for_its_current_generation() {
        let mut app = layout_app(Language::Turkish);
        app.generation = 8;
        app.sender
            .send(BackgroundResult::DialogPath(7, PathBuf::from("stale.mp4")))
            .unwrap();
        app.tick(&egui::Context::default());
        assert_eq!(app.generation, 8);
        app.sender
            .send(BackgroundResult::DialogPath(
                8,
                PathBuf::from("selected.mp4"),
            ))
            .unwrap();
        app.tick(&egui::Context::default());
        assert_eq!(app.generation, 9);
    }
}
