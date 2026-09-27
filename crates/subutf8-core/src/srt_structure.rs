use std::iter;
use std::ops::Range;

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

use crate::constants::{
    CARRIAGE_RETURN, CLOCK_FIELD_DIGITS, CLOCK_SEPARATOR, CRLF, FIRST_LINE_NUMBER, LINE_FEED,
    LINE_TERMINATORS, MILLISECONDS_DIGITS, MILLISECONDS_SEPARATOR, TIMING_ARROW,
    UTF16_CARRIAGE_RETURN, UTF16_CODE_UNIT_BYTES, UTF16_LINE_FEED, WEBVTT_MILLISECONDS_SEPARATOR,
};

/// One block of lines between blank lines, read as a cue without judging it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue<'text> {
    pub number: Option<&'text str>,
    pub timing: Option<&'text str>,
    pub text_lines: Vec<&'text str>,
    first_line: usize,
}

impl Cue<'_> {
    fn timing_line(&self) -> usize {
        self.first_line + usize::from(self.number.is_some())
    }
}

/// ENC-19. Line numbers start at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureWarning {
    /// No timing line anywhere, so nothing reads as a subtitle.
    NoCues,
    MissingCueNumber {
        line: usize,
    },
    /// A cue number that is not greater than the one before.
    CueNumberOutOfOrder {
        line: usize,
    },
    MalformedTiming {
        line: usize,
    },
    /// A timing line that is well formed except for '.' before the milliseconds.
    TimingUsesDot {
        line: usize,
    },
}

/// Splits text at CRLF, LF and CR alike, since subtitle files mix them.
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(end) = rest.find(LINE_TERMINATORS) {
        lines.push(&rest[..end]);
        let terminator = if rest[end..].starts_with(CRLF) {
            CRLF
        } else {
            &rest[end..=end]
        };
        rest = &rest[end + terminator.len()..];
    }
    if !rest.is_empty() {
        lines.push(rest);
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineUnit {
    LineFeed,
    CarriageReturn,
    Other,
}

fn byte_line_unit(unit: &[u8]) -> LineUnit {
    match unit[0] {
        LINE_FEED => LineUnit::LineFeed,
        CARRIAGE_RETURN => LineUnit::CarriageReturn,
        _ => LineUnit::Other,
    }
}

fn utf16_line_unit(unit: u16) -> LineUnit {
    match unit {
        UTF16_LINE_FEED => LineUnit::LineFeed,
        UTF16_CARRIAGE_RETURN => LineUnit::CarriageReturn,
        _ => LineUnit::Other,
    }
}

fn little_endian_line_unit(unit: &[u8]) -> LineUnit {
    utf16_line_unit(u16::from_le_bytes([unit[0], unit[1]]))
}

fn big_endian_line_unit(unit: &[u8]) -> LineUnit {
    utf16_line_unit(u16::from_be_bytes([unit[0], unit[1]]))
}

/// UTF-16 is read in two-byte units, so a 0x0A byte that is half of another character is no
/// line break. The other encodings keep ASCII bytes as they are.
fn unit_reader(encoding: &'static Encoding) -> (usize, fn(&[u8]) -> LineUnit) {
    if encoding == UTF_16LE {
        return (UTF16_CODE_UNIT_BYTES, little_endian_line_unit);
    }
    if encoding == UTF_16BE {
        return (UTF16_CODE_UNIT_BYTES, big_endian_line_unit);
    }
    (size_of::<u8>(), byte_line_unit)
}

/// UI-14: the byte ranges of a file's line breaks, in order, read before decoding. CRLF
/// counts once, as in `split_lines`.
fn line_breaks<'bytes>(
    bytes: &'bytes [u8],
    encoding: &'static Encoding,
) -> impl Iterator<Item = Range<usize>> + 'bytes {
    let (unit_bytes, read_unit) = unit_reader(encoding);
    let mut units = bytes
        .chunks_exact(unit_bytes)
        .map(read_unit)
        .enumerate()
        .peekable();
    iter::from_fn(move || {
        let (index, unit) = units.find(|(_, unit)| *unit != LineUnit::Other)?;
        let is_crlf = unit == LineUnit::CarriageReturn
            && units
                .next_if(|(_, next)| *next == LineUnit::LineFeed)
                .is_some();
        let start = index * unit_bytes;
        let break_units = 1 + usize::from(is_crlf);
        Some(start..start + break_units * unit_bytes)
    })
}

/// UI-14: the line holding the byte at `byte_offset` in a file. A line break belongs to the
/// line it ends.
pub(crate) fn line_number_at(
    bytes: &[u8],
    byte_offset: usize,
    encoding: &'static Encoding,
) -> usize {
    let breaks_before = line_breaks(bytes, encoding)
        .take_while(|line_break| line_break.end <= byte_offset)
        .count();
    FIRST_LINE_NUMBER + breaks_before
}

/// UI-14: the bytes of the line holding `byte_offset`, without its line break.
pub(crate) fn line_range_at(
    bytes: &[u8],
    byte_offset: usize,
    encoding: &'static Encoding,
) -> Range<usize> {
    let mut start = 0;
    for line_break in line_breaks(bytes, encoding) {
        if line_break.end > byte_offset {
            return start..line_break.start;
        }
        start = line_break.end;
    }
    start..bytes.len()
}

