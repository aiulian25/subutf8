use std::borrow::Cow;
use std::slice;

use encoding_rs::{
    BIG5_INIT, Decoder, DecoderResult, EUC_JP_INIT, EUC_KR_INIT, Encoding, GB18030_INIT, GBK_INIT,
    SHIFT_JIS_INIT, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252,
};

use crate::constants::{
    C1_CONTROL_CHARACTERS, LATIN1_NAME, MIXED_READING_SEPARATOR, REPAIRED_READING_SUFFIX,
    UTF8_BYTE_ORDER_MARK, UTF8_MAXIMUM_CHARACTER_BYTES,
};
use crate::srt_structure::{line_number_at, split_byte_lines};

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

/// Where a problem is: from the start of the file, and its line (UI-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub byte_offset: usize,
    pub line: usize,
}

impl Location {
    /// `encoding` is the one decoding the content: the byte-order mark's, when there is one.
    pub fn in_file(bytes: &[u8], byte_offset: usize, encoding: &'static Encoding) -> Self {
        Self {
            byte_offset,
            line: line_number_at(bytes, byte_offset, encoding),
        }
    }
}

/// Where and why strict decoding failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodingProblem {
    /// A byte sequence the encoding does not define.
    InvalidBytes { location: Location },
    /// A NUL or C1 control character, which means the encoding is wrong (ENC-11).
    ControlCharacter { location: Location },
}

impl DecodingProblem {
    pub fn location(self) -> Location {
        match self {
            Self::InvalidBytes { location } | Self::ControlCharacter { location } => location,
        }
    }
}

/// A problem found by `decode_refusing`, at an offset within the bytes it was given.
enum RawProblem {
    InvalidBytes(usize),
    ControlCharacter(usize),
}

impl RawProblem {
    fn offset(&self) -> usize {
        match self {
            Self::InvalidBytes(offset) | Self::ControlCharacter(offset) => *offset,
        }
    }

    /// The problem at its place in the whole file.
    fn at(self, location: Location) -> DecodingProblem {
        match self {
            Self::InvalidBytes(_) => DecodingProblem::InvalidBytes { location },
            Self::ControlCharacter(_) => DecodingProblem::ControlCharacter { location },
        }
    }
}

/// How a file's bytes become text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// ENC-04 to ENC-18: the whole file in one encoding; a byte-order mark still decides.
    Whole(&'static Encoding),
    /// ENC-21: lines that are valid UTF-8 stay as they are; every other line is decoded with
    /// the encoding.
    Utf8LinesElse(&'static Encoding),
    /// ENC-22: a garbled UTF-8 file turned back into the bytes it was misread from, which are
    /// decoded with their original encoding.
    RepairMisreading(Misreading),
}

impl Reading {
    /// The encoding chosen for the file, for its lines that are not UTF-8, or that a repair
    /// decodes with.
    pub fn encoding(self) -> &'static Encoding {
        match self {
            Self::Whole(encoding) | Self::Utf8LinesElse(encoding) => encoding,
            Self::RepairMisreading(misreading) => misreading.original,
        }
    }

    /// How the reading is shown and recorded: `windows-1250`, `UTF-8 + windows-1250`, or
    /// `windows-1250 (repaired)`.
    pub fn name(self) -> String {
        match self {
            Self::Whole(encoding) => encoding.name().to_owned(),
            Self::Utf8LinesElse(encoding) => {
                [UTF_8.name(), MIXED_READING_SEPARATOR, encoding.name()].concat()
            }
            Self::RepairMisreading(misreading) => {
                [misreading.original.name(), REPAIRED_READING_SUFFIX].concat()
            }
        }
    }
}

/// ENC-22: how garbled text was read before it was saved as UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MisreadVia {
    Windows1252,
    /// One byte per character, which leaves C1 control characters where windows-1252 has
    /// punctuation.
    Latin1,
}

