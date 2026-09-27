use std::collections::HashSet;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

use crate::classification::{Classification, count_non_ascii};
use crate::constants::{C1_CONTROL_CHARACTERS, CANDIDATE_LIMIT, MINIMUM_DETECTION_EVIDENCE_BYTES};
use crate::decoding::{
    ConversionFailure, DecodingProblem, MisreadVia, Misreading, Reading, convert_reading,
    decode_strictly, misread_bytes,
};
use crate::encoding_catalog::manual_choices;
use crate::language::{SubtitleLanguage, detection_domains};
use crate::preview::sample_line;
use crate::srt_structure::split_byte_lines;

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

/// UI-15: an encoding a person may want, with a line to recognise it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub encoding: &'static Encoding,
    pub sample: String,
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

/// ENC-22: how UTF-8 text was garbled, when it turns back without loss into bytes that decode
/// strictly to other text. Text with C1 characters was read through Latin-1, else through
/// windows-1252.
pub fn find_misreading(text: &str, language: Option<&SubtitleLanguage>) -> Option<Misreading> {
    if text.is_ascii() {
        return None;
    }
    let has_c1_characters = text
        .chars()
        .any(|character| C1_CONTROL_CHARACTERS.contains(&character));
    let via = if has_c1_characters {
        MisreadVia::Latin1
    } else {
        MisreadVia::Windows1252
    };
    let bytes = misread_bytes(text, via)?;
    let original = original_encoding(&bytes, via, language)?;
    let repaired = decode_strictly(&bytes, original).ok()?;
    (repaired != text).then_some(Misreading { via, original })
}

/// ENC-22: UTF-8 when the bytes are valid UTF-8, else the detector's encoding. It must be
/// certain, so real accented UTF-8 text is not taken for garbled, unless C1 characters, which
/// subtitles never use, already show the misreading.
fn original_encoding(
    bytes: &[u8],
    via: MisreadVia,
    language: Option<&SubtitleLanguage>,
) -> Option<&'static Encoding> {
    if std::str::from_utf8(bytes).is_ok() {
        return Some(UTF_8);
    }
    match (via, detect(bytes, language)) {
        (_, Detection::Certain(encoding))
        | (
            MisreadVia::Latin1,
            Detection::NeedsReview {
                suggestion: encoding,
                ..
            },
        ) => Some(encoding),
        (MisreadVia::Windows1252, Detection::NeedsReview { .. }) => None,
    }
}

/// ENC-12 and ENC-21: whether a hand-chosen reading can be used for a file. Keeping UTF-8 lines
/// applies to damaged or mixed files only. The file must convert with it.
pub fn check_manual_reading(
    bytes: &[u8],
    classification: Classification,
    reading: Reading,
) -> Result<(), ManualChoiceRefusal> {
    if !manual_choices().contains(&reading.encoding()) {
        return Err(ManualChoiceRefusal::NotOffered);
    }
    let keeps_utf8_lines = matches!(reading, Reading::Utf8LinesElse(_));
    let is_mixed = classification == Classification::DamagedOrMixedUtf8;
    if !takes_manual_choice(classification) || (keeps_utf8_lines && !is_mixed) {
        return Err(ManualChoiceRefusal::NotApplicable);
    }
    convert_reading(bytes, reading)
        .map(|_| ())
        .map_err(|failure| match failure {
            ConversionFailure::DoesNotDecode(problem) => {
                ManualChoiceRefusal::DoesNotDecode(problem)
            }
            other => ManualChoiceRefusal::DoesNotDecode(DecodingProblem::InvalidBytes {
                location: other.location(),
            }),
        })
}

/// UI-15: offered encodings that decode the file strictly, most likely first, at most
/// CANDIDATE_LIMIT. Files that take no hand-chosen encoding have none.
pub fn candidates(
    bytes: &[u8],
    classification: Classification,
    language: Option<&SubtitleLanguage>,
) -> Vec<Candidate> {
    if !takes_manual_choice(classification) {
        return Vec::new();
    }
    let mut tried = HashSet::new();
    candidate_order(bytes, classification, language)
        .into_iter()
        .filter(|encoding| manual_choices().contains(encoding) && tried.insert(*encoding))
        .filter_map(|encoding| {
            let sample = candidate_sample(bytes, classification, encoding)?;
            Some(Candidate { encoding, sample })
        })
        .take(CANDIDATE_LIMIT)
        .collect()
}

/// UI-15 and ENC-21: a mixed file is read with its UTF-8 lines kept, and shown by its first line
/// that is not UTF-8, which is the one the encoding decides.
fn candidate_sample(
    bytes: &[u8],
    classification: Classification,
    encoding: &'static Encoding,
) -> Option<String> {
    if classification != Classification::DamagedOrMixedUtf8 {
        return decode_strictly(bytes, encoding)
            .ok()
            .map(|text| sample_line(&text));
    }
    convert_reading(bytes, Reading::Utf8LinesElse(encoding)).ok()?;
    let line = split_byte_lines(bytes).find(|line| std::str::from_utf8(line).is_err())?;
    decode_strictly(line, encoding)
        .ok()
        .map(|text| sample_line(&text))
}

