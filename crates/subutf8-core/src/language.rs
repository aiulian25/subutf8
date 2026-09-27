use crate::constants::{LANGUAGE_SUBTAG_LETTERS, REGION_SUBTAG_DIGITS, REGION_SUBTAG_LETTERS};

/// ENC-13: languages whose legacy subtitles come from one encoding family, each paired with
/// a country domain that the detector ties to that family. Language codes are never passed
/// to the detector directly: many belong to unrelated countries (`uk` is the United Kingdom,
/// `ar` Argentina), and the detector treats unknown two-letter domains as Western.
/// Languages missing here, including the Western ones, get no hint.
static DETECTION_HINTS: &[(&str, &str)] = &[
    ("ro", "ro"),
    ("cs", "cz"),
    ("sk", "sk"),
    ("hr", "hr"),
    ("hu", "hu"),
    ("pl", "pl"),
    ("sl", "si"),
    // Bosnian and Serbian subtitles come in Latin and Cyrillic; Bosnia's domain covers both.
    ("bs", "ba"),
    ("sr", "ba"),
    ("ru", "ru"),
    ("uk", "ua"),
    ("be", "by"),
    ("bg", "bg"),
    ("mk", "mk"),
    ("kk", "kz"),
    ("ky", "kg"),
    ("tg", "tj"),
    ("mn", "mn"),
    ("el", "gr"),
    ("tr", "tr"),
    ("az", "az"),
    ("he", "il"),
    ("yi", "il"),
    ("ar", "eg"),
    ("fa", "ir"),
    ("ur", "pk"),
    ("ps", "af"),
    ("lt", "lt"),
    ("lv", "lv"),
    ("vi", "vn"),
    ("th", "th"),
    ("zh", "cn"),
    ("zh-CN", "cn"),
    ("zh-SG", "sg"),
    ("zh-TW", "tw"),
    ("zh-HK", "hk"),
    ("zh-MO", "mo"),
    ("ja", "jp"),
    ("ko", "kr"),
    ("is", "is"),
    ("fo", "fo"),
];

/// A validated subtitle language tag such as `ro` or `pt-BR` (NAME-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleLanguage {
    tag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLanguageTag;

impl SubtitleLanguage {
    /// NAME-05: accepts 2 or 3 letters, optionally followed by a hyphen and a region of
    /// 2 letters or 3 digits, and stores a lower-case language with an upper-case region.
    pub fn parse(text: &str) -> Result<Self, InvalidLanguageTag> {
        let (language, region) = match text.split_once('-') {
            Some((language, region)) => (language, Some(region)),
            None => (text, None),
        };
        if !is_language_subtag(language) || !region.is_none_or(is_region_subtag) {
            return Err(InvalidLanguageTag);
        }
        let mut tag = language.to_ascii_lowercase();
        if let Some(region) = region {
            tag.push('-');
            tag.push_str(&region.to_ascii_uppercase());
        }
        Ok(Self { tag })
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The country domain that steers detection towards this language's legacy encodings.
    pub fn detection_hint(&self) -> Option<&'static str> {
        let language = self.tag.split('-').next().unwrap_or(&self.tag);
        find_hint(&self.tag).or_else(|| find_hint(language))
    }
}

fn find_hint(tag: &str) -> Option<&'static str> {
    DETECTION_HINTS
        .iter()
        .find(|(language, _)| *language == tag)
        .map(|(_, domain)| *domain)
}

fn is_language_subtag(text: &str) -> bool {
    LANGUAGE_SUBTAG_LETTERS.contains(&text.len())
        && text.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn is_region_subtag(text: &str) -> bool {
    let is_letters =
        text.len() == REGION_SUBTAG_LETTERS && text.bytes().all(|byte| byte.is_ascii_alphabetic());
    let is_digits =
        text.len() == REGION_SUBTAG_DIGITS && text.bytes().all(|byte| byte.is_ascii_digit());
    is_letters || is_digits
}

#[cfg(test)]
mod tests {
    use super::*;
    use chardetng::EncodingDetector;

    fn hint(tag: &str) -> Option<&'static str> {
        SubtitleLanguage::parse(tag).unwrap().detection_hint()
    }

    /// NAME-05.
    #[test]
    fn language_tag_is_validated() {
        let accepted = [
            ("ro", "ro"),
            ("RO", "ro"),
            ("fil", "fil"),
            ("pt-br", "pt-BR"),
            ("zh-TW", "zh-TW"),
            ("es-419", "es-419"),
        ];
        for (text, tag) in accepted {
            assert_eq!(SubtitleLanguage::parse(text).unwrap().tag(), tag, "{text}");
        }
        let refused = [
            "", "r", "romana", "../x", "ro/", "ro-", "-ro", "ro-RO-x", "ro_RO", "ro RO", " ro",
            "ro-1", "ro-12", "ro-1234", "ro-R", "ro-ROU", "ră", "r1",
        ];
        for text in refused {
            assert_eq!(
                SubtitleLanguage::parse(text),
                Err(InvalidLanguageTag),
                "{text:?}"
            );
        }
    }

    /// ENC-13.
    #[test]
    fn hints_use_the_right_country_domains() {
        let expected = [
            ("ro", Some("ro")),
            ("uk", Some("ua")),
            ("ar", Some("eg")),
            ("el", Some("gr")),
            ("ja", Some("jp")),
            ("ko", Some("kr")),
            ("he", Some("il")),
            ("sl", Some("si")),
            ("sr", Some("ba")),
            ("zh", Some("cn")),
            ("zh-TW", Some("tw")),
            ("zh-hk", Some("hk")),
            ("ro-MD", Some("ro")),
            ("fr", None),
            ("en", None),
            ("cy", None),
            ("xx", None),
        ];
        for (tag, domain) in expected {
            assert_eq!(hint(tag), domain, "{tag}");
        }
    }

    /// ENC-13. The detector panics on domains that are not lower-case ASCII.
    #[test]
    fn hint_table_is_safe_for_the_detector() {
        for (language, domain) in DETECTION_HINTS {
            assert_eq!(SubtitleLanguage::parse(language).unwrap().tag(), *language);
            assert!(
                domain.bytes().all(|byte| byte.is_ascii_lowercase()),
                "{domain}"
            );
            assert!(
                EncodingDetector::tld_may_affect_guess(Some(domain.as_bytes())),
                "{domain}"
            );
        }
    }
}