impl MisreadVia {
    pub fn name(self) -> &'static str {
        match self {
            Self::Windows1252 => WINDOWS_1252.name(),
            Self::Latin1 => LATIN1_NAME,
        }
    }
}

/// ENC-22: text in the `original` encoding that was read `via` another one and saved as UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Misreading {
    pub via: MisreadVia,
    pub original: &'static Encoding,
}

/// A converted file. The UTF-8 bytes of `text` are the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub text: String,
    pub warnings: Vec<ConversionWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionWarning {
    /// ENC-17, in the CJK encodings only.
    RoundTripDiffers { location: Location },
    /// ENC-18.
    ContainsReplacementCharacters,
}

/// Why a file cannot be converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionFailure {
    /// ENC-04.
    DamagedUtf8 { location: Location },
    /// ENC-05, including UTF-32 files, which decode to NUL characters.
    DamagedUtf16 { location: Location },
    /// ENC-11.
    DoesNotDecode(DecodingProblem),
    /// ENC-17, outside the CJK encodings.
    RoundTripDiffers { location: Location },
    /// ENC-22: valid UTF-8 without a byte-order mark that holds NUL or C1 control characters,
    /// which subtitles never use.
    ControlCharactersInUtf8 { location: Location },
}

impl ConversionFailure {
    pub fn location(self) -> Location {
        match self {
            Self::DamagedUtf8 { location }
            | Self::DamagedUtf16 { location }
            | Self::RoundTripDiffers { location }
            | Self::ControlCharactersInUtf8 { location } => location,
            Self::DoesNotDecode(problem) => problem.location(),
        }
    }
}

/// ENC-11: decodes a whole file without replacing anything, refusing text with NUL or C1
/// characters.
pub fn decode_strictly(
    bytes: &[u8],
    encoding: &'static Encoding,
) -> Result<String, DecodingProblem> {
    decode_refusing(bytes, encoding, is_nul_or_c1_control).map_err(|problem| {
        let location = Location::in_file(bytes, problem.offset(), encoding);
        problem.at(location)
    })
}

/// Converts a file as `reading` says: whole, line by line for a mixed file (ENC-21), or
/// repaired (ENC-22).
pub fn convert_reading(bytes: &[u8], reading: Reading) -> Result<Conversion, ConversionFailure> {
    match reading {
        Reading::Whole(encoding) => convert(bytes, encoding),
        Reading::Utf8LinesElse(encoding) => convert_mixed(bytes, encoding),
        Reading::RepairMisreading(misreading) => convert_repaired(bytes, misreading),
    }
}

/// A UTF-8 file's bytes after its byte-order mark.
fn utf8_content(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_prefix(UTF8_BYTE_ORDER_MARK.as_bytes())
        .unwrap_or(bytes)
}

/// ENC-22: the text of a valid UTF-8 file after its byte-order mark, NUL and C1 characters
/// included.
pub fn utf8_text(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(utf8_content(bytes)).ok()
}

/// ENC-22: the bytes garbled text was read from: through windows-1252, or through Latin-1,
/// where each character up to U+00FF is one byte. None when a character does not fit.
pub(crate) fn misread_bytes(text: &str, via: MisreadVia) -> Option<Vec<u8>> {
    match via {
        MisreadVia::Windows1252 => {
            let (bytes, _, had_unmappable) = WINDOWS_1252.encode(text);
            (!had_unmappable).then(|| bytes.into_owned())
        }
        MisreadVia::Latin1 => text
            .chars()
            .map(|character| u8::try_from(character).ok())
            .collect(),
    }
}

