use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::Encoding;

use crate::classification::{Classification, count_non_ascii};
use crate::constants::MINIMUM_DETECTION_EVIDENCE_BYTES;
use crate::decoding::{DecodingProblem, decode_strictly};
use crate::encoding_catalog::manual_choices;
use crate::language::SubtitleLanguage;

/// The outcome of ENC-10 for a file that needs detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    /// The file is Ready.
    Certain(&'static Encoding),
    /// The file Needs review, with the encoding to offer first.
    NeedsReview {
        suggestion: &'static Encoding,
        reason: ReviewReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewReason {
    /// Fewer than LIMIT-05 non-ASCII bytes.
    TooLittleEvidence,
    /// The best guess does not decode strictly (ENC-11).
    GuessDoesNotDecode,
    /// The subtitle language changes the decoded text (ENC-13).
    LanguageHintDisagrees,
}

/// Why a hand-chosen encoding was refused (ENC-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualChoiceRefusal {
    /// Not in the offered list (UI-09).
    NotOffered,
    /// The file takes no manual choice: it has a byte-order mark, is valid UTF-8, is empty
    /// or is not text.
    NotApplicable,
    DoesNotDecode(DecodingProblem),
}

/// ENC-09 to ENC-11 and ENC-13, for a file classified as needing detection.
pub fn detect(bytes: &[u8], language: Option<&SubtitleLanguage>) -> Detection {
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let hint = language.and_then(SubtitleLanguage::detection_hint);
    let guess = detector.guess(hint.map(str::as_bytes), Utf8Detection::Deny);
    let guess_without_hint = detector.guess(None, Utf8Detection::Deny);
    if count_non_ascii(bytes) < MINIMUM_DETECTION_EVIDENCE_BYTES {
        return needs_review(guess, ReviewReason::TooLittleEvidence);
    }
    let Ok(text) = decode_strictly(bytes, guess) else {
        return needs_review(guess, ReviewReason::GuessDoesNotDecode);
    };
    let hint_changes_text = guess != guess_without_hint
        && decode_strictly(bytes, guess_without_hint).as_ref() != Ok(&text);
    if hint_changes_text {
        return needs_review(guess, ReviewReason::LanguageHintDisagrees);
    }
    Detection::Certain(guess)
}

fn needs_review(suggestion: &'static Encoding, reason: ReviewReason) -> Detection {
    Detection::NeedsReview { suggestion, reason }
}

/// ENC-12: whether a hand-chosen encoding can be used for a file.
pub fn check_manual_choice(
    bytes: &[u8],
    classification: Classification,
    encoding: &'static Encoding,
) -> Result<(), ManualChoiceRefusal> {
    if !manual_choices().contains(&encoding) {
        return Err(ManualChoiceRefusal::NotOffered);
    }
    if !takes_manual_choice(classification) {
        return Err(ManualChoiceRefusal::NotApplicable);
    }
    decode_strictly(bytes, encoding)
        .map(|_| ())
        .map_err(ManualChoiceRefusal::DoesNotDecode)
}

/// ENC-12: files identified by a byte-order mark, valid UTF-8, empty or not text take no
/// hand-chosen encoding.
pub fn takes_manual_choice(classification: Classification) -> bool {
    matches!(
        classification,
        Classification::NeedsDetection
            | Classification::LooksLikeUtf16WithoutByteOrderMark(_)
            | Classification::DamagedOrMixedUtf8
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classification::classify;
    use crate::test_fixtures::{expected, source};
    use encoding_rs::{
        BIG5, EUC_JP, EUC_KR, GBK, IBM866, ISO_2022_JP, ISO_8859_2, ISO_8859_4, ISO_8859_5,
        ISO_8859_6, ISO_8859_7, ISO_8859_8, ISO_8859_13, ISO_8859_16, KOI8_U, SHIFT_JIS, UTF_8,
        UTF_16LE, WINDOWS_874, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1253,
        WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257, WINDOWS_1258,
    };

    fn language(tag: &str) -> SubtitleLanguage {
        SubtitleLanguage::parse(tag).unwrap()
    }

    /// Detects a fixture with and without its language. Both must be certain and give the
    /// exact expected text, though they may name different sibling encodings (ENC-10).
    fn assert_detected_exactly(fixture: &str, tag: &str) -> &'static Encoding {
        let bytes = source(fixture);
        assert_eq!(
            classify(&bytes),
            Classification::NeedsDetection,
            "{fixture}"
        );
        let with_language = detect(&bytes, Some(&language(tag)));
        let without_language = detect(&bytes, None);
        let (Detection::Certain(encoding), Detection::Certain(encoding_without_language)) =
            (with_language, without_language)
        else {
            panic!("{fixture}: with {tag} {with_language:?}, without {without_language:?}");
        };
        for detected in [encoding, encoding_without_language] {
            assert_eq!(
                decode_strictly(&bytes, detected).unwrap(),
                expected(fixture),
                "{fixture} as {}",
                detected.name()
            );
        }
        encoding
    }

    /// ENC-09 and ENC-10.
    #[test]
    fn decodes_romanian_windows_1250() {
        assert_eq!(
            assert_detected_exactly("windows-1250-romanian", "ro"),
            WINDOWS_1250
        );
    }

    /// ENC-09 and ENC-10.
    #[test]
    fn decodes_romanian_iso_8859_2() {
        assert_detected_exactly("iso-8859-2-romanian", "ro");
    }

    /// ENC-09 and ENC-10.
    #[test]
    fn decodes_other_romanian_fixtures() {
        for fixture in ["nonstandard", "line-endings-mixed", "markup"] {
            assert_detected_exactly(fixture, "ro");
        }
    }

    /// ENC-09 and ENC-10.
    #[test]
    fn decodes_french_windows_1252() {
        assert_detected_exactly("windows-1252-french", "fr");
    }

    /// ENC-09 and ENC-10.
    #[test]
    fn decodes_russian_windows_1251() {
        assert_detected_exactly("windows-1251-russian", "ru");
    }

    /// ENC-09 and ENC-10.
    #[test]
    fn decodes_greek_windows_1253() {
        assert_detected_exactly("windows-1253-greek", "el");
    }

    /// ENC-09 and ENC-10.
    #[test]
    fn decodes_arabic_windows_1256() {
        assert_detected_exactly("windows-1256-arabic", "ar");
    }

    /// ENC-09, ENC-10 and ENC-17.
    #[test]
    fn decodes_japanese_shift_jis() {
        assert_detected_exactly("shift-jis-japanese", "ja");
    }

    /// ENC-09, ENC-10 and ENC-17.
    #[test]
    fn decodes_korean_euc_kr() {
        assert_detected_exactly("euc-kr-korean", "ko");
    }

    /// ENC-10 and LIMIT-05. The suggestion still decodes the text correctly.
    #[test]
    fn short_input_needs_review() {
        let bytes = source("short-romanian");
        let Detection::NeedsReview { suggestion, reason } = detect(&bytes, Some(&language("ro")))
        else {
            panic!("short input was accepted");
        };
        assert_eq!(reason, ReviewReason::TooLittleEvidence);
        assert_eq!(
            decode_strictly(&bytes, suggestion).unwrap(),
            expected("short-romanian")
        );
    }

    /// ENC-10 and ENC-13: French text with Romanian as its language.
    #[test]
    fn hint_disagreement_needs_review() {
        let bytes = source("windows-1252-french");
        assert_eq!(
            detect(&bytes, Some(&language("ro"))),
            Detection::NeedsReview {
                suggestion: WINDOWS_1250,
                reason: ReviewReason::LanguageHintDisagrees
            }
        );
    }

    /// ENC-12 and the known limit: detection gives cedilla letters; choosing ISO-8859-16
    /// by hand gives the comma letters exactly.
    #[test]
    fn iso_8859_16_needs_manual_choice() {
        let bytes = source("iso-8859-16-romanian");
        let text = expected("iso-8859-16-romanian");
        let cedilla_text: String = text
            .chars()
            .map(|character| match character {
                'ș' => 'ş',
                'ț' => 'ţ',
                'Ș' => 'Ş',
                'Ț' => 'Ţ',
                other => other,
            })
            .collect();
        let Detection::Certain(detected) = detect(&bytes, Some(&language("ro"))) else {
            panic!("ISO-8859-16 fixture was not detected");
        };
        assert_eq!(decode_strictly(&bytes, detected).unwrap(), cedilla_text);
        assert_eq!(
            check_manual_choice(&bytes, Classification::NeedsDetection, ISO_8859_16),
            Ok(())
        );
        assert_eq!(decode_strictly(&bytes, ISO_8859_16).unwrap(), text);
    }

    /// ENC-07 and ENC-12.
    #[test]
    fn utf16_without_bom_is_exact_after_manual_choice() {
        let bytes = source("utf16le-no-bom");
        assert_eq!(
            check_manual_choice(&bytes, classify(&bytes), UTF_16LE),
            Ok(())
        );
        assert_eq!(
            decode_strictly(&bytes, UTF_16LE).unwrap(),
            expected("utf16le-no-bom")
        );
    }

    /// ENC-11 and ENC-12: ISO-8859-2 lacks the typographic quotes of windows-1250.
    #[test]
    fn manual_choice_is_strictly_decoded() {
        let bytes = source("markup");
        let first_quote = bytes.iter().position(|byte| *byte == 0x84).unwrap();
        assert_eq!(
            check_manual_choice(&bytes, Classification::NeedsDetection, ISO_8859_2),
            Err(ManualChoiceRefusal::DoesNotDecode(
                DecodingProblem::ControlCharacter {
                    byte_offset: first_quote
                }
            ))
        );
        assert!(matches!(
            check_manual_choice(&bytes, Classification::NeedsDetection, SHIFT_JIS),
            Err(ManualChoiceRefusal::DoesNotDecode(
                DecodingProblem::InvalidBytes { .. }
            ))
        ));
    }

    /// ENC-12.
    #[test]
    fn manual_choice_is_refused_where_it_does_not_apply() {
        let without_choice = [
            Classification::Empty,
            Classification::Utf8WithByteOrderMark,
            Classification::DamagedUtf8WithByteOrderMark { byte_offset: 0 },
            Classification::Utf16WithByteOrderMark(UTF_16LE),
            Classification::Utf8WithoutByteOrderMark,
            Classification::NotText,
        ];
        for classification in without_choice {
            assert_eq!(
                check_manual_choice(b"text", classification, WINDOWS_1250),
                Err(ManualChoiceRefusal::NotApplicable),
                "{classification:?}"
            );
        }
    }

    /// ENC-12 and UI-09.
    #[test]
    fn only_offered_encodings_can_be_chosen() {
        let bytes = source("mixed-utf8-1250");
        for encoding in [UTF_8, ISO_2022_JP] {
            assert_eq!(
                check_manual_choice(&bytes, Classification::DamagedOrMixedUtf8, encoding),
                Err(ManualChoiceRefusal::NotOffered),
                "{}",
                encoding.name()
            );
        }
    }

    /// UI-09: every encoding the detector can guess, per its documentation, is in the list,
    /// so a suggestion can always be shown as selected.
    #[test]
    fn every_encoding_the_detector_can_return_is_offered() {
        let detectable = [
            GBK,
            BIG5,
            EUC_KR,
            SHIFT_JIS,
            EUC_JP,
            WINDOWS_1250,
            WINDOWS_1251,
            WINDOWS_1252,
            WINDOWS_1253,
            WINDOWS_1254,
            WINDOWS_1255,
            WINDOWS_1256,
            WINDOWS_1257,
            WINDOWS_1258,
            WINDOWS_874,
            ISO_8859_2,
            ISO_8859_4,
            ISO_8859_5,
            ISO_8859_6,
            ISO_8859_7,
            ISO_8859_8,
            ISO_8859_13,
            KOI8_U,
            IBM866,
        ];
        for encoding in detectable {
            assert!(manual_choices().contains(&encoding), "{}", encoding.name());
        }
    }

    /// ENC-09: the detector is never allowed to guess UTF-8 or ISO-2022-JP.
    #[test]
    fn detector_never_guesses_utf8_or_iso_2022_jp() {
        let iso_2022_jp_text = b"\x1b$B$3$s$K$A$O\x1b(B, \x1b$B$3$s$K$A$O\x1b(B";
        for bytes in [&source("mixed-utf8-1250")[..], &iso_2022_jp_text[..]] {
            let (Detection::Certain(guess)
            | Detection::NeedsReview {
                suggestion: guess, ..
            }) = detect(bytes, None);
            assert!(guess != UTF_8 && guess != ISO_2022_JP, "{}", guess.name());
        }
    }
}
