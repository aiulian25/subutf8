use std::borrow::Cow;
use std::slice;

use encoding_rs::{
    BIG5_INIT, Decoder, DecoderResult, EUC_JP_INIT, EUC_KR_INIT, Encoding, GB18030_INIT, GBK_INIT,
    SHIFT_JIS_INIT, UTF_8, UTF_16BE, UTF_16LE,
};

use crate::constants::{C1_CONTROL_CHARACTERS, UTF8_MAXIMUM_CHARACTER_BYTES};

/// ENC-17: encodings whose standard decoders map a few duplicate byte sequences to one
/// character, so their round trip may differ without anything being lost.
static ROUND_TRIP_MAY_DIFFER: [&Encoding; 6] = [
    &SHIFT_JIS_INIT,
    &EUC_JP_INIT,
    &EUC_KR_INIT,
    &GBK_INIT,
    &GB18030_INIT,
    &BIG5_INIT,
];

/// Where and why strict decoding failed. Offsets count from the start of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodingProblem {
    /// A byte sequence the encoding does not define.
    InvalidBytes { byte_offset: usize },
    /// A NUL or C1 control character, which means the encoding is wrong (ENC-11).
    ControlCharacter { byte_offset: usize },
}

impl DecodingProblem {
    pub fn byte_offset(self) -> usize {
        match self {
            Self::InvalidBytes { byte_offset } | Self::ControlCharacter { byte_offset } => {
                byte_offset
            }
        }
    }
}

/// A converted file. The UTF-8 bytes of `text` are the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub text: String,
    pub warnings: Vec<ConversionWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionWarning {
    /// ENC-17, in the CJK encodings only. The offset counts from the start of the file.
    RoundTripDiffers { byte_offset: usize },
    /// ENC-18.
    ContainsReplacementCharacters,
}

/// Why a file cannot be converted. Offsets count from the start of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionFailure {
    /// ENC-04.
    DamagedUtf8 { byte_offset: usize },
    /// ENC-05, including UTF-32 files, which decode to NUL characters.
    DamagedUtf16 { byte_offset: usize },
    /// ENC-11.
    DoesNotDecode(DecodingProblem),
    /// ENC-17, outside the CJK encodings.
    RoundTripDiffers { byte_offset: usize },
}

/// ENC-11: decodes without replacing anything, refusing text with NUL or C1 characters.
pub fn decode_strictly(
    bytes: &[u8],
    encoding: &'static Encoding,
) -> Result<String, DecodingProblem> {
    decode_refusing(bytes, encoding, is_nul_or_c1_control)
}

/// Converts a file into text for UTF-8 output (ENC-04, ENC-05, ENC-11 and ENC-14 to ENC-18).
/// `encoding` applies when the file has no byte-order mark. When it has one, the mark
/// decides, because such a file takes no other encoding (ENC-12).
pub fn convert(bytes: &[u8], encoding: &'static Encoding) -> Result<Conversion, ConversionFailure> {
    let byte_order_mark = Encoding::for_bom(bytes);
    let (source_encoding, content_start) = byte_order_mark.unwrap_or((encoding, 0));
    let content = &bytes[content_start..];
    let text = if byte_order_mark.is_some() {
        decode_after_byte_order_mark(content, source_encoding, content_start)?
    } else {
        decode_strictly(content, source_encoding).map_err(ConversionFailure::DoesNotDecode)?
    };
    let warnings = conversion_warnings(content, &text, source_encoding, content_start)?;
    Ok(Conversion { text, warnings })
}

fn decode_after_byte_order_mark(
    content: &[u8],
    encoding: &'static Encoding,
    content_start: usize,
) -> Result<String, ConversionFailure> {
    if encoding == UTF_8 {
        return String::from_utf8(content.to_vec()).map_err(|error| {
            ConversionFailure::DamagedUtf8 {
                byte_offset: content_start + error.utf8_error().valid_up_to(),
            }
        });
    }
    decode_refusing(content, encoding, is_nul).map_err(|problem| ConversionFailure::DamagedUtf16 {
        byte_offset: content_start + problem.byte_offset(),
    })
}