/// ENC-22: a garbled file's text turned back into the bytes it was misread from, which are
/// decoded strictly with the original encoding (ENC-11) and must encode back to themselves
/// (ENC-17). Each character became one byte, so a problem is placed at the character that
/// became its byte.
fn convert_repaired(bytes: &[u8], misreading: Misreading) -> Result<Conversion, ConversionFailure> {
    let content_start = bytes.len() - utf8_content(bytes).len();
    let text = decode_after_byte_order_mark(bytes, UTF_8, content_start)?;
    let place = |recovered_offset: usize| {
        let character_start = text
            .char_indices()
            .nth(recovered_offset)
            .map_or(text.len(), |(start, _)| start);
        Location::in_file(bytes, content_start + character_start, UTF_8)
    };
    let recovered = misread_bytes(&text, misreading.via).ok_or_else(|| {
        ConversionFailure::DoesNotDecode(DecodingProblem::InvalidBytes { location: place(0) })
    })?;
    let original = misreading.original;
    let repaired =
        decode_refusing(&recovered, original, is_nul_or_c1_control).map_err(|problem| {
            let location = place(problem.offset());
            ConversionFailure::DoesNotDecode(problem.at(location))
        })?;
    let round_trip_difference =
        first_round_trip_difference(&recovered, &repaired, original).map(place);
    let warnings = warnings_for(&repaired, original, round_trip_difference)?;
    Ok(Conversion {
        text: repaired,
        warnings,
    })
}

/// ENC-21: every line that is valid UTF-8 stays as it is; every other line is decoded strictly
/// with `encoding` and must encode back to its own bytes (ENC-17). Lines are counted as the
/// bytes split them.
fn convert_mixed(
    bytes: &[u8],
    encoding: &'static Encoding,
) -> Result<Conversion, ConversionFailure> {
    let content = utf8_content(bytes);
    let mut line_start = bytes.len() - content.len();
    let mut text = String::with_capacity(content.len());
    let mut round_trip_difference = None;
    for line in split_byte_lines(content) {
        let place = |offset: usize| Location::in_file(bytes, line_start + offset, UTF_8);
        let (read, difference) = read_mixed_line(line, encoding).map_err(|problem| {
            let location = place(problem.offset());
            ConversionFailure::DoesNotDecode(problem.at(location))
        })?;
        round_trip_difference = round_trip_difference.or(difference.map(place));
        text.push_str(&read);
        line_start += line.len();
    }
    let warnings = warnings_for(&text, encoding, round_trip_difference)?;
    Ok(Conversion { text, warnings })
}

/// One line of a mixed file, and where its round trip first differs. Offsets count from the
/// line's start.
fn read_mixed_line<'line>(
    line: &'line [u8],
    encoding: &'static Encoding,
) -> Result<(Cow<'line, str>, Option<usize>), RawProblem> {
    if let Ok(text) = std::str::from_utf8(line) {
        return Ok((Cow::Borrowed(text), None));
    }
    let text = decode_refusing(line, encoding, is_nul_or_c1_control)?;
    let difference = first_round_trip_difference(line, &text, encoding);
    Ok((Cow::Owned(text), difference))
}

/// Converts a file into text for UTF-8 output (ENC-04, ENC-05, ENC-11 and ENC-14 to ENC-18).
/// `encoding` applies when the file has no byte-order mark. When it has one, the mark
/// decides, because such a file takes no other encoding (ENC-12).
pub fn convert(bytes: &[u8], encoding: &'static Encoding) -> Result<Conversion, ConversionFailure> {
    let byte_order_mark = Encoding::for_bom(bytes);
    let (source_encoding, content_start) = byte_order_mark.unwrap_or((encoding, 0));
    let text = if byte_order_mark.is_some() {
        decode_after_byte_order_mark(bytes, source_encoding, content_start)?
    } else {
        decode_strictly(bytes, source_encoding)
            .map_err(|problem| decoding_failure(problem, source_encoding))?
    };
    let warnings = conversion_warnings(bytes, &text, source_encoding, content_start)?;
    Ok(Conversion { text, warnings })
}

/// ENC-11, and ENC-22: in valid UTF-8, a control character says nothing about the encoding.
fn decoding_failure(problem: DecodingProblem, encoding: &'static Encoding) -> ConversionFailure {
    match problem {
        DecodingProblem::ControlCharacter { location } if encoding == UTF_8 => {
            ConversionFailure::ControlCharactersInUtf8 { location }
        }
        _ => ConversionFailure::DoesNotDecode(problem),
    }
}