/// ENC-21: a file's lines, each with its line break, so that together they give the bytes back.
/// CRLF, LF and CR end a line, as in `split_lines`.
pub(crate) fn split_byte_lines<'bytes>(
    bytes: &'bytes [u8],
) -> impl Iterator<Item = &'bytes [u8]> + 'bytes {
    let mut breaks = line_breaks(bytes, UTF_8);
    let mut start = 0;
    iter::from_fn(move || {
        if start == bytes.len() {
            return None;
        }
        let end = breaks
            .next()
            .map_or(bytes.len(), |line_break| line_break.end);
        let line = &bytes[start..end];
        start = end;
        Some(line)
    })
}

pub fn parse_cues(text: &str) -> Vec<Cue<'_>> {
    let numbered_lines: Vec<(usize, &str)> = split_lines(text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| (index + FIRST_LINE_NUMBER, line))
        .collect();
    numbered_lines
        .split(|(_, line)| line.trim().is_empty())
        .filter(|block| !block.is_empty())
        .map(read_cue)
        .collect()
}

fn read_cue<'text>(block: &[(usize, &'text str)]) -> Cue<'text> {
    let lines: Vec<&str> = block.iter().map(|(_, line)| *line).collect();
    let has_number = is_cue_number(lines[0]);
    let after_number = &lines[usize::from(has_number)..];
    let has_timing = after_number
        .first()
        .is_some_and(|line| line.contains(TIMING_ARROW));
    Cue {
        number: has_number.then(|| lines[0].trim()),
        timing: has_timing.then(|| after_number[0].trim()),
        text_lines: after_number[usize::from(has_timing)..].to_vec(),
        first_line: block[0].0,
    }
}

/// ENC-19: warns about the structure, but never blocks conversion. Gaps in numbering and
/// extra blank lines are accepted.
pub fn check_structure(text: &str) -> Vec<StructureWarning> {
    let cues = parse_cues(text);
    if !cues
        .iter()
        .any(|cue| cue.timing.is_some_and(reads_as_timing_line))
    {
        return vec![StructureWarning::NoCues];
    }
    let mut warnings = Vec::new();
    let mut previous_number: Option<u64> = None;
    for cue in &cues {
        if cue.number.is_none() {
            warnings.push(StructureWarning::MissingCueNumber {
                line: cue.first_line,
            });
        }
        let number = cue.number.and_then(|number| number.parse::<u64>().ok());
        if let (Some(number), Some(previous)) = (number, previous_number)
            && number <= previous
        {
            warnings.push(StructureWarning::CueNumberOutOfOrder {
                line: cue.first_line,
            });
        }
        previous_number = number.or(previous_number);
        warnings.extend(timing_warning(cue));
    }
    warnings
}

fn timing_warning(cue: &Cue) -> Option<StructureWarning> {
    let line = cue.timing_line();
    match cue.timing {
        Some(timing) if is_timing_line(timing) => None,
        Some(timing) if reads_as_timing_line(timing) => {
            Some(StructureWarning::TimingUsesDot { line })
        }
        _ => Some(StructureWarning::MalformedTiming { line }),
    }
}

/// A timing line, also with '.' before the milliseconds.
fn reads_as_timing_line(line: &str) -> bool {
    is_timing_line(&line.replace(
        WEBVTT_MILLISECONDS_SEPARATOR,
        &MILLISECONDS_SEPARATOR.to_string(),
    ))
}

fn is_cue_number(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && trimmed.bytes().all(|byte| byte.is_ascii_digit())
}

/// `00:00:01,000 --> 00:00:03,500`, where hours may have any number of digits and text
/// such as a position may follow the end time.
fn is_timing_line(line: &str) -> bool {
    let Some((start, end)) = line.split_once(TIMING_ARROW) else {
        return false;
    };
    let end = end.split_whitespace().next().unwrap_or_default();
    is_timestamp(start.trim()) && is_timestamp(end)
}

fn is_timestamp(text: &str) -> bool {
    let Some((clock, milliseconds)) = text.split_once(MILLISECONDS_SEPARATOR) else {
        return false;
    };
    let fields: Vec<&str> = clock.split(CLOCK_SEPARATOR).collect();
    let [hours, minutes, seconds] = fields.as_slice() else {
        return false;
    };
    !hours.is_empty()
        && is_digits(hours)
        && has_digits(minutes, CLOCK_FIELD_DIGITS)
        && has_digits(seconds, CLOCK_FIELD_DIGITS)
        && has_digits(milliseconds, MILLISECONDS_DIGITS)
}

fn is_digits(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii_digit())
}

