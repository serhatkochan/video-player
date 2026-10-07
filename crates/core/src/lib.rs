pub mod media;
pub mod navigation;
pub mod state;

pub use media::{AUDIO_EXTENSIONS, MediaFile, MediaKind, SUBTITLE_EXTENSIONS, VIDEO_EXTENSIONS};
pub use navigation::MediaNeighbors;
pub use state::{AppState, Language};

pub fn format_time(seconds: f64) -> String {
    let seconds = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_display_handles_long_movies_and_invalid_values() {
        assert_eq!(format_time(3661.9), "1:01:01");
        assert_eq!(format_time(f64::NAN), "0:00");
        assert_eq!(format_time(-12.0), "0:00");
    }
}
