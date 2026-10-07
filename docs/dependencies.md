# Windows build and dependency provenance

Requirements: Windows 11 x64, Rust 1.98+, the MSVC x64 build tools/Windows SDK, Windows PowerShell, and `tar.exe`. License collection reads exact upstream Git revisions from public GitHub APIs; optionally set `GH_TOKEN` to avoid shared anonymous rate limits. No separately installed codec pack is required. The build uses immutable dependency pins in `packaging/runtime-manifest.json`.

```powershell
./scripts/Get-Runtime.ps1
./scripts/Get-BuildTools.ps1
./scripts/Build-Windows.ps1
```

`Build-Windows` compiles the workspace and thumbnail worker, audits FFmpeg, collects Rust notices, stages payloads, and compiles an NSIS installer under `dist/`. Use `-SkipCargo` only after a successful release build. `-NsisPath` can select an already installed compiler. Portable NSIS is unpacked into ignored `tools/`; no global tool installation is made.

`runtime/` contains `libmpv-2.dll`, FFmpeg 8.1 shared DLLs (`avcodec-62`, `avformat-62`, `avutil-60`, `avfilter-11`, `swscale-9`, `swresample-6`, plus upstream `avdevice-62`), the audited `ffmpeg.exe`, matching headers under `include/`, upstream notices under `licenses/`, and the pin manifest. The installer places the runtime DLLs beside `video-player.exe`, `video_player_thumbnail.dll`, and `thumbnail-worker.exe`.

| Runtime | Pinned upstream artifact | Verification |
| --- | --- | --- |
| libmpv | [zhongfly LGPL x64 build, 2026-10-07](https://github.com/zhongfly/mpv-winbuild/releases/tag/2026-10-07-eb0ee10315) | Archive SHA-256, exact mpv commit, `-Dgpl=false` runtime configuration |
| FFmpeg | [BtbN monthly LGPL shared 8.1 build, 2026-09-30](https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-09-30-13-08) | Archive SHA-256, shared ABI, no GPL/nonfree flags, zscale/tonemap filters |
| NSIS | [Official NSIS 3.11 checksum](https://sourceforge.net/projects/nsis/files/NSIS%203/3.11/nsis-3.11.zip/download) via [Tauri mirror](https://github.com/tauri-apps/binary-releases/releases/tag/nsis-3.11) | Byte-identical official archive SHA-256 |

The LGPL variant omits GPL software encoders such as x264/x265; H.264 and HEVC **decoding** remains supported. FFmpeg's `--enable-version3` selects LGPLv3. libmpv's static dependency contents must be audited with its corresponding source bundle. GPL-enabled system FFmpeg builds must never be copied into the distribution.

## Corresponding sources and publishing

The current pins are verified for development. Exact mpv, both FFmpeg revisions, and all three builder/toolchain source snapshots are supplied by `Get-SourceSeeds.ps1`, with hashes in `packaging/source-seeds-manifest.json`. **Public binary redistribution is blocked:** every statically incorporated external dependency and exact build input must still be supplied and audited. libmpv's rolling build dependencies need all exact revisions and applicable patches. FFmpeg also needs every statically incorporated external dependency, not just the FFmpeg Git checkout. Its [builder recipes](https://github.com/BtbN/FFmpeg-Builds) and mpv's [LGPL patch](https://github.com/zhongfly/mpv-winbuild/blob/f5eae188817041f0b2acb1ecd6fb5f095a0f4def/compile-lgpl-libmpv.patch) help reproduce the builds.

Preserve each verified binary archive before upstream retention expires. Gather exact corresponding sources, copyright notices, patches, and build scripts from the recorded build run, or rebuild the runtimes from fully pinned sources with a retained download cache. Review the complete collection and publish a source bundle on GitHub. Add its immutable URL/hash to `sourceBundles` in the pin manifest; `Get-Runtime` will stage it into `runtime/sources/` for local and CI builds.

The source bundle must contain `corresponding-sources.json` with schema version 1, `complete: true`, `reviewedBy`, and one `runtimes` entry per media runtime. Each entry includes `id`, the pinned `binaryArchiveSha256`, `includesAllStaticDependencies: true`, `includesBuildScripts: true`, and `archives` with relative `file`/`sha256` pairs. These assertions require an actual source audit; setting them without complete inputs does not satisfy the license.

Run `Build-Windows.ps1 -ForRelease -AcceptanceFile <completed-report.json>` only after that source review and the hardware tests in [acceptance.md](acceptance.md). It refuses release builds if either gate is incomplete. The GitHub workflow creates a reviewable draft only after these gates pass. No accepted public release is claimed by the development installer.

## Registry and DLL lifetime

Installation scope is automatic: an administrator installs under `%PROGRAMFILES%\VideoPlayer\versions\<version>-<payload-hash>` with HKLM registration; a standard account installs under `%LOCALAPPDATA%\Programs\VideoPlayer\versions\<version>-<payload-hash>` with HKCU registration. Windows requests normal administrator consent through `highestAvailable` when UAC is enabled. Each scope has separate state and an installation marker that the uninstaller honors. Active versions are never overwritten, which avoids replacing DLLs still loaded by Explorer. The payload includes the installer source so an uninstaller change also changes the payload directory. Shared thumbnail defaults are registered for the extension and `SystemFileAssociations`; each Video Player ProgID also exposes the provider. COM uses `IInitializeWithStream` and an empty AppID `DllSurrogate`; no process-isolation override is set.

`integration-state.json` stores exact prior registry value kinds/data, including a missing value versus an empty default, and binds them to their scope and hive. An atomically persisted rollback journal recovers interrupted first installs and upgrades; scope-specific cross-session mutexes serialize registry updates. Upgrades retain the initial backup and preserve values another app changed. Uninstall restores only values that still match our last write. An old version's uninstaller cannot remove a newer version's integration. A machine install can retire the invoking account's old per-user Video Player registration with a transactional ownership-checked handoff. Other users' profiles are not edited. No `UserChoice` hash or default association is written; the user chooses the default in Windows Settings.

Administrator accounts with UAC disabled have an additional Windows restriction: COM ignores their per-user registration. The current automatic all-users installation uses machine registration and supports this case without changing UAC policy. Diagnosis looks for the effective COM server and extension handlers across HKCU and HKLM, and explains older per-user or conflicting registrations. [Microsoft documents the per-user COM restriction](https://learn.microsoft.com/en-us/windows/win32/sysinfo/merged-view-of-hkey-classes-root).

Uninstall may leave inactive DLL files until Explorer releases them. Old immutable version directories are retained across upgrades; running their own uninstaller removes them without affecting the active version. Settings/history remain under the application data directory.
