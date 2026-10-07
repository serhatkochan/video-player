//! Exercise the real bundled decoder and D3D11 renderer without the egui shell.
//! cargo run -p video-player --example media_probe -- <media-file> [runtime-directory] [--exercise-controls] [--subtitle <file>]

#[path = "../src/host.rs"]
mod host;
#[path = "../src/mpv.rs"]
mod mpv;

use anyhow::{Context, Result, bail};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next().context("Usage: media_probe <media-file> [runtime-directory] [--exercise-controls] [--subtitle <file>], or --runtime-info [runtime-directory]")?;
    if first == "--runtime-info" {
        let runtime = args
            .next()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("runtime"));
        let video = host::VideoHost::new(0)?;
        let player = mpv::Mpv::new(video.hwnd(), &runtime)?;
        let configuration = player.runtime_configuration();
        if configuration.is_empty() {
            bail!("mpv-configuration is unavailable");
        }
        println!("{configuration}");
        return Ok(());
    }
    let media = PathBuf::from(first)
        .canonicalize()
        .context("Resolve media file")?;
    let mut args = args.peekable();
    let runtime = if args
        .peek()
        .is_some_and(|arg| !arg.to_string_lossy().starts_with("--"))
    {
        PathBuf::from(args.next().unwrap())
    } else {
        PathBuf::from("runtime")
    };
    let mut exercise = false;
    let mut subtitle = None;
    while let Some(arg) = args.next() {
        if arg == "--exercise-controls" {
            exercise = true;
        } else if arg == "--subtitle" {
            if subtitle.is_some() {
                bail!("Only one external subtitle can be loaded per probe");
            }
            subtitle = Some(
                PathBuf::from(args.next().context("--subtitle requires a file")?)
                    .canonicalize()
                    .context("Resolve external subtitle file")?,
            );
        } else {
            bail!("Unknown probe argument: {}", arg.to_string_lossy());
        }
    }
    let path = media
        .to_str()
        .context("The media path must be valid Unicode")?;
    let video = host::VideoHost::new(0)?;
    video.place(40, 40, 960, 540);
    video.show(true);
    let player = mpv::Mpv::new(video.hwnd(), &runtime)?;
    println!("runtime configuration: {}", player.runtime_configuration());
    player.command(&["loadfile", path, "replace"])?;
    player.set_property("pause", "no")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut progressed = false;
    let mut exercised = false;
    let mut subtitle_loaded = false;
    let mut printed_tracks = false;
    let mut next_print = Instant::now();
    while Instant::now() < deadline && !video.closed() {
        host::pump_messages();
        while let Some(event) = player.poll_event().or_else(|| video.poll_event()) {
            match event {
                host::HostEvent::TogglePause => player.command(&["cycle", "pause"])?,
                host::HostEvent::Seek(delta) => {
                    player.command(&["seek", &delta.to_string(), "relative"])?
                }
                _ => {}
            }
        }
        let snapshot = player.snapshot();
        if let Some(error) = &snapshot.error {
            bail!("Decoder/renderer error: {error}");
        }
        progressed |= snapshot.position > 0.05 && snapshot.duration > 0.0;
        if !printed_tracks && !snapshot.tracks.is_empty() {
            for track in &snapshot.tracks {
                println!(
                    "track id={} type={} lang={} title={} selected={}",
                    track.id, track.kind, track.lang, track.title, track.selected
                );
            }
            printed_tracks = true;
        }
        if let Some(subtitle) = &subtitle
            && !subtitle_loaded
            && snapshot.position > 0.2
        {
            let existing_ids: Vec<_> = snapshot
                .tracks
                .iter()
                .filter(|track| track.kind == "sub")
                .map(|track| track.id)
                .collect();
            let subtitle_path = subtitle
                .to_str()
                .context("The subtitle path must be valid Unicode")?;
            player.command(&["sub-add", subtitle_path, "select"])?;
            wait_for(
                &player,
                |s| {
                    s.tracks.iter().any(|track| {
                        track.kind == "sub" && track.selected && !existing_ids.contains(&track.id)
                    })
                },
                "new external subtitle selected",
            )?;
            let selected = player
                .snapshot()
                .tracks
                .into_iter()
                .find(|track| {
                    track.kind == "sub" && track.selected && !existing_ids.contains(&track.id)
                })
                .context("External subtitle selection disappeared")?;
            println!(
                "subtitle-added id={} selected={} filename={}",
                selected.id,
                selected.selected,
                subtitle.file_name().unwrap().to_string_lossy()
            );
            println!(
                "track id={} type={} lang={} title={} selected={}",
                selected.id, selected.kind, selected.lang, selected.title, selected.selected
            );
            subtitle_loaded = true;
            println!("PASS: external subtitle loaded and selected.");
        }
        if exercise && !exercised && snapshot.position > 0.5 {
            if !video.native_drop_target_ready() {
                bail!("The application has not replaced libmpv's native file drop target");
            }
            player.set_property("pause", "yes")?;
            player.set_property("volume", "37")?;
            player.set_property("mute", "yes")?;
            player.set_property("speed", "1.25")?;
            player.set_property("sub-delay", "0.35")?;
            wait_for(
                &player,
                |s| {
                    s.paused
                        && s.mute
                        && (s.volume - 37.0).abs() < 0.01
                        && (s.speed - 1.25).abs() < 0.01
                        && (s.subtitle_delay - 0.35).abs() < 0.01
                },
                "pause/volume/mute/speed/subtitle delay",
            )?;
            for (kind, property) in [("audio", "aid"), ("sub", "sid")] {
                let previous = snapshot
                    .tracks
                    .iter()
                    .find(|track| track.kind == kind && track.selected)
                    .map(|track| track.id);
                if let Some(track) = snapshot
                    .tracks
                    .iter()
                    .find(|track| track.kind == kind && !track.selected)
                    .or_else(|| snapshot.tracks.iter().find(|track| track.kind == kind))
                {
                    let id = track.id;
                    player.set_property(property, &id.to_string())?;
                    wait_for(
                        &player,
                        |s| {
                            s.tracks
                                .iter()
                                .any(|track| track.kind == kind && track.id == id && track.selected)
                        },
                        &format!("{kind} track selection"),
                    )?;
                    if let Some(previous) = previous
                        && previous != id
                    {
                        println!("track-switch type={kind} from={previous} to={id} selected=true");
                    }
                }
            }
            player.command(&["keypress", "SPACE"])?;
            wait_interaction(&player, host::HostEvent::TogglePause)?;
            player.command(&["seek", "1", "absolute+exact"])?;
            wait_for(
                &player,
                |s| (s.position - 1.0).abs() < 0.05,
                "absolute seek",
            )?;
            player.set_property("mute", "no")?;
            player.set_property("pause", "no")?;
            exercised = true;
            println!(
                "PASS: pause, volume, mute, speed, subtitle delay, track selection, input routing and seek."
            );
        }
        if Instant::now() >= next_print {
            println!(
                "position={:.3} duration={:.3} codec={} hwdec={} output={} tracks={} idle={} eof={}",
                snapshot.position,
                snapshot.duration,
                snapshot.video_codec,
                snapshot.hwdec,
                snapshot.output_colorspace,
                snapshot.tracks.len(),
                snapshot.idle,
                snapshot.eof
            );
            next_print = Instant::now() + Duration::from_secs(1);
        }
        if progressed && (snapshot.position >= 3.0 || snapshot.eof) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !progressed {
        bail!("No decoded playback progress was observed within 20 seconds");
    }
    if subtitle.is_some() && !subtitle_loaded {
        bail!("The requested external subtitle was not loaded and selected");
    }
    if exercise && !exercised {
        bail!("Playback ended before controls could be exercised");
    }
    println!("PASS: playback advanced. HDR correctness still requires display validation.");
    drop(player);
    drop(video);
    Ok(())
}

fn wait_for(
    player: &mpv::Mpv,
    predicate: impl Fn(&mpv::PlaybackSnapshot) -> bool,
    behavior: &str,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        host::pump_messages();
        let snapshot = player.snapshot();
        if let Some(error) = &snapshot.error {
            bail!("{behavior}: {error}");
        }
        if predicate(&snapshot) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    bail!("Timed out verifying {behavior}")
}

fn wait_interaction(player: &mpv::Mpv, expected: host::HostEvent) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        host::pump_messages();
        while let Some(event) = player.poll_event() {
            if event == expected {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    bail!("Timed out verifying native input routing")
}
