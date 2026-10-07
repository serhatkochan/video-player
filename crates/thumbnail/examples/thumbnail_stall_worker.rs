//! Deliberately unresponsive process used only by thumbnail_probe --timeout-worker.
fn main() {
    std::thread::sleep(std::time::Duration::from_secs(60));
}
