fn main() {
    #[cfg(windows)]
    if let Err(error) = video_player_thumbnail::worker_main() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