fn conversion_warnings(
    content: &[u8],
    text: &str,
    encoding: &'static Encoding,
    content_start: usize,
) -> Result<Vec<ConversionWarning>, ConversionFailure> {
    let mut warnings = Vec::new();
    if let Some(difference) = first_round_trip_difference(content, text, encoding) {
        let byte_offset = content_start + difference;
        if !ROUND_TRIP_MAY_DIFFER.contains(&encoding) {
            return Err(ConversionFailure::RoundTripDiffers { byte_offset });
        }
        warnings.push(ConversionWarning::RoundTripDiffers { byte_offset });
    }
    if text.contains(char::REPLACEMENT_CHARACTER) {
        warnings.push(ConversionWarning::ContainsReplacementCharacters);
    }
    Ok(warnings)
}

fn first_round_trip_difference(
    content: &[u8],
    text: &str,
    encoding: &'static Encoding,
) -> Option<usize> {
    let encoded_again = encode_back(text, encoding);
    if encoded_again.as_ref() == content {
        return None;
    }
    let first_differing_byte = content
        .iter()
        .zip(encoded_again.iter())
        .position(|(original, again)| original != again);
    Some(first_differing_byte.unwrap_or(content.len().min(encoded_again.len())))
}

// encoding_rs has no UTF-16 encoder, so UTF-16 text is encoded again by hand.
fn encode_back<'text>(text: &'text str, encoding: &'static Encoding) -> Cow<'text, [u8]> {
    if encoding == UTF_16LE {
        return Cow::Owned(text.encode_utf16().flat_map(u16::to_le_bytes).collect());
    }
    if encoding == UTF_16BE {
        return Cow::Owned(text.encode_utf16().flat_map(u16::to_be_bytes).collect());
    }
    encoding.encode(text).0
}

fn is_nul(character: char) -> bool {
    character == '\0'
}

fn is_nul_or_c1_control(character: char) -> bool {
    is_nul(character) || C1_CONTROL_CHARACTERS.contains(&character)
}

fn decode_refusing(
    bytes: &[u8],
    encoding: &'static Encoding,
    is_refused: fn(char) -> bool,
) -> Result<String, DecodingProblem> {
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut text = String::new();
    feed(&mut decoder, bytes, &mut text, true)
        .map_err(|byte_offset| DecodingProblem::InvalidBytes { byte_offset })?;
    let Some(position) = text.chars().position(is_refused) else {
        return Ok(text);
    };
    Err(DecodingProblem::ControlCharacter {
        byte_offset: byte_offset_of_character(bytes, encoding, position),
    })
}

/// Decodes all of `input`, growing `output` as needed. An invalid sequence stops it and
/// yields the sequence's offset within `input`.
fn feed(decoder: &mut Decoder, input: &[u8], output: &mut String, last: bool) -> Result<(), usize> {
    let mut read_so_far = 0;
    loop {
        output.reserve(input.len() - read_so_far + UTF8_MAXIMUM_CHARACTER_BYTES);
        let (result, read) =
            decoder.decode_to_string_without_replacement(&input[read_so_far..], output, last);
        read_so_far += read;
        match result {
            DecoderResult::InputEmpty => return Ok(()),
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(invalid_length, read_after_invalid) => {
                return Err(read_so_far
                    - usize::from(invalid_length)
                    - usize::from(read_after_invalid));
            }
        }
    }
}

