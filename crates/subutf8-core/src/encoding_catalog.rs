use encoding_rs::{
    BIG5_INIT, EUC_JP_INIT, EUC_KR_INIT, Encoding, GBK_INIT, IBM866_INIT, ISO_8859_2_INIT,
    ISO_8859_3_INIT, ISO_8859_4_INIT, ISO_8859_5_INIT, ISO_8859_6_INIT, ISO_8859_7_INIT,
    ISO_8859_8_INIT, ISO_8859_10_INIT, ISO_8859_13_INIT, ISO_8859_14_INIT, ISO_8859_15_INIT,
    ISO_8859_16_INIT, KOI8_R_INIT, KOI8_U_INIT, MACINTOSH_INIT, SHIFT_JIS_INIT, UTF_16BE_INIT,
    UTF_16LE_INIT, WINDOWS_874_INIT, WINDOWS_1250_INIT, WINDOWS_1251_INIT, WINDOWS_1252_INIT,
    WINDOWS_1253_INIT, WINDOWS_1254_INIT, WINDOWS_1255_INIT, WINDOWS_1256_INIT, WINDOWS_1257_INIT,
    WINDOWS_1258_INIT, X_MAC_CYRILLIC_INIT,
};

/// UI-09: the encodings offered for a manual choice, Romanian-relevant ones first.
///
/// Besides UTF-8 and ISO-2022-JP, two standard encodings are left out as duplicates:
/// gb18030, because the standard GBK decoder already reads it, and ISO-8859-8-I,
/// which decodes exactly like ISO-8859-8.
static MANUAL_CHOICES: [&Encoding; 34] = [
    &WINDOWS_1250_INIT,
    &ISO_8859_2_INIT,
    &ISO_8859_16_INIT,
    &WINDOWS_1252_INIT,
    &ISO_8859_15_INIT,
    &WINDOWS_1251_INIT,
    &KOI8_U_INIT,
    &KOI8_R_INIT,
    &ISO_8859_5_INIT,
    &IBM866_INIT,
    &X_MAC_CYRILLIC_INIT,
    &WINDOWS_1253_INIT,
    &ISO_8859_7_INIT,
    &WINDOWS_1254_INIT,
    &WINDOWS_1255_INIT,
    &ISO_8859_8_INIT,
    &WINDOWS_1256_INIT,
    &ISO_8859_6_INIT,
    &WINDOWS_1257_INIT,
    &ISO_8859_13_INIT,
    &ISO_8859_4_INIT,
    &WINDOWS_1258_INIT,
    &WINDOWS_874_INIT,
    &SHIFT_JIS_INIT,
    &EUC_JP_INIT,
    &GBK_INIT,
    &BIG5_INIT,
    &EUC_KR_INIT,
    &ISO_8859_3_INIT,
    &ISO_8859_10_INIT,
    &ISO_8859_14_INIT,
    &MACINTOSH_INIT,
    &UTF_16LE_INIT,
    &UTF_16BE_INIT,
];

pub fn manual_choices() -> &'static [&'static Encoding] {
    &MANUAL_CHOICES
}

/// ENC-12: accepts only the exact name of an offered encoding, so that labels such as
/// `utf8` or `latin1` sent from the interface cannot select anything else.
pub fn find_manual_choice(name: &str) -> Option<&'static Encoding> {
    MANUAL_CHOICES
        .iter()
        .copied()
        .find(|encoding| encoding.name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{
        ISO_2022_JP, ISO_8859_2, ISO_8859_16, REPLACEMENT, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1250,
        X_USER_DEFINED,
    };

    /// UI-09.
    #[test]
    fn romanian_encodings_come_first() {
        assert_eq!(
            &manual_choices()[..3],
            &[WINDOWS_1250, ISO_8859_2, ISO_8859_16]
        );
    }

    /// UI-09.
    #[test]
    fn utf16_is_offered_but_utf8_and_iso_2022_jp_are_not() {
        let choices = manual_choices();
        assert!(choices.contains(&UTF_16LE) && choices.contains(&UTF_16BE));
        for refused in [UTF_8, ISO_2022_JP, REPLACEMENT, X_USER_DEFINED] {
            assert!(!choices.contains(&refused), "{}", refused.name());
        }
    }

    /// UI-09.
    #[test]
    fn each_encoding_is_offered_once() {
        let choices = manual_choices();
        for (position, encoding) in choices.iter().enumerate() {
            assert!(
                !choices[position + 1..].contains(encoding),
                "{}",
                encoding.name()
            );
        }
    }

    /// ENC-12.
    #[test]
    fn only_offered_names_are_accepted() {
        for encoding in manual_choices() {
            assert_eq!(find_manual_choice(encoding.name()), Some(*encoding));
        }
        let refused_names = [
            "UTF-8",
            "utf8",
            "latin1",
            "ISO-2022-JP",
            "replacement",
            "x-user-defined",
            "WINDOWS-1250",
            "",
        ];
        for name in refused_names {
            assert_eq!(find_manual_choice(name), None, "{name}");
        }
    }
}
