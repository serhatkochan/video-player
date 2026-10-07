# Windows release acceptance

Automated checks establish build/registry behavior. They do not establish physical HDR output, device performance, or Explorer behavior on a clean Windows installation. A stable release requires actual hardware/Explorer acceptance and complete corresponding source review. A clearly labeled preview requires complete corresponding sources, passing automated quality, runtime, thumbnail, codec-sample and measured-performance checks, and explicit `not-tested` statuses for remaining hardware scenarios.

Start from `packaging/acceptance.example.json`. Record the exact Git commit, test date, tester, Windows build, GPU/driver, display model/HDR mode, and measured results. Every required stable result must be `pass`; retain logs, screenshots, and fixture hashes with the report. A preview must identify `channel: preview`, list its limitations, and use only `pass` or `not-tested` for the hardware matrix. A report cannot release changed source. A later commit adding only files under `docs/acceptance/` is permitted so that the attestation can be committed after testing.

| Scenario | Required evidence |
| --- | --- |
| Clean Windows 11 x64 | Install with standard user account, no Store codec extensions or extra codec pack; open supported files by dialog, drop, and Explorer double-click. |
| VLC coexistence | Install with VLC present, select Video Player using Settings, verify thumbnails at large/extra-large icon sizes with a fresh thumbnail cache. Repeat after changing the default app. |
| Codec matrix | MP4/M4V, MKV, MOV, WebM, AVI, WMV; H.264, HEVC 8/10-bit, AV1 8/10-bit, VP8/VP9, MPEG-2/MPEG-4 Part 2, WMV/VC-1. Include 4K/8K, rotation, variable frame rate, Türkçe filenames and long paths. Verify the selected video **and audio**. |
| Audio and subtitles | MP3, AAC/M4A, FLAC, WAV; AAC, Opus, Vorbis, AC-3/E-AC-3 and DTS embedded audio. Select multiple tracks; external/embedded SRT, styled ASS/SSA, VTT, subtitle delay; seek/speed and resume. |
| HDR10 physical display | HDR enabled in Windows, 10-bit HEVC/AV1 HDR fixtures, check D3D11 HDR output/color space and display behavior, toggle HDR, move between HDR and SDR monitors, enter/leave fullscreen. Capture mpv diagnostic properties; a screenshot alone cannot prove HDR. |
| SDR tone mapping | Same HDR fixtures on SDR display and Explorer thumbnails; no washed-out colors, verify exposure and color comparisons against a trusted rendering. |
| Corrupt media isolation | Truncated, invalid, unsupported and hanging inputs; Explorer remains responsive, worker deadline triggers, no orphan worker after completion. Large files must not be copied into temporary storage. |
| Install/upgrade/uninstall | Automatic all-users install for administrators and per-user install for standard accounts; UAC consent/denial; uninstaller honors its stored scope; machine uninstall from a standard account requests administrator credentials; safe handoff from an older per-user install; provider loaded in Explorer during upgrade; previous providers restored in their original hive on uninstall; another app's later changes preserved; stale uninstaller harmless. Reboot/sign-in if loaded DLL files remain. |
| Performance | Measure cold/warm launch, idle CPU/RAM, first playback frame, dropped frames, seek latency and thumbnail latency on Intel/AMD devices with hardware decoding and software fallback. Report results; do not extrapolate from a Spotify client. |
| Windows settings/policies | Disable thumbnails with user setting and policy, verify diagnosis and understandable recovery path. Test an administrator with UAC disabled using automatic machine registration, and verify its thumbnails and diagnosis. Also verify diagnosis of an old per-user-only installation or foreign per-user registration that takes precedence. Restore the appropriate setting and verify thumbnails. |

Automated commands:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
./scripts/Test-WindowsIntegration.ps1
./scripts/Test-Runtime.ps1
./scripts/Build-Windows.ps1
```

The integration script tests a unique isolated HKCU test subtree and deletes only that subtree; it does not register the app on the development machine. NSIS compilation produces a development installer without executing it. Test the real installer inside clean VMs before installation acceptance is marked passed. Test each supported Windows release and monitor/GPU combination before expanding the documented guarantee.

After installing the real package, exercise thumbnail extraction through the Windows Shell cache with a colorful synthetic video fixture:

```powershell
cargo run -p video-player-thumbnail --example shell_thumbnail_probe -- "C:\fixtures\Türkçe h264.mp4" "C:\fixtures\shell-h264.bmp" 256
```

Repeat with HEVC and AV1 fixtures. The probe forces extraction in a required Windows surrogate, checks actual colored bitmap pixels, saves the bitmap for inspection, and rejects a provider loaded inside the caller. It does not register or remove providers or change file associations. Windows may request a larger provider thumbnail internally than the client size; this path must be tested in addition to the registration-free factory probe. Corroborate handler selection with the active registration and the exact installed provider DLL path observed in the Windows surrogate. Retain that evidence with the probe output, then verify Explorer's visible result and default-app coexistence separately.
