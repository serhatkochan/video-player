# Windows build and dependency provenance

Requirements: Windows 11 x64, Rust 1.98+, the MSVC x64 build tools/Windows SDK, Windows PowerShell, and `tar.exe`. No separately installed codec pack is required. The build downloads checksum-pinned media archives and corresponding sources using `packaging/runtime-manifest.json`. Application users need neither these development tools nor credentials; the installer contains all runtime files and works offline.

```powershell
./scripts/Get-Runtime.ps1
./scripts/Get-BuildTools.ps1
./scripts/Build-Windows.ps1
```

`Build-Windows` compiles the workspace and thumbnail worker, audits FFmpeg, collects Rust notices, stages payloads, and compiles an NSIS installer under `dist/`. Use `-SkipCargo` only after a successful release build. `-NsisPath` can select an already installed compiler. Portable NSIS is unpacked into ignored `tools/`; no global tool installation is made.

`runtime/` contains `libmpv-2.dll`, `spirv-cross-c-shared.dll`, FFmpeg 8.1 shared DLLs (`avcodec-62`, `avformat-62`, `avutil-60`, `avfilter-11`, `swscale-9`, `swresample-6`), the audited `ffmpeg.exe`, matching headers under `include/`, upstream notices under `licenses/`, and the pin manifest. The installer stages only the pinned archive's runtime-file whitelist, beside `video-player.exe`, `video_player_thumbnail.dll`, and `thumbnail-worker.exe`. Rust and media builds use the static MSVC runtime so a separate Visual C++ redistributable is unnecessary.

| Runtime | Pinned upstream artifact | Verification |
| --- | --- | --- |
| libmpv | Project build from pinned mpv `e470f8986e7d544c7ccf526e987b9f869f0b5904`, based on [the retained LGPL recipe](https://github.com/peschuster/libmpv-build/tree/9541f361ec25075e267f38f41ed460d204cd7f13) | Archive SHA-256, exact source revisions, D3D11/Shaderc/SPIRV-Cross, GPL disabled, no extra CRT DLLs |
| FFmpeg | Project shared build from [FFmpeg 8.1 Meson port](https://gitlab.freedesktop.org/gstreamer/meson-ports/ffmpeg/-/tree/ff04763ef7cd858605cb118c5626070498f2c49c) | Archive SHA-256, matching shared ABI and headers, LGPL 2.1, zscale/tonemap, dav1d, no GPL/nonfree flags |
| NSIS | [Official NSIS 3.11 checksum](https://sourceforge.net/projects/nsis/files/NSIS%203/3.11/nsis-3.11.zip/download) via [Tauri mirror](https://github.com/tauri-apps/binary-releases/releases/tag/nsis-3.11) | Byte-identical official archive SHA-256 |

The LGPL variant omits GPL software encoders such as x264/x265; H.264 and HEVC **decoding** remains supported. The separate FFmpeg DLLs are LGPL-2.1-or-later. The combined libmpv DLL is distributed under LGPL-3.0-or-later, using the later-version permission for compatibility with its Apache-2.0 shaderc/SPIRV-Tools components. GPL-enabled system FFmpeg builds must never be copied into the distribution.

## Corresponding sources and publishing

The media build scripts under `scripts/media-runtime/` retain the exact source trees, recursive submodules, downloaded WrapDB source/patch archives, applied patches, component license notices, build flags and reproduction recipes. No rolling binary dependency is used. `sources.json` and vendored wrap hashes record inputs. General compiler tools and unmodified Windows system libraries are not statically incorporated third-party media source dependencies.

The release retains binary and complete source archives on GitHub Releases. Their immutable URLs and hashes are in the runtime manifest; `Get-Runtime` verifies them and extracts corresponding sources into `runtime/sources/`. Source retention and license review remain mandatory for every runtime update. Legacy `Get-SourceSeeds.ps1` snapshots alone cannot authorize binary redistribution.

The source bundle must contain `corresponding-sources.json` with schema version 1, `complete: true`, `reviewedBy`, and one `runtimes` entry per media runtime. Each entry includes `id`, the pinned `binaryArchiveSha256`, `includesAllStaticDependencies: true`, `includesBuildScripts: true`, and `archives` with relative `file`/`sha256` pairs. These assertions require an actual source audit; setting them without complete inputs does not satisfy the license.

Run `Build-Windows.ps1 -ForRelease -AcceptanceFile <completed-report.json>` for a fully accepted stable build, or add `-Channel preview` for a clearly declared preview with passing automated checks and explicitly untested hardware scenarios. Both require complete corresponding sources. See [acceptance.md](acceptance.md). The optional GitHub workflow creates a reviewable draft after these gates pass; the same commands work locally.

To rebuild the media libraries rather than download them, prepare PowerShell 7, Visual Studio C++/Clang, CMake, Meson 1.12.1, Ninja 1.13.0, NASM and pkg-config, then run `Build-Libmpv.ps1` and `Pack-SharedFfmpeg.ps1`. Extracted source archives also retain these recipes and source revision markers. The media workflow runs these commands on Windows; it is not part of application installation or playback.

## Registry and DLL lifetime

Installation scope is automatic: an administrator installs under `%PROGRAMFILES%\VideoPlayer\versions\<version>-<payload-hash>` with HKLM registration; a standard account installs under `%LOCALAPPDATA%\Programs\VideoPlayer\versions\<version>-<payload-hash>` with HKCU registration. Windows requests normal administrator consent through `highestAvailable` when UAC is enabled. Each scope has separate state and an installation marker that the uninstaller honors. Active versions are never overwritten, which avoids replacing DLLs still loaded by Explorer. The payload includes the installer source so an uninstaller change also changes the payload directory. Shared thumbnail defaults are registered for the extension and `SystemFileAssociations`; each Video Player ProgID also exposes the provider. COM uses `IInitializeWithStream` and an empty AppID `DllSurrogate`; no process-isolation override is set.

`integration-state.json` stores exact prior registry value kinds/data, including a missing value versus an empty default, and binds them to their scope and hive. An atomically persisted rollback journal recovers interrupted first installs and upgrades; scope-specific cross-session mutexes serialize registry updates. Upgrades retain the initial backup and preserve values another app changed. Uninstall restores only values that still match our last write. An old version's uninstaller cannot remove a newer version's integration. A machine install can retire the invoking account's old per-user Video Player registration with a transactional ownership-checked handoff. Other users' profiles are not edited. No `UserChoice` hash or default association is written; the user chooses the default in Windows Settings.

Administrator accounts with UAC disabled have an additional Windows restriction: COM ignores their per-user registration. The current automatic all-users installation uses machine registration and supports this case without changing UAC policy. Diagnosis looks for the effective COM server and extension handlers across HKCU and HKLM, and explains older per-user or conflicting registrations. [Microsoft documents the per-user COM restriction](https://learn.microsoft.com/en-us/windows/win32/sysinfo/merged-view-of-hkey-classes-root).

Uninstall may leave inactive DLL files until Explorer releases them. Old immutable version directories are retained across upgrades; running their own uninstaller removes them without affecting the active version. Settings/history remain under the application data directory.
