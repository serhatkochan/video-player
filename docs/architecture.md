# Architecture

The Cargo workspace contains three crates:

| Crate | Responsibility |
| --- | --- |
| `video-player-core` | Media extension validation, file fingerprinting, bounded local preferences and resume history. |
| `video-player` | egui/eframe UI, native child HWND, worker-owned dynamically loaded libmpv, settings diagnosis and explicit GitHub update checks. |
| `video-player-thumbnail` | COM class factory/provider, bounded stream broker, and FFmpeg decoder worker. |

The egui window owns a native video child. libmpv uses `vo=gpu-next`, D3D11, automatic hardware decoding, and target color-space negotiation. Video is rendered directly into that child, preserving the native presentation path for HDR. Controls occupy a separate area. Settings hide the child because native HWND content composes above egui content. Both native host and mpv input events are drained by the UI. Shutdown destroys mpv before the embedding window.

The playback worker owns the libmpv handle. It dispatches commands in order and publishes snapshots to the UI. A per-file `start` option restores playback without applying a stale seek to a previous file. Resume storage also checks the active path before recording a position.

The thumbnail DLL implements `IInitializeWithStream` and `IThumbnailProvider`; it never needs the media's filesystem path. Explorer can keep its standard process isolation. A separate `thumbnail-worker.exe` receives bounded read/seek requests through anonymous pipes. The broker accesses `IStream` on its original COM thread, enforces the wall-clock deadline and I/O budget, and kills the Windows Job on failure. The worker decodes a frame, honors aspect ratio/rotation, tone maps HDR to SDR, scales to the requested size, and returns opaque BGRA pixels. Pipe threads finish before the DLL can unload.

FFmpeg's public struct prefixes are tied to the packaged 8.1 ABI. The code checks DLL major versions, and a C probe using the matching headers verifies the offsets used by Rust. Replacing the runtime requires an ABI review, updated artifact hashes and decoder tests.

Installation automatically chooses all-users scope for administrators (`Program Files`, HKLM) and current-user scope for standard accounts (`LocalAppData`, HKCU). Both use immutable version/payload directories. The integration script stores exact prior registry value kinds/data, including absence, in separate scope-bound backups and recovery journals. Upgrades keep that initial backup, and uninstall restores only values still owned by this installation. The uninstaller reads its saved scope instead of changing behavior with the caller's token. A machine installation can transactionally retire the current account's older per-user Video Player registration when ownership is proven; other accounts and other applications' changes are preserved. The script never writes a `UserChoice` hash. Default-app selection opens Windows Settings.

Public release CI requires an acceptance report matching the Git commit and a reviewed complete corresponding-source bundle for native runtime dependencies. A successful build or synthetic decode probe does not substitute for physical HDR and clean Windows Explorer acceptance.
