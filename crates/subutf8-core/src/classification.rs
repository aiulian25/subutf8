use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

use crate::constants::{
    HUNDRED_PERCENT, MIXED_UTF8_MINIMUM_VALID_PERCENT, UTF16_CODE_UNIT_BYTES,
    UTF16_MINIMUM_NUL_PERCENT, UTF16_MINIMUM_SAME_PARITY_PERCENT,
};
use crate::decoding::Location;

/// What rules ENC-03 to ENC-09 decide about a file's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// ENC-03.
    Empty,
    /// ENC-04, valid.
    Utf8WithByteOrderMark,
    /// ENC-04, invalid.
    DamagedUtf8WithByteOrderMark { location: Location },
    /// ENC-05. Whether the rest decodes is checked when converting.
    Utf16WithByteOrderMark(&'static Encoding),
    /// ENC-06.
    Utf8WithoutByteOrderMark,
    /// ENC-07, UTF-16 pattern. Holds the variant to offer first.
    LooksLikeUtf16WithoutByteOrderMark(&'static Encoding),
    /// ENC-07, any other NUL bytes.
    NotText,
    /// ENC-08.
    DamagedOrMixedUtf8,
    /// ENC-09.
    NeedsDetection,
}

pub fn classify(bytes: &[u8]) -> Classification {
    if let Some((encoding, byte_order_mark_length)) = Encoding::for_bom(bytes) {
        return classify_after_byte_order_mark(encoding, bytes, byte_order_mark_length);
    }
    if bytes.is_empty() {
        return Classification::Empty;
    }
    if bytes.contains(&0) {
        return classify_nul_bytes(bytes);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return Classification::Utf8WithoutByteOrderMark;
    }
    if is_damaged_or_mixed_utf8(bytes) {
        return Classification::DamagedOrMixedUtf8;
    }
    Classification::NeedsDetection
}

fn classify_after_byte_order_mark(
    encoding: &'static Encoding,
    bytes: &[u8],
    byte_order_mark_length: usize,
) -> Classification {
    let content = &bytes[byte_order_mark_length..];
    if content.is_empty() {
        return Classification::Empty;
    }
    if encoding != UTF_8 {
        return Classification::Utf16WithByteOrderMark(encoding);
    }
    match std::str::from_utf8(content) {
        Ok(_) => Classification::Utf8WithByteOrderMark,
        Err(error) => Classification::DamagedUtf8WithByteOrderMark {
            location: Location::in_file(bytes, byte_order_mark_length + error.valid_up_to(), UTF_8),
        },
    }
}

fn classify_nul_bytes(bytes: &[u8]) -> Classification {
    let nul_at_even_offsets = count_nul(bytes.iter().step_by(UTF16_CODE_UNIT_BYTES));
    let nul_at_odd_offsets = count_nul(bytes.iter().skip(1).step_by(UTF16_CODE_UNIT_BYTES));
    let nul_count = nul_at_even_offsets + nul_at_odd_offsets;
    let follows_utf16_pattern =
        reaches_percentage(nul_count, bytes.len(), UTF16_MINIMUM_NUL_PERCENT)
            && reaches_percentage(
                nul_at_even_offsets.max(nul_at_odd_offsets),
                nul_count,
                UTF16_MINIMUM_SAME_PARITY_PERCENT,
            );
    if !follows_utf16_pattern {
        return Classification::NotText;
    }
    // ASCII text in UTF-16LE puts the NUL byte second in each pair, at an odd offset.
    let little_endian_is_likely = nul_at_odd_offsets > nul_at_even_offsets;
    let likely_variant = if little_endian_is_likely {
        UTF_16LE
    } else {
        UTF_16BE
    };
    Classification::LooksLikeUtf16WithoutByteOrderMark(likely_variant)
}

fn is_damaged_or_mixed_utf8(bytes: &[u8]) -> bool {
    reaches_percentage(
        non_ascii_bytes_in_valid_utf8(bytes),
        count_non_ascii(bytes),
        MIXED_UTF8_MINIMUM_VALID_PERCENT,
    )
}

fn non_ascii_bytes_in_valid_utf8(mut remaining: &[u8]) -> usize {
    let mut counted = 0;
    loop {
        let error = match std::str::from_utf8(remaining) {
            Ok(valid) => return counted + count_non_ascii(valid.as_bytes()),
            Err(error) => error,
        };
        counted += count_non_ascii(&remaining[..error.valid_up_to()]);
        let Some(invalid_length) = error.error_len() else {
            return counted;
        };
        remaining = &remaining[error.valid_up_to() + invalid_length..];
    }
}

fn count_nul<'a>(bytes: impl Iterator<Item = &'a u8>) -> usize {
    bytes.filter(|byte| **byte == 0).count()
}

