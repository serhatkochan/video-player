use crate::i18n::text;
use serde::Serialize;
use video_player_core::Language;

pub const THUMBNAIL_CLSID: &str = "{8C1D9ED4-6900-4D31-9EBB-570623A87973}";

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub ok: bool,
    pub message: String,
}

pub fn check(language: Language) -> Vec<Diagnostic> {
    let mut results = Vec::new();
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        let uac_disabled_admin = read_dword(
            HKEY_LOCAL_MACHINE,
            "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\System",
            "EnableLUA",
        ) == Some(0)
            && unsafe { windows::Win32::UI::Shell::IsUserAnAdmin() }.as_bool();
        let server_key = format!("Software\\Classes\\CLSID\\{THUMBNAIL_CLSID}\\InprocServer32");
        let server = if uac_disabled_admin {
            read_string(HKEY_LOCAL_MACHINE, &server_key, "")
        } else {
            read_string(HKEY_CURRENT_USER, &server_key, "")
                .or_else(|| read_string(HKEY_LOCAL_MACHINE, &server_key, ""))
        };
        let server_available = server
            .as_deref()
            .is_some_and(|path| !path.is_empty() && std::path::Path::new(path).is_file());
        results.push(Diagnostic { ok: server_available, message: if uac_disabled_admin && !server_available {
            text(language,
                "System thumbnail integration is required for this administrator account with UAC disabled. Run the current setup: it automatically installs for all users when administrator permissions are available.",
                "UAC kapalı bu yönetici hesabı için sistem geneli küçük resim kaydı gerekiyor. Güncel kurulum paketini çalıştırın; yönetici yetkisi varsa tüm kullanıcılar için otomatik kurulur.")
        } else if server_available {
            text(language, "Video Player COM thumbnail component is registered and available for this account.",
                "Video Player COM küçük resim bileşeni bu hesap için kayıtlı ve kullanılabilir.")
        } else {
            text(language, "Video Player COM thumbnail component is missing. Install or repair with the current setup package.",
                "Video Player COM küçük resim bileşeni bulunamadı. Güncel kurulum paketiyle kurun veya onarın.")
        }.into() });
        let advanced = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced";
        let policy = "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer";
        let disabled_by_user = read_dword(HKEY_CURRENT_USER, advanced, "IconsOnly") == Some(1);
        results.push(Diagnostic { ok: !disabled_by_user, message: text(language,
            "Explorer: enable thumbnails in File Explorer Options > View (turn off Always show icons, never thumbnails).",
            "Explorer: Dosya Gezgini Seçenekleri > Görünüm içinde Her zaman simge göster, küçük resim gösterme seçeneğini kapatın.").into() });
        let disabled_by_policy = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .any(|root| read_dword(root, policy, "DisableThumbnails") == Some(1));
        results.push(Diagnostic { ok: !disabled_by_policy, message: text(language,
            "Windows policy: thumbnail generation must be allowed. Contact your administrator if it is disabled.",
            "Windows ilkesi: küçük resim oluşturulmasına izin verilmeli. Engelliyse sistem yöneticinizle görüşün.").into() });
        let missing = video_player_core::VIDEO_EXTENSIONS.iter().filter(|extension| {
            let key = format!("Software\\Classes\\SystemFileAssociations\\.{extension}\\ShellEx\\{{E357FCCD-A995-4576-B01F-234630154E96}}");
            !read_string(HKEY_CURRENT_USER, &key, "")
                .or_else(|| read_string(HKEY_LOCAL_MACHINE, &key, ""))
                .is_some_and(|value| value.eq_ignore_ascii_case(THUMBNAIL_CLSID))
        }).copied().collect::<Vec<_>>();
        let mut message = text(language,
            "Video Player thumbnail associations: install or repair using the full setup package, then use medium or larger icons in Explorer. A different per-user thumbnail registration can take precedence over system registration.",
            "Video Player küçük resim ilişkileri: tam kurulum paketiyle kurun veya onarın; Explorer'da orta ya da daha büyük simgeleri seçin. Başka bir kullanıcı kaydı sistem genelindeki kaydın önüne geçebilir.").to_owned();
        if !missing.is_empty() {
            message.push_str(&format!(" ({})", missing.join(", ")));
        }
        results.push(Diagnostic {
            ok: missing.is_empty(),
            message,
        });
    }
    results
}

#[cfg(windows)]
fn read_dword(root: windows::Win32::System::Registry::HKEY, path: &str, name: &str) -> Option<u32> {
    use windows::Win32::System::Registry::{RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::PCWSTR;
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut value = 0u32;
    let mut size = 4u32;
    let result = unsafe {
        RegGetValueW(
            root,
            PCWSTR(path.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    (result.0 == 0).then_some(value)
}

#[cfg(windows)]
fn read_string(
    root: windows::Win32::System::Registry::HKEY,
    path: &str,
    name: &str,
) -> Option<String> {
    use windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::PCWSTR;
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut buffer = [0u16; 256];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            root,
            PCWSTR(path.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    (result.0 == 0).then(|| {
        String::from_utf16_lossy(
            &buffer[..buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len())],
        )
    })
}