fn has_digits(text: &str, count: usize) -> bool {
    text.len() == count && is_digits(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoding::convert;
    use crate::test_fixtures::{expected, source};
    use encoding_rs::WINDOWS_1250;

    fn utf16_little_endian(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn lines_split_at_every_kind_of_line_ending() {
        assert_eq!(split_lines("a\r\nb\nc\rd"), ["a", "b", "c", "d"]);
        assert_eq!(split_lines("a\n\nb\r\n"), ["a", "", "b"]);
        assert_eq!(split_lines("\r\n"), [""]);
        assert!(split_lines("").is_empty());
    }

    /// UI-14.
    #[test]
    fn line_number_at_counts_every_kind_of_line_break() {
        let bytes = b"a\r\nb\nc\rd";
        for (byte_offset, line) in [(0, 1), (2, 1), (3, 2), (5, 3), (7, 4), (100, 4)] {
            assert_eq!(
                line_number_at(bytes, byte_offset, WINDOWS_1250),
                line,
                "{byte_offset}"
            );
        }
    }

    /// UI-14.
    #[test]
    fn line_number_at_reads_utf16_in_units() {
        assert_eq!(line_number_at(&utf16_little_endian("a\nb"), 4, UTF_16LE), 2);
        let bytes = utf16_little_endian("\u{0A0D}x");
        assert_eq!(&bytes[..2], [0x0D, 0x0A]);
        assert_eq!(line_number_at(&bytes, 2, UTF_16LE), 1);
    }

    /// UI-14.
    #[test]
    fn line_range_at_excludes_the_line_break() {
        let bytes = b"ab\r\ncd";
        assert_eq!(line_range_at(bytes, 4, WINDOWS_1250), 4..6);
        assert_eq!(line_range_at(bytes, 2, WINDOWS_1250), 0..2);
        assert_eq!(line_range_at(b"ab\n", 3, WINDOWS_1250), 3..3);
    }

    /// ENC-21.
    #[test]
    fn byte_lines_join_back() {
        let bytes = b"a\r\nb\nc\rd";
        let lines: Vec<&[u8]> = split_byte_lines(bytes).collect();
        assert_eq!(lines, [&b"a\r\n"[..], b"b\n", b"c\r", b"d"]);
        assert_eq!(lines.concat(), bytes);
        assert_eq!(split_byte_lines(b"a\n").collect::<Vec<_>>(), [b"a\n"]);
        assert_eq!(split_byte_lines(b"").count(), 0);
    }

    /// ENC-19.
    #[test]
    fn standard_subtitles_have_no_warnings() {
        let standard = [
            "ascii-only",
            "utf8-romanian",
            "windows-1250-romanian",
            "windows-1252-french",
            "windows-1251-russian",
            "shift-jis-japanese",
            "short-romanian",
            "line-endings-mixed",
            "markup",
        ];
        for fixture in standard {
            assert_eq!(check_structure(&expected(fixture)), [], "{fixture}");
        }
    }

    /// ENC-19: the warnings name the lines, and the file still converts. Line 9 is
    /// `00:00:07.200 --> 00:00:09.900`.
    #[test]
    fn nonstandard_structure_warns_only() {
        assert_eq!(
            check_structure(&expected("nonstandard")),
            [
                StructureWarning::MissingCueNumber { line: 5 },
                StructureWarning::TimingUsesDot { line: 9 },
                StructureWarning::CueNumberOutOfOrder { line: 13 },
            ]
        );
        assert!(convert(&source("nonstandard"), WINDOWS_1250).is_ok());
    }

    /// ENC-19: '.' before the milliseconds gets its own warning, also when every timing
    /// has it; other forms stay unusual.
    #[test]
    fn dot_timings_get_their_own_warning() {
        assert_eq!(
            check_structure("1\n00:00:01.000 --> 00:00:02.000\nHi\n"),
            [StructureWarning::TimingUsesDot { line: 2 }]
        );
        assert_eq!(
            check_structure(
                "1\n00:00:01,000 --> 00:00:02,000\nHi\n\n2\n00.00.03,000 --> 00:00:04,000\nBye\n"
            ),
            [StructureWarning::MalformedTiming { line: 6 }]
        );
    }

    /// ENC-19.
    #[test]
    fn text_without_timing_lines_has_no_cues() {
        for text in ["", "Just a note.\nNothing else.\n", "\n\n"] {
            assert_eq!(
                check_structure(text),
                [StructureWarning::NoCues],
                "{text:?}"
            );
        }
    }

    /// ENC-19.
    #[test]
    fn timing_lines_are_checked_for_their_form() {
        let accepted = [
            "00:00:01,000 --> 00:00:02,500",
            "0:00:01,000 --> 0:00:02,500",
            "100:00:01,000 --> 100:00:02,500",
            "00:00:01,000 --> 00:00:02,500 X1:10 X2:20 Y1:30 Y2:40",
            "  00:00:01,000  -->  00:00:02,500  ",
        ];
        for line in accepted {
            assert!(is_timing_line(line), "{line:?}");
        }
        let refused = [
            "00:00:01.000 --> 00:00:02.500",
            "00:00:01 --> 00:00:02",
            "00:00:01,000 -> 00:00:02,500",
            "00:00:01,000 --> ",
            "00:0:01,000 --> 00:00:02,500",
            "00:00:01,00 --> 00:00:02,500",
            "aa:bb:cc,ddd --> 00:00:02,500",
        ];
        for line in refused {
            assert!(!is_timing_line(line), "{line:?}");
        }
    }
}