/// UTF-16 in the byte order its pattern suggests, then the other. Otherwise the detector's
/// guess for the subtitle language, its guess without one, then its guess for each hint
/// domain, as every one steers it towards another family of encodings.
fn candidate_order(
    bytes: &[u8],
    classification: Classification,
    language: Option<&SubtitleLanguage>,
) -> Vec<&'static Encoding> {
    if let Classification::LooksLikeUtf16WithoutByteOrderMark(likely) = classification {
        return vec![likely, UTF_16LE, UTF_16BE];
    }
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let language_hint = language.and_then(SubtitleLanguage::detection_hint);
    [language_hint, None]
        .into_iter()
        .chain(detection_domains().into_iter().map(Some))
        .map(|hint| detector.guess(hint.map(str::as_bytes), Utf8Detection::Deny))
        .collect()
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
    use crate::decoding::Location;
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
            check_manual_reading(
                &bytes,
                Classification::NeedsDetection,
                Reading::Whole(ISO_8859_16)
            ),
            Ok(())
        );
        assert_eq!(decode_strictly(&bytes, ISO_8859_16).unwrap(), text);
    }

    /// ENC-07 and ENC-12.
    #[test]
    fn utf16_without_bom_is_exact_after_manual_choice() {
        let bytes = source("utf16le-no-bom");
        assert_eq!(
            check_manual_reading(&bytes, classify(&bytes), Reading::Whole(UTF_16LE)),
            Ok(())
        );
        assert_eq!(
            decode_strictly(&bytes, UTF_16LE).unwrap(),
            expected("utf16le-no-bom")
        );
    }

    /// ENC-11 and ENC-12: ISO-8859-2 lacks the typographic quotes of windows-1250, and the
    /// first one is on line 3.
    #[test]
    fn manual_choice_is_strictly_decoded() {
        let bytes = source("markup");
        let first_quote = bytes.iter().position(|byte| *byte == 0x84).unwrap();
        assert_eq!(
            check_manual_reading(
                &bytes,
                Classification::NeedsDetection,
                Reading::Whole(ISO_8859_2)
            ),
            Err(ManualChoiceRefusal::DoesNotDecode(
                DecodingProblem::ControlCharacter {
                    location: Location {
                        byte_offset: first_quote,
                        line: 3
                    }
                }
            ))
        );
        assert!(matches!(
            check_manual_reading(
                &bytes,
                Classification::NeedsDetection,
                Reading::Whole(SHIFT_JIS)
            ),
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
            Classification::DamagedUtf8WithByteOrderMark {
                location: Location {
                    byte_offset: 0,
                    line: 1,
                },
            },
            Classification::Utf16WithByteOrderMark(UTF_16LE),
            Classification::Utf8WithoutByteOrderMark,
            Classification::NotText,
        ];
        for classification in without_choice {
            assert_eq!(
                check_manual_reading(b"text", classification, Reading::Whole(WINDOWS_1250)),
                Err(ManualChoiceRefusal::NotApplicable),
                "{classification:?}"
            );
        }
        let keep_utf8_lines = Reading::Utf8LinesElse(WINDOWS_1250);
        assert_eq!(
            check_manual_reading(b"text", Classification::NeedsDetection, keep_utf8_lines),
            Err(ManualChoiceRefusal::NotApplicable)
        );
    }

    /// ENC-12 and ENC-21: a mixed file fits windows-1250 only with its UTF-8 lines kept.
    #[test]
    fn mixed_file_takes_a_reading_that_keeps_utf8_lines() {
        let bytes = source("mixed-utf8-1250");
        let mixed = Classification::DamagedOrMixedUtf8;
        assert_eq!(
            check_manual_reading(&bytes, mixed, Reading::Utf8LinesElse(WINDOWS_1250)),
            Ok(())
        );
        assert!(matches!(
            check_manual_reading(&bytes, mixed, Reading::Whole(WINDOWS_1250)),
            Err(ManualChoiceRefusal::DoesNotDecode(_))
        ));
        let found = candidates(&bytes, mixed, Some(&language("ro")));
        assert!(
            found
                .iter()
                .any(|candidate| candidate.encoding == WINDOWS_1250
                    && candidate.sample == "Bună dimineaţa! Ştii ce înseamnă asta?")
        );
    }

    /// ENC-12 and UI-09.
    #[test]
    fn only_offered_encodings_can_be_chosen() {
        let bytes = source("mixed-utf8-1250");
        for encoding in [UTF_8, ISO_2022_JP] {
            assert_eq!(
                check_manual_reading(
                    &bytes,
                    Classification::DamagedOrMixedUtf8,
                    Reading::Whole(encoding)
                ),
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

    /// UI-15: the guess for the language comes first and reads correctly.
    #[test]
    fn short_romanian_candidates_are_readable() {
        let bytes = source("short-romanian");
        let found = candidates(
            &bytes,
            Classification::NeedsDetection,
            Some(&language("ro")),
        );
        assert!(
            found.len() > 1 && found.len() <= CANDIDATE_LIMIT,
            "{found:?}"
        );
        assert_eq!(found[0].sample, "Bună!");
        for candidate in &found {
            assert!(manual_choices().contains(&candidate.encoding));
            assert!(decode_strictly(&bytes, candidate.encoding).is_ok());
        }
        let mut encodings: Vec<&str> = found.iter().map(|found| found.encoding.name()).collect();
        encodings.sort_unstable();
        encodings.dedup();
        assert_eq!(encodings.len(), found.len());
    }

    /// UI-15.
    #[test]
    fn utf16_candidates_come_from_the_byte_pattern() {
        let bytes = source("utf16le-no-bom");
        let found = candidates(&bytes, classify(&bytes), None);
        assert_eq!(found[0].encoding, UTF_16LE);
        assert!(!found[0].sample.is_empty());
    }

    /// UI-15.
    #[test]
    fn utf8_files_have_no_candidates() {
        let bytes = source("utf8-romanian");
        assert!(candidates(&bytes, Classification::Utf8WithoutByteOrderMark, None).is_empty());
    }

    /// ENC-22.
    #[test]
    fn garbled_romanian_is_windows_1250_read_as_windows_1252() {
        let bytes = source("garbled-romanian");
        let text = std::str::from_utf8(&bytes).unwrap();
        let misreading = find_misreading(text, Some(&language("ro"))).unwrap();
        assert_eq!(
            misreading,
            Misreading {
                via: MisreadVia::Windows1252,
                original: WINDOWS_1250
            }
        );
        let reading = Reading::RepairMisreading(misreading);
        assert_eq!(reading.name(), "windows-1250 (repaired)");
        let without_language = Reading::RepairMisreading(find_misreading(text, None).unwrap());
        for reading in [reading, without_language] {
            assert_eq!(
                convert_reading(&bytes, reading).unwrap().text,
                expected("windows-1250-romanian")
            );
        }
    }

    /// ENC-22: real accented UTF-8 text is left alone, whatever the subtitle language.
    #[test]
    fn real_utf8_text_is_not_a_misreading() {
        let fixtures = [
            "utf8-romanian",
            "windows-1252-french",
            "ascii-only",
            "markup",
            "iso-8859-16-romanian",
            "windows-1251-russian",
            "windows-1253-greek",
            "windows-1256-arabic",
            "shift-jis-japanese",
            "euc-kr-korean",
        ];
        for fixture in fixtures {
            let text = expected(fixture);
            for tag in [None, Some("ro"), Some("fr")] {
                let subtitle_language = tag.map(language);
                assert_eq!(
                    find_misreading(&text, subtitle_language.as_ref()),
                    None,
                    "{fixture} with {tag:?}"
                );
            }
        }
    }

    /// ENC-22: UTF-8 read as windows-1252 turns back into the UTF-8 it was.
    #[test]
    fn utf8_read_as_windows_1252_is_found() {
        let bytes = source("utf8-romanian");
        let (garbled, _, _) = WINDOWS_1252.decode(&bytes);
        let misreading = find_misreading(&garbled, None).unwrap();
        assert_eq!(
            misreading,
            Misreading {
                via: MisreadVia::Windows1252,
                original: UTF_8
            }
        );
        let repaired = convert_reading(garbled.as_bytes(), Reading::RepairMisreading(misreading));
        assert_eq!(repaired.unwrap().text, expected("utf8-romanian"));
    }

    /// ENC-22: C1 characters show text read through Latin-1, and are undone through it. The
    /// windows-1252 dash U+0096 is what 1.0.0 refused with nothing left to choose.
    #[test]
    fn c1_characters_are_undone_through_latin1() {
        let bytes = source("markup");
        let as_latin1: String = bytes.iter().map(|byte| char::from(*byte)).collect();
        let misreading = find_misreading(&as_latin1, Some(&language("ro"))).unwrap();
        assert_eq!(
            misreading,
            Misreading {
                via: MisreadVia::Latin1,
                original: WINDOWS_1250
            }
        );
        let repaired = convert_reading(as_latin1.as_bytes(), Reading::RepairMisreading(misreading));
        assert_eq!(repaired.unwrap().text, expected("markup"));

        let dash = "Un \u{96} doi";
        let misreading = find_misreading(dash, None).unwrap();
        assert_eq!(misreading.via, MisreadVia::Latin1);
        let repaired = convert_reading(dash.as_bytes(), Reading::RepairMisreading(misreading));
        assert_eq!(repaired.unwrap().text, "Un \u{2013} doi");
    }
}
