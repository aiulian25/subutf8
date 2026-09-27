use std::borrow::Cow;

use crate::constants::{
    LANGUAGE_SUBTAG_LETTERS, REGION_SUBTAG_DIGITS, REGION_SUBTAG_LETTERS, ROMANIAN_COMMA_LETTERS,
    ROMANIAN_LANGUAGE, SUBTAG_SEPARATOR,
};

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
        let (language, region) = match text.split_once(SUBTAG_SEPARATOR) {
            Some((language, region)) => (language, Some(region)),
            None => (text, None),
        };
        if !is_language_subtag(language) || !region.is_none_or(is_region_subtag) {
            return Err(InvalidLanguageTag);
        }
        let mut tag = language.to_ascii_lowercase();
        if let Some(region) = region {
            tag.push(SUBTAG_SEPARATOR);
            tag.push_str(&region.to_ascii_uppercase());
        }
        Ok(Self { tag })
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The country domain that steers detection towards this language's legacy encodings.
    pub fn detection_hint(&self) -> Option<&'static str> {
        find_hint(&self.tag).or_else(|| find_hint(self.language_subtag()))
    }

    /// ENC-23: `ro`, with or without a region such as `ro-MD`.
    pub fn is_romanian(&self) -> bool {
        self.language_subtag() == ROMANIAN_LANGUAGE
    }

    fn language_subtag(&self) -> &str {
        self.tag.split(SUBTAG_SEPARATOR).next().unwrap_or(&self.tag)
    }
}

/// ENC-23: the text a subtitle is written with. A Romanian one takes the comma letters when
/// they are chosen.
pub fn written_text<'text>(
    text: &'text str,
    language: Option<&SubtitleLanguage>,
    comma_letters: bool,
) -> Cow<'text, str> {
    let applies = comma_letters && language.is_some_and(SubtitleLanguage::is_romanian);
    if !applies {
        return Cow::Borrowed(text);
    }
    with_romanian_comma_letters(text)
}

/// ENC-23: ș ț Ș Ț in place of the cedilla letters ş ţ Ş Ţ, the only ones windows-1250 and
/// ISO-8859-2 have.
pub fn with_romanian_comma_letters(text: &str) -> Cow<'_, str> {
    if !text
        .chars()
        .any(|character| comma_letter(character).is_some())
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .map(|character| comma_letter(character).unwrap_or(character))
            .collect(),
    )
}

fn comma_letter(character: char) -> Option<char> {
    ROMANIAN_COMMA_LETTERS
        .iter()
        .find(|(cedilla, _)| *cedilla == character)
        .map(|(_, comma)| *comma)
}

/// ENC-13 and UI-19: the languages that steer detection, Romanian first, as the page suggests
/// them.
pub fn hinted_languages() -> Vec<&'static str> {
    DETECTION_HINTS
        .iter()
        .map(|(language, _)| *language)
        .collect()
}

/// UI-15: every country domain of the hint table once, in table order.
pub(crate) fn detection_domains() -> Vec<&'static str> {
    let mut domains = Vec::new();
    for (_, domain) in DETECTION_HINTS {
        if !domains.contains(domain) {
            domains.push(*domain);
        }
    }
    domains
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

    /// UI-19: every language of the hint table once, Romanian first.
    #[test]
    fn hinted_languages_start_with_romanian() {
        let languages = hinted_languages();
        assert_eq!(languages[0], "ro");
        for language in ["lt", "zh-TW", "fo"] {
            assert!(languages.contains(&language), "{language}");
        }
        for (position, language) in languages.iter().enumerate() {
            assert!(!languages[position + 1..].contains(language), "{language}");
        }
        assert_eq!(languages.len(), DETECTION_HINTS.len());
    }

    /// UI-15: Bosnian and Serbian, Hebrew and Yiddish, and both Chinese tags share domains.
    #[test]
    fn detection_domains_are_distinct() {
        let domains = detection_domains();
        assert_eq!(domains[0], "ro");
        for (position, domain) in domains.iter().enumerate() {
            assert!(!domains[position + 1..].contains(domain), "{domain}");
        }
        for (_, domain) in DETECTION_HINTS {
            assert!(domains.contains(domain), "{domain}");
        }
    }

    /// ENC-23.
    #[test]
    fn romanian_tags_are_recognised() {
        for (tag, is_romanian) in [
            ("ro", true),
            ("RO", true),
            ("ro-MD", true),
            ("rom", false),
            ("en", false),
            ("pt-BR", false),
        ] {
            let language = SubtitleLanguage::parse(tag).unwrap();
            assert_eq!(language.is_romanian(), is_romanian, "{tag}");
        }
    }

    /// ENC-23: only a Romanian subtitle with comma letters chosen changes, and text without
    /// cedilla letters is not copied.
    #[test]
    fn cedilla_letters_become_comma_letters() {
        let text = "Bună dimineaţa! Ştii? Aşa, ŢARA.";
        let comma_text = "Bună dimineața! Știi? Așa, ȚARA.";
        assert_eq!(with_romanian_comma_letters(text), comma_text);
        assert!(matches!(
            with_romanian_comma_letters("abc"),
            Cow::Borrowed("abc")
        ));
        let romanian = SubtitleLanguage::parse("ro").ok();
        let english = SubtitleLanguage::parse("en").ok();
        assert_eq!(written_text(text, romanian.as_ref(), true), comma_text);
        assert_eq!(written_text(text, romanian.as_ref(), false), text);
        assert_eq!(written_text(text, english.as_ref(), true), text);
        assert_eq!(written_text(text, None, true), text);
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
