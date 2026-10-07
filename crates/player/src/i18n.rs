use video_player_core::Language;

pub fn text(language: Language, english: &'static str, turkish: &'static str) -> &'static str {
    match language {
        Language::English => english,
        Language::Turkish => turkish,
    }
}

pub fn system_language() -> Language {
    #[cfg(windows)]
    {
        let language = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        if language & 0x3ff == 0x1f {
            return Language::Turkish;
        }
    }
    Language::English
}
