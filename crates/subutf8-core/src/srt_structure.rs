use crate::constants::{
    CLOCK_FIELD_DIGITS, CLOCK_SEPARATOR, CRLF, LINE_TERMINATORS, MILLISECONDS_DIGITS,
    MILLISECONDS_SEPARATOR, TIMING_ARROW,
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

pub fn parse_cues(text: &str) -> Vec<Cue<'_>> {
    let numbered_lines: Vec<(usize, &str)> = split_lines(text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| (index + 1, line))
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
        .any(|cue| cue.timing.is_some_and(is_timing_line))
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
        if !cue.timing.is_some_and(is_timing_line) {
            warnings.push(StructureWarning::MalformedTiming {
                line: cue.timing_line(),
            });
        }
    }
    warnings
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

    #[test]
    fn lines_split_at_every_kind_of_line_ending() {
        assert_eq!(split_lines("a\r\nb\nc\rd"), ["a", "b", "c", "d"]);
        assert_eq!(split_lines("a\n\nb\r\n"), ["a", "", "b"]);
        assert_eq!(split_lines("\r\n"), [""]);
        assert!(split_lines("").is_empty());
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

    /// ENC-19: the warnings name the lines, and the file still converts.
    #[test]
    fn nonstandard_structure_warns_only() {
        assert_eq!(
            check_structure(&expected("nonstandard")),
            [
                StructureWarning::MissingCueNumber { line: 5 },
                StructureWarning::MalformedTiming { line: 9 },
                StructureWarning::CueNumberOutOfOrder { line: 13 },
            ]
        );
        assert!(convert(&source("nonstandard"), WINDOWS_1250).is_ok());
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
