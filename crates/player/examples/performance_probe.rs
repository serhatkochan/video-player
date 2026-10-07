//! Developer benchmark of production native-host/libmpv snapshot latency.
//! Usage: performance_probe <media> <runtime-directory> <output.json> [trials] [seek-seconds]

#[path = "../src/host.rs"]
#[allow(dead_code)]
mod host;
#[path = "../src/mpv.rs"]
#[allow(dead_code)]
mod mpv;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(3);
const WAIT_LIMIT: Duration = Duration::from_secs(20);

#[derive(Serialize)]
struct Trial {
    native_host_and_mpv_initialization_ms: f64,
    first_playback_progress_ms: f64,
    paused_exact_seek_ms: f64,
    seek_baseline_seconds: f64,
    seek_target_seconds: f64,
    seek_observed_seconds: f64,
    duration_seconds: f64,
    video_codec: String,
    hwdec: String,
    output_colorspace: String,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(3..=5).contains(&args.len()) {
        bail!(
            "Usage: performance_probe <media> <runtime-directory> <output.json> [trials] [seek-seconds]"
        );
    }
    let media = video_player_core::MediaFile::open(PathBuf::from(&args[0]))?;
    if media.kind != video_player_core::MediaKind::Video {
        bail!("A local video fixture is required");
    }
    let runtime = PathBuf::from(&args[1]).canonicalize()?;
    let output = PathBuf::from(&args[2]);
    let count = args
        .get(3)
        .map(|arg| {
            arg.to_str()
                .context("Trial count must be Unicode")?
                .parse::<usize>()
                .context("Invalid trial count")
        })
        .transpose()?
        .unwrap_or(7);
    if !(1..=100).contains(&count) {
        bail!("Trial count must be between 1 and 100");
    }
    let requested_seek = args
        .get(4)
        .map(|arg| {
            arg.to_str()
                .context("Seek position must be Unicode")?
                .parse::<f64>()
                .context("Invalid seek position")
        })
        .transpose()?;
    if requested_seek.is_some_and(|position| !position.is_finite() || position <= 0.0) {
        bail!("Seek position must be a finite positive number");
    }
    eprintln!("Discarded warmup");
    let warmup = trial(&media.path, &runtime, requested_seek)?;
    let mut trials = Vec::with_capacity(count);
    for index in 1..=count {
        let result = trial(&media.path, &runtime, requested_seek)
            .with_context(|| format!("Trial {index}"))?;
        eprintln!(
            "Trial {index}: init={:.3} ms progress={:.3} ms seek={:.3} ms",
            result.native_host_and_mpv_initialization_ms,
            result.first_playback_progress_ms,
            result.paused_exact_seek_ms
        );
        trials.push(result);
    }
    let summary = serde_json::json!({
        "native_host_and_mpv_initialization_ms": statistics(trials.iter().map(|t| t.native_host_and_mpv_initialization_ms)),
        "first_playback_progress_ms": statistics(trials.iter().map(|t| t.first_playback_progress_ms)),
        "paused_exact_seek_ms": statistics(trials.iter().map(|t| t.paused_exact_seek_ms)),
    });
    let report = serde_json::json!({
        "schema_version": 1,
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "media_filename": media.display_name(),
        "media_bytes": media.size,
        "native_surface": "hidden 960x540 HWND; production VideoHost and Mpv",
        "poll_interval_ms": POLL_INTERVAL.as_millis(),
        "wait_deadline_ms": WAIT_LIMIT.as_millis(),
        "discarded_warmup": warmup,
        "measured_trials": trials,
        "summary": summary,
        "limitations": [
            "Initialization covers VideoHost and Mpv, not egui or complete application startup.",
            "First playback progress is a matching nonidle snapshot with duration > 0 and position > 0, not the first rendered frame.",
            "Seek latency ends at paused time-pos within 0.05 seconds of the requested different position, not rendered-frame confirmation.",
            "Each trial uses a new host and player; OS, GPU-driver and filesystem caches are not flushed.",
            "Polling, async property publication and Windows thread scheduling affect the measured latency.",
            "Snapshot waits have fixed deadlines; the production blocking Mpv constructor and teardown have no separate timeout."
        ]
    });
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("Write report: {}", output.display()))?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn trial(media: &Path, runtime: &Path, requested_seek: Option<f64>) -> Result<Trial> {
    let path = media.to_str().context("Media path must be Unicode")?;
    let initialization_started = Instant::now();
    let video = host::VideoHost::new(0)?;
    let player = mpv::Mpv::new(video.hwnd(), runtime)?;
    let initialization_ms = milliseconds(initialization_started);

    let playback_started = Instant::now();
    player.command(&["loadfile", path, "replace"])?;
    player.set_property("pause", "no")?;
    let progressing = wait_for(
        &video,
        &player,
        media,
        "first playback progress",
        |snapshot| {
            !snapshot.idle && !snapshot.paused && snapshot.duration > 0.0 && snapshot.position > 0.0
        },
    )?;
    let first_progress_ms = milliseconds(playback_started);
    if progressing.duration < 4.0 {
        bail!("Use a fixture lasting at least four seconds");
    }
    player.set_property("pause", "yes")?;
    let baseline = wait_for(&video, &player, media, "paused baseline", |snapshot| {
        snapshot.paused && !snapshot.idle && snapshot.duration > 0.0
    })?;
    let target = requested_seek.unwrap_or(baseline.duration / 2.0);
    if target >= baseline.duration - 0.5 || (target - baseline.position).abs() < 1.0 {
        bail!(
            "Seek target must be before the end and at least one second from the paused baseline"
        );
    }
    let seek_started = Instant::now();
    player.command(&["seek", &target.to_string(), "absolute+exact"])?;
    let sought = wait_for(&video, &player, media, "paused exact seek", |snapshot| {
        snapshot.paused && !snapshot.idle && (snapshot.position - target).abs() <= 0.05
    })?;
    let seek_ms = milliseconds(seek_started);
    let metadata = wait_for(&video, &player, media, "decoder metadata", |snapshot| {
        !snapshot.video_codec.is_empty() && !snapshot.output_colorspace.is_empty()
    })?;
    Ok(Trial {
        native_host_and_mpv_initialization_ms: initialization_ms,
        first_playback_progress_ms: first_progress_ms,
        paused_exact_seek_ms: seek_ms,
        seek_baseline_seconds: baseline.position,
        seek_target_seconds: target,
        seek_observed_seconds: sought.position,
        duration_seconds: baseline.duration,
        video_codec: metadata.video_codec,
        hwdec: metadata.hwdec,
        output_colorspace: metadata.output_colorspace,
    })
}