/// Finds where the character at `position` starts in the input by decoding it again one
/// byte at a time. Only runs after a refused character has been found.
fn byte_offset_of_character(bytes: &[u8], encoding: &'static Encoding, position: usize) -> usize {
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut piece = String::new();
    let mut characters_before = 0;
    let mut character_start = 0;
    for (offset, byte) in bytes.iter().enumerate() {
        piece.clear();
        if feed(&mut decoder, slice::from_ref(byte), &mut piece, false).is_err() {
            return offset;
        }
        let completed = piece.chars().count();
        if characters_before + completed > position {
            return character_start;
        }
        characters_before += completed;
        if completed > 0 {
            character_start = offset + 1;
        }
    }
    character_start
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classification::{Classification, classify};
    use crate::test_fixtures::{expected, source};
    use encoding_rs::{
        EUC_KR, ISO_8859_2, ISO_8859_16, SHIFT_JIS, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252,
        WINDOWS_1253, WINDOWS_1256,
    };

    fn utf16(text: &str, to_bytes: fn(u16) -> [u8; 2]) -> Vec<u8> {
        text.encode_utf16().flat_map(to_bytes).collect()
    }

    #[test]
    fn valid_text_decodes_unchanged() {
        assert_eq!(
            decode_strictly("Café".as_bytes(), UTF_8),
            Ok(String::from("Café"))
        );
        assert_eq!(
            decode_strictly(&[b'C', b'a', b'f', 0xE9], WINDOWS_1252),
            Ok(String::from("Café"))
        );
    }

    /// ENC-11.
    #[test]
    fn invalid_bytes_are_reported_where_they_start() {
        assert_eq!(
            decode_strictly(&[b'o', b'k', 0x80, b'!'], EUC_KR),
            Err(DecodingProblem::InvalidBytes { byte_offset: 2 })
        );
        assert_eq!(
            decode_strictly(&[b'o', b'k', 0x82], SHIFT_JIS),
            Err(DecodingProblem::InvalidBytes { byte_offset: 2 })
        );
    }

    /// ENC-11: single-byte decoders report unused bytes as C1 controls, not as errors.
    #[test]
    fn control_characters_are_reported_where_they_start() {
        assert_eq!(
            decode_strictly(&[b'o', b'k', 0x81, b'!'], WINDOWS_1252),
            Err(DecodingProblem::ControlCharacter { byte_offset: 2 })
        );
        assert_eq!(
            decode_strictly(&[b'o', b'k', 0x00, b'!'], WINDOWS_1252),
            Err(DecodingProblem::ControlCharacter { byte_offset: 2 })
        );
        let utf16_with_nul = [b'A', 0x00, 0x00, 0x00];
        assert_eq!(
            decode_strictly(&utf16_with_nul, UTF_16LE),
            Err(DecodingProblem::ControlCharacter { byte_offset: 2 })
        );
        let shift_jis_hiragana_then_c1 = [0x82, 0xB1, 0x82, 0xF1, 0x80];
        assert_eq!(
            decode_strictly(&shift_jis_hiragana_then_c1, SHIFT_JIS),
            Err(DecodingProblem::ControlCharacter { byte_offset: 4 })
        );
    }

    /// ENC-14 and ENC-15: every fixture with expected text converts to exactly that text.
    #[test]
    fn every_fixture_converts_to_its_expected_text() {
        let conversions = [
            ("ascii-only", UTF_8),
            ("utf8-romanian", UTF_8),
            ("utf8-bom-romanian", UTF_8),
            ("utf16le-bom", UTF_8),
            ("utf16le-no-bom", UTF_16LE),
            ("windows-1250-romanian", WINDOWS_1250),
            ("iso-8859-2-romanian", ISO_8859_2),
            ("iso-8859-16-romanian", ISO_8859_16),
            ("short-romanian", WINDOWS_1250),
            ("nonstandard", WINDOWS_1250),
            ("line-endings-mixed", WINDOWS_1250),
            ("markup", WINDOWS_1250),
            ("windows-1252-french", WINDOWS_1252),
            ("windows-1251-russian", WINDOWS_1251),
            ("windows-1253-greek", WINDOWS_1253),
            ("windows-1256-arabic", WINDOWS_1256),
            ("shift-jis-japanese", SHIFT_JIS),
            ("euc-kr-korean", EUC_KR),
        ];
        for (fixture, encoding) in conversions {
            assert_eq!(
                convert(&source(fixture), encoding),
                Ok(Conversion {
                    text: expected(fixture),
                    warnings: Vec::new()
                }),
                "{fixture}"
            );
        }
    }

    /// ENC-04 and ENC-14. The encoding given is ignored, because the mark decides.
    #[test]
    fn source_byte_order_mark_is_not_part_of_the_text() {
        let bytes = source("utf8-bom-romanian");
        let conversion = convert(&bytes, WINDOWS_1252).unwrap();
        assert_eq!(conversion.text.as_bytes(), &bytes["\u{FEFF}".len()..]);
        assert!(!conversion.text.starts_with('\u{FEFF}'));
    }

    /// ENC-06: valid UTF-8 is never decoded as anything else, so garbled text stays as it is.
    #[test]
    fn garbled_text_is_not_repaired() {
        let bytes = source("garbled-romanian");
        assert_eq!(classify(&bytes), Classification::Utf8WithoutByteOrderMark);
        assert_eq!(convert(&bytes, UTF_8).unwrap().text.as_bytes(), bytes);
    }

    /// ENC-14: only a byte-order mark at the very start is dropped.
    #[test]
    fn byte_order_mark_character_inside_the_text_is_kept() {
        let bytes = "\u{FEFF}a\u{FEFF}b".as_bytes();
        assert_eq!(convert(bytes, UTF_8).unwrap().text, "a\u{FEFF}b");
    }

    /// ENC-05.
    #[test]
    fn decodes_utf16_with_bom() {
        assert_eq!(
            convert(&source("utf16le-bom"), UTF_8).unwrap().text,
            expected("utf16le-bom")
        );
        let big_endian = [&[0xFE, 0xFF][..], &utf16("Bună", u16::to_be_bytes)].concat();
        assert_eq!(convert(&big_endian, UTF_8).unwrap().text, "Bună");
    }

    /// ENC-05: a file cut in the middle of a character fails where the cut is.
    #[test]
    fn truncated_utf16_fails_where_it_is_cut() {
        let bytes = source("utf16le-bom");
        let truncated = &bytes[..bytes.len() - 1];
        assert_eq!(
            convert(truncated, UTF_8),
            Err(ConversionFailure::DamagedUtf16 {
                byte_offset: truncated.len() - 1
            })
        );
    }

    /// ENC-04.
    #[test]
    fn damaged_utf8_after_bom_fails_where_it_is_damaged() {
        let bytes = [0xEF, 0xBB, 0xBF, b'O', b'K', 0xC3];
        assert_eq!(
            convert(&bytes, UTF_8),
            Err(ConversionFailure::DamagedUtf8 { byte_offset: 5 })
        );
    }

    /// ENC-05: UTF-32 starts like UTF-16 and decodes to NUL characters.
    #[test]
    fn utf32_is_refused() {
        let utf32_little_endian = [0xFF, 0xFE, 0x00, 0x00, b'H', 0x00, 0x00, 0x00];
        assert_eq!(
            convert(&utf32_little_endian, UTF_8),
            Err(ConversionFailure::DamagedUtf16 { byte_offset: 2 })
        );
    }

    /// ENC-15.
    #[test]
    fn line_endings_are_preserved() {
        let text = convert(&source("line-endings-mixed"), WINDOWS_1250)
            .unwrap()
            .text;
        assert_eq!(text, expected("line-endings-mixed"));
        assert!(text.contains("\r\n") && text.contains("\r4") && text.contains("\n\n"));
    }

    /// ENC-15.
    #[test]
    fn markup_is_preserved() {
        let text = convert(&source("markup"), WINDOWS_1250).unwrap().text;
        assert!(text.contains("<i>„Bună dimineaţa!” – mi-a spus ea…</i>"));
        assert!(text.contains("{\\an8}"));
        assert!(text.contains("<script>"));
    }

    /// ENC-11: a wrong encoding is refused rather than converted into different text.
    #[test]
    fn wrong_encoding_does_not_convert() {
        assert!(matches!(
            convert(&source("markup"), ISO_8859_2),
            Err(ConversionFailure::DoesNotDecode(
                DecodingProblem::ControlCharacter { .. }
            ))
        ));
    }

    /// ENC-17: Shift_JIS 0xED 0x40 decodes to 纊, which the standard encoder writes as
    /// 0xFA 0x5C, so the round trip differs without anything being lost.
    #[test]
    fn round_trip_mismatch_is_a_warning_for_cjk() {
        let bytes = [b'1', b'\n', 0xED, 0x40, b'\n'];
        assert_eq!(
            convert(&bytes, SHIFT_JIS),
            Ok(Conversion {
                text: String::from("1\n纊\n"),
                warnings: vec![ConversionWarning::RoundTripDiffers { byte_offset: 2 }]
            })
        );
    }

    /// ENC-18.
    #[test]
    fn existing_replacement_characters_warn() {
        let bytes = "\u{FEFF}Bun\u{FFFD}!".as_bytes();
        assert_eq!(
            convert(bytes, UTF_8),
            Ok(Conversion {
                text: String::from("Bun\u{FFFD}!"),
                warnings: vec![ConversionWarning::ContainsReplacementCharacters]
            })
        );
    }
}
