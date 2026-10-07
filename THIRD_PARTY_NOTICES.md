# Third-party software

Video Player's original Rust application and Explorer integration code are MIT licensed. Media libraries retain their own licenses; the MIT license does not relicense those libraries.

## Media runtime

- **mpv / libmpv:** copyright mpv contributors. GPL components are disabled. The combined Windows DLL is distributed under LGPL-3.0-or-later, using mpv and FFmpeg's later-version permission for compatibility with the Apache-2.0 shaderc and SPIRV-Tools components. See `licenses/mpv/Copyright`, `licenses/libmpv/COMBINED-LICENSE.txt`, and the preserved component license texts.
- **FFmpeg:** copyright FFmpeg developers. The separate FFmpeg 8.1 shared runtime is LGPL-2.1-or-later. GPL, version3 and nonfree build options are disabled; `av*.dll` / `sw*.dll` remain replaceable. The archive also preserves the BSD dav1d, WTFPL zimg and zlib notices under `licenses/media/`. See the exact license texts and `ffmpeg-build-config.txt`.
- FFmpeg/libmpv's statically included dependencies retain their individual notices and source obligations. A complete corresponding source bundle, including exact dependency sources and build scripts, is mandatory for public binary redistribution.

Runtime DLLs are replaceable; Video Player does not restrict modification, debugging, or reverse engineering of the LGPL components. Modified compatible DLLs can replace the adjacent originals in an installed version directory. Back up original files and close the application/Explorer before replacing loaded DLLs.

The pinned binary URLs, SHA-256 checksums, source revisions, and builder provenance are in `runtime-manifest.json`. The release also supplies `sources/` and `licenses/`. Upstream source links are informational and are not a substitute for complete corresponding sources.

## Rust dependencies

The installer includes copied upstream copyright notices and license texts under `licenses/rust/<crate>-<version>/`. `licenses/rust/dependency-index.json` records versions, declared SPDX licenses, and upstream repositories from the locked Cargo dependency graph. Generate this directory with `scripts/Get-RustNotices.ps1` after changing dependencies.

## Installer tools

NSIS is used to generate the installation executable. Its core and plug-ins use the zlib/libpng license; its LZMA compression module uses the Common Public License 1.0 with an explicit exception permitting linked application code to retain its own license. The complete upstream copyright notice, license texts, and linking exception are included in `licenses/nsis/COPYING`. The unmodified NSIS 3.11 sources are available from the [official NSIS 3.11 distribution](https://sourceforge.net/projects/nsis/files/NSIS%203/3.11/). NSIS is not installed globally by the build scripts. The portable build compiler is checksum pinned to the official NSIS 3.11 distribution, mirrored by the Tauri project.