pub(crate) fn count_non_ascii(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| !byte.is_ascii()).count()
}

fn reaches_percentage(part: usize, whole: usize, percentage: usize) -> bool {
    part * HUNDRED_PERCENT >= whole * percentage
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::source as fixture;

    fn utf16(text: &str, to_bytes: fn(u16) -> [u8; 2]) -> Vec<u8> {
        text.encode_utf16().flat_map(to_bytes).collect()
    }

    /// ENC-03.
    #[test]
    fn empty_file_is_skipped() {
        assert_eq!(classify(&fixture("empty")), Classification::Empty);
        assert_eq!(classify(&fixture("utf8-bom-only")), Classification::Empty);
        assert_eq!(classify(&[0xFF, 0xFE]), Classification::Empty);
    }

    /// ENC-04.
    #[test]
    fn utf8_byte_order_mark_is_recognised() {
        assert_eq!(
            classify(&fixture("utf8-bom-romanian")),
            Classification::Utf8WithByteOrderMark
        );
    }

    /// ENC-04.
    #[test]
    fn damaged_utf8_after_byte_order_mark_reports_its_offset() {
        let bytes = [0xEF, 0xBB, 0xBF, b'O', b'K', 0xC3];
        assert_eq!(
            classify(&bytes),
            Classification::DamagedUtf8WithByteOrderMark {
                location: Location {
                    byte_offset: 5,
                    line: 1
                }
            }
        );
    }

    /// ENC-05.
    #[test]
    fn utf16_byte_order_marks_are_recognised() {
        assert_eq!(
            classify(&fixture("utf16le-bom")),
            Classification::Utf16WithByteOrderMark(UTF_16LE)
        );
        let big_endian = [&[0xFE, 0xFF][..], &utf16("Bună", u16::to_be_bytes)].concat();
        assert_eq!(
            classify(&big_endian),
            Classification::Utf16WithByteOrderMark(UTF_16BE)
        );
    }

    /// ENC-06.
    #[test]
    fn valid_utf8_is_recognised() {
        assert_eq!(
            classify(&fixture("utf8-romanian")),
            Classification::Utf8WithoutByteOrderMark
        );
    }

    /// ENC-06.
    #[test]
    fn pure_ascii_counts_as_utf8() {
        assert_eq!(
            classify(&fixture("ascii-only")),
            Classification::Utf8WithoutByteOrderMark
        );
    }

    /// ENC-07.
    #[test]
    fn utf16_without_bom_needs_review() {
        assert_eq!(
            classify(&fixture("utf16le-no-bom")),
            Classification::LooksLikeUtf16WithoutByteOrderMark(UTF_16LE)
        );
        assert_eq!(
            classify(&utf16("Hello there.", u16::to_be_bytes)),
            Classification::LooksLikeUtf16WithoutByteOrderMark(UTF_16BE)
        );
    }

    /// ENC-06 and ENC-07: UTF-16 made of ASCII letters is also valid UTF-8.
    #[test]
    fn ascii_utf16_is_not_mistaken_for_utf8() {
        let bytes = utf16("Hello there.", u16::to_le_bytes);
        assert!(std::str::from_utf8(&bytes).is_ok());
        assert_eq!(
            classify(&bytes),
            Classification::LooksLikeUtf16WithoutByteOrderMark(UTF_16LE)
        );
    }

    /// ENC-07.
    #[test]
    fn binary_input_is_refused() {
        assert_eq!(classify(&fixture("binary")), Classification::NotText);
    }

    /// ENC-08.
    #[test]
    fn truncated_utf8_is_not_converted() {
        assert_eq!(
            classify(&fixture("truncated-utf8")),
            Classification::DamagedOrMixedUtf8
        );
    }

    /// ENC-08.
    #[test]
    fn mixed_encoding_needs_review() {
        assert_eq!(
            classify(&fixture("mixed-utf8-1250")),
            Classification::DamagedOrMixedUtf8
        );
    }

    /// ENC-08 and ENC-09. Shift_JIS text is the closest to the mixed-UTF-8 line.
    #[test]
    fn legacy_encodings_go_to_detection() {
        let legacy_fixtures = [
            "windows-1250-romanian",
            "iso-8859-2-romanian",
            "iso-8859-16-romanian",
            "windows-1252-french",
            "windows-1251-russian",
            "windows-1253-greek",
            "windows-1256-arabic",
            "shift-jis-japanese",
            "euc-kr-korean",
            "short-romanian",
            "nonstandard",
            "line-endings-mixed",
            "markup",
        ];
        for name in legacy_fixtures {
            assert_eq!(
                classify(&fixture(name)),
                Classification::NeedsDetection,
                "{name}"
            );
        }
    }
}
