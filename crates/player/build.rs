use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/video-player.ico");
    for name in [
        "RC",
        "WindowsSdkVerBinPath",
        "WindowsSdkDir",
        "ProgramFiles(x86)",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").as_deref(),
        Ok("msvc"),
        "The Windows player requires the MSVC toolchain and Windows SDK"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
        .join("assets/video-player.ico");
    let icon_path = icon.to_string_lossy().replace('\\', "\\\\");
    let version = env::var("CARGO_PKG_VERSION").expect("Cargo package version");
    let numeric_version = version.replace('.', ",");
    let source = format!(
        r#"1 ICON "{icon_path}"
1 VERSIONINFO
FILEVERSION {numeric_version},0
PRODUCTVERSION {numeric_version},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Video Player contributors\0"
            VALUE "FileDescription", "Video Player\0"
            VALUE "FileVersion", "{version}\0"
            VALUE "InternalName", "video-player\0"
            VALUE "LegalCopyright", "MIT licensed Video Player contributors\0"
            VALUE "OriginalFilename", "video-player.exe\0"
            VALUE "ProductName", "Video Player\0"
            VALUE "ProductVersion", "{version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
    );
    let rc = out.join("video-player.rc");
    let resource = out.join("video-player.res");
    fs::write(&rc, source).expect("Write Windows icon resource");
    let compiler = resource_compiler().expect("Windows SDK rc.exe not found; install the Windows SDK with the MSVC C++ build tools or set RC");
    let status = Command::new(compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&rc)
        .status()
        .expect("Run Windows SDK resource compiler");
    assert!(status.success(), "Windows icon resource compilation failed");
    println!(
        "cargo:rustc-link-arg-bin=video-player={}",
        resource.display()
    );
}

fn resource_compiler() -> Option<PathBuf> {
    if let Some(path) = env::var_os("RC") {
        return Some(PathBuf::from(path));
    }
    if Command::new("rc.exe").arg("/?").output().is_ok() {
        return Some(PathBuf::from("rc.exe"));
    }
    if let Some(bin) = env::var_os("WindowsSdkVerBinPath") {
        let compiler = PathBuf::from(bin).join("x64/rc.exe");
        if compiler.is_file() {
            return Some(compiler);
        }
    }
    let sdk = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ProgramFiles(x86)")
                .map(|program_files| PathBuf::from(program_files).join("Windows Kits/10"))
        })?;
    let mut versions = fs::read_dir(sdk.join("bin"))
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let version = name
                .to_str()?
                .split('.')
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            let compiler = entry.path().join("x64/rc.exe");
            compiler.is_file().then_some((version, compiler))
        })
        .collect::<Vec<_>>();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().next().map(|(_, compiler)| compiler)
}