fn decode_after_byte_order_mark(
    bytes: &[u8],
    encoding: &'static Encoding,
    content_start: usize,
) -> Result<String, ConversionFailure> {
    let content = &bytes[content_start..];
    if encoding == UTF_8 {
        return String::from_utf8(content.to_vec()).map_err(|error| {
            let byte_offset = content_start + error.utf8_error().valid_up_to();
            ConversionFailure::DamagedUtf8 {
                location: Location::in_file(bytes, byte_offset, encoding),
            }
        });
    }
    decode_refusing(content, encoding, is_nul).map_err(|problem| {
        let byte_offset = content_start + problem.offset();
        ConversionFailure::DamagedUtf16 {
            location: Location::in_file(bytes, byte_offset, encoding),
        }
    })
}

fn conversion_warnings(
    bytes: &[u8],
    text: &str,
    encoding: &'static Encoding,
    content_start: usize,
) -> Result<Vec<ConversionWarning>, ConversionFailure> {
    let difference = first_round_trip_difference(&bytes[content_start..], text, encoding)
        .map(|difference| Location::in_file(bytes, content_start + difference, encoding));
    warnings_for(text, encoding, difference)
}

/// ENC-17 and ENC-18: a round trip that differs fails the file, except in the encodings that
/// may differ, where it warns; replacement characters in the text warn.
fn warnings_for(
    text: &str,
    encoding: &'static Encoding,
    round_trip_difference: Option<Location>,
) -> Result<Vec<ConversionWarning>, ConversionFailure> {
    let mut warnings = Vec::new();
    if let Some(location) = round_trip_difference {
        if !ROUND_TRIP_MAY_DIFFER.contains(&encoding) {
            return Err(ConversionFailure::RoundTripDiffers { location });
        }
        warnings.push(ConversionWarning::RoundTripDiffers { location });
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

pub(crate) fn is_nul_or_c1_control(character: char) -> bool {
    is_nul(character) || C1_CONTROL_CHARACTERS.contains(&character)
}

fn decode_refusing(
    bytes: &[u8],
    encoding: &'static Encoding,
    is_refused: fn(char) -> bool,
) -> Result<String, RawProblem> {
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut text = String::new();
    feed(&mut decoder, bytes, &mut text, true).map_err(RawProblem::InvalidBytes)?;
    let Some(position) = text.chars().position(is_refused) else {
        return Ok(text);
    };
    Err(RawProblem::ControlCharacter(byte_offset_of_character(
        bytes, encoding, position,
    )))
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

    fn at(byte_offset: usize, line: usize) -> Location {
        Location { byte_offset, line }
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
            Err(DecodingProblem::InvalidBytes { location: at(2, 1) })
        );
        assert_eq!(
            decode_strictly(&[b'o', b'k', b'\n', 0x82], SHIFT_JIS),
            Err(DecodingProblem::InvalidBytes { location: at(3, 2) })
        );
    }

    /// ENC-11: single-byte decoders report unused bytes as C1 controls, not as errors.
    #[test]
    fn control_characters_are_reported_where_they_start() {
        assert_eq!(
            decode_strictly(&[b'o', b'k', 0x81, b'!'], WINDOWS_1252),
            Err(DecodingProblem::ControlCharacter { location: at(2, 1) })
        );
        assert_eq!(
            decode_strictly(&[b'o', b'k', 0x00, b'!'], WINDOWS_1252),
            Err(DecodingProblem::ControlCharacter { location: at(2, 1) })
        );
        let utf16_with_nul = [b'A', 0x00, 0x00, 0x00];
        assert_eq!(
            decode_strictly(&utf16_with_nul, UTF_16LE),
            Err(DecodingProblem::ControlCharacter { location: at(2, 1) })
        );
        let shift_jis_hiragana_then_c1 = [0x82, 0xB1, 0x82, 0xF1, 0x80];
        assert_eq!(
            decode_strictly(&shift_jis_hiragana_then_c1, SHIFT_JIS),
            Err(DecodingProblem::ControlCharacter { location: at(4, 1) })
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

    /// ENC-06: valid UTF-8 read as UTF-8 stays as it is; only a chosen repair (ENC-22) undoes
    /// garbling.
    #[test]
    fn garbled_text_is_not_repaired() {
        let bytes = source("garbled-romanian");
        assert_eq!(classify(&bytes), Classification::Utf8WithoutByteOrderMark);
        assert_eq!(convert(&bytes, UTF_8).unwrap().text.as_bytes(), bytes);
    }

    /// ENC-22: a C1 character in valid UTF-8 fails on its line, not as a wrong encoding.
    #[test]
    fn control_characters_in_utf8_fail_on_their_line() {
        let bytes = "1\nUn \u{96} doi\n".as_bytes();
        assert_eq!(
            convert(bytes, UTF_8),
            Err(ConversionFailure::ControlCharactersInUtf8 { location: at(5, 2) })
        );
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

    /// ENC-05: a file cut in the middle of a character fails where the cut is, on the last of
    /// its 32 lines.
    #[test]
    fn truncated_utf16_fails_where_it_is_cut() {
        let bytes = source("utf16le-bom");
        let truncated = &bytes[..bytes.len() - 1];
        assert_eq!(
            convert(truncated, UTF_8),
            Err(ConversionFailure::DamagedUtf16 {
                location: at(truncated.len() - 1, 32)
            })
        );
    }

    /// ENC-04.
    #[test]
    fn damaged_utf8_after_bom_fails_where_it_is_damaged() {
        let bytes = [0xEF, 0xBB, 0xBF, b'O', b'K', 0xC3];
        assert_eq!(
            convert(&bytes, UTF_8),
            Err(ConversionFailure::DamagedUtf8 { location: at(5, 1) })
        );
    }

    /// ENC-05: UTF-32 starts like UTF-16 and decodes to NUL characters.
    #[test]
    fn utf32_is_refused() {
        let utf32_little_endian = [0xFF, 0xFE, 0x00, 0x00, b'H', 0x00, 0x00, 0x00];
        assert_eq!(
            convert(&utf32_little_endian, UTF_8),
            Err(ConversionFailure::DamagedUtf16 { location: at(2, 1) })
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
                warnings: vec![ConversionWarning::RoundTripDiffers { location: at(2, 2) }]
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

    /// ENC-21: the UTF-8 half stays as it is, and only the windows-1250 half is decoded.
    #[test]
    fn mixed_file_keeps_utf8_lines() {
        let bytes = source("mixed-utf8-1250");
        let reading = Reading::Utf8LinesElse(WINDOWS_1250);
        assert_eq!(
            convert_reading(&bytes, reading),
            Ok(Conversion {
                text: expected("mixed-utf8-1250"),
                warnings: Vec::new()
            })
        );
        assert_eq!(reading.name(), "UTF-8 + windows-1250");
        assert!(convert(&bytes, WINDOWS_1250).is_err());
    }

    /// ENC-21: a line that does not fit the encoding is named in the whole file.
    #[test]
    fn mixed_file_fails_on_the_line_that_does_not_fit() {
        let bytes = ["Bună\n".as_bytes(), &[b'o', b'k', b'\n', 0x81, b'\n']].concat();
        assert_eq!(
            convert_reading(&bytes, Reading::Utf8LinesElse(WINDOWS_1252)),
            Err(ConversionFailure::DoesNotDecode(
                DecodingProblem::ControlCharacter { location: at(9, 3) }
            ))
        );
        assert_eq!(
            convert_reading(&bytes[..9], Reading::Whole(UTF_8)),
            convert(&bytes[..9], UTF_8)
        );
    }
}