fn wait_for(
    video: &host::VideoHost,
    player: &mpv::Mpv,
    media: &Path,
    behavior: &str,
    predicate: impl Fn(&mpv::PlaybackSnapshot) -> bool,
) -> Result<mpv::PlaybackSnapshot> {
    let deadline = Instant::now() + WAIT_LIMIT;
    let normalized = |path: &str| {
        path.trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase()
    };
    let expected = normalized(&media.to_string_lossy());
    while Instant::now() < deadline {
        host::pump_messages();
        if video.closed() {
            bail!("Native video surface closed while waiting for {behavior}");
        }
        let snapshot = player.snapshot();
        if let Some(error) = &snapshot.error {
            bail!("{behavior}: {error}");
        }
        let matching = snapshot
            .path
            .as_deref()
            .is_some_and(|path| normalized(path) == expected);
        if matching && predicate(&snapshot) {
            return Ok(snapshot);
        }
        thread::sleep(POLL_INTERVAL);
    }
    bail!("Timed out waiting for {behavior}")
}

fn milliseconds(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn statistics(values: impl Iterator<Item = f64>) -> serde_json::Value {
    let mut values: Vec<_> = values.collect();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    };
    serde_json::json!({ "median": median, "min": values[0], "max": values[values.len() - 1] })
}
