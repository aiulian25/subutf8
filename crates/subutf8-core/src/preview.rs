use encoding_rs::Encoding;

use crate::constants::{
    CANDIDATE_SAMPLE_CHARACTERS, PREVIEW_CUES, PREVIEW_LINE_BREAK, PREVIEW_MAXIMUM_CHARACTERS,
};
use crate::decoding::{
    Location, Misreading, Reading, convert_reading, is_nul_or_c1_control, utf8_text,
};
use crate::srt_structure::{Cue, line_range_at, parse_cues};

/// One cue as the preview shows it. Its fields are plain text for the interface to
/// display, never to interpret (UI-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewCue {
    pub number: Option<String>,
    pub timing: Option<String>,
    pub text: String,
    /// A field was longer than the preview limit and was cut.
    pub is_clipped: bool,
}

/// UI-14: the line where decoding stops, as far as it can be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenLine {
    pub line: usize,
    pub text: String,
}

/// UI-04: up to five cues whose text has non-ASCII characters, the ones that show a wrong
/// encoding. A file without any shows its first cues instead.
pub fn preview(text: &str) -> Vec<PreviewCue> {
    let cues = parse_cues(text);
    let revealing: Vec<&Cue> = cues
        .iter()
        .filter(|cue| has_non_ascii_text(cue))
        .take(PREVIEW_CUES)
        .collect();
    let shown = if revealing.is_empty() {
        cues.iter().take(PREVIEW_CUES).collect()
    } else {
        revealing
    };
    shown.into_iter().map(preview_cue).collect()
}

fn has_non_ascii_text(cue: &Cue) -> bool {
    cue.text_lines.iter().any(|line| !line.is_ascii())
}

/// UI-15: the first subtitle line with non-ASCII text, which shows whether an encoding reads
/// correctly; else the first line with any text.
pub(crate) fn sample_line(text: &str) -> String {
    let cues = parse_cues(text);
    let lines = || {
        cues.iter()
            .flat_map(|cue| cue.text_lines.iter())
            .map(|line| line.trim())
    };
    let sample = lines()
        .find(|line| !line.is_ascii())
        .or_else(|| lines().find(|line| !line.is_empty()))
        .unwrap_or_default();
    sample.chars().take(CANDIDATE_SAMPLE_CHARACTERS).collect()
}

fn preview_cue(cue: &Cue) -> PreviewCue {
    let (number, number_is_clipped) = clip_optional(cue.number);
    let (timing, timing_is_clipped) = clip_optional(cue.timing);
    let (text, text_is_clipped) = clip(&cue.text_lines.join(PREVIEW_LINE_BREAK));
    PreviewCue {
        number,
        timing,
        text,
        is_clipped: number_is_clipped || timing_is_clipped || text_is_clipped,
    }
}

fn clip_optional(field: Option<&str>) -> (Option<String>, bool) {
    field.map_or((None, false), |field| {
        let (clipped, is_clipped) = clip(field);
        (Some(clipped), is_clipped)
    })
}

fn clip(field: &str) -> (String, bool) {
    match field.char_indices().nth(PREVIEW_MAXIMUM_CHARACTERS) {
        Some((cut, _)) => (field[..cut].to_owned(), true),
        None => (field.to_owned(), false),
    }
}

/// UI-14: the line of a file holding `location`, decoded with `encoding` as far as it goes.
/// Bytes that do not decode, NUL and C1 controls show as U+FFFD, where the encoding fails.
pub fn broken_line(bytes: &[u8], encoding: &'static Encoding, location: Location) -> BrokenLine {
    let line_bytes = &bytes[line_range_at(bytes, location.byte_offset, encoding)];
    let (decoded, _) = encoding.decode_with_bom_removal(line_bytes);
    let readable: String = decoded.chars().map(visible).collect();
    let (text, _) = clip(&readable);
    BrokenLine {
        line: location.line,
        text,
    }
}

fn visible(character: char) -> char {
    if is_nul_or_c1_control(character) {
        return char::REPLACEMENT_CHARACTER;
    }
    character
}

/// ENC-22: a garbled file's first line of accented text, as it is and repaired (UI-15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairSample {
    /// Control characters show as U+FFFD, as in UI-14.
    pub as_is: String,
    pub repaired: String,
}

/// ENC-22: none when the repair does not fit the file.
pub fn repair_sample(bytes: &[u8], misreading: Misreading) -> Option<RepairSample> {
    let text = utf8_text(bytes)?;
    let repaired = convert_reading(bytes, Reading::RepairMisreading(misreading)).ok()?;
    Some(RepairSample {
        as_is: sample_line(text).chars().map(visible).collect(),
        repaired: sample_line(&repaired.text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoding::{MisreadVia, decode_strictly};
    use crate::test_fixtures::{expected, source};
    use encoding_rs::{UTF_16LE, WINDOWS_1250, WINDOWS_1252};

    fn numbers(cues: &[PreviewCue]) -> Vec<&str> {
        cues.iter()
            .map(|cue| cue.number.as_deref().unwrap_or_default())
            .collect()
    }

    /// UI-04.
    #[test]
    fn preview_shows_cues_that_reveal_the_encoding() {
        let cues = preview(&expected("windows-1250-romanian"));
        assert_eq!(numbers(&cues), ["1", "2", "3", "4", "5"]);
        assert_eq!(
            cues[0],
            PreviewCue {
                number: Some(String::from("1")),
                timing: Some(String::from("00:00:01,000 --> 00:00:03,500")),
                text: String::from("Bună dimineaţa! Ştii ce înseamnă asta?"),
                is_clipped: false,
            }
        );
        assert_eq!(
            cues[2].text,
            "Mâine dimineaţă ne întâlnim la gară,\nlângă chioşcul cu ziare."
        );
    }

    /// UI-04.
    #[test]
    fn ascii_only_cues_are_skipped() {
        let text = "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n\
                    2\n00:00:03,000 --> 00:00:04,000\nBună\n\n\
                    3\n00:00:05,000 --> 00:00:06,000\nBye\n\n\
                    4\n00:00:07,000 --> 00:00:08,000\nŞtiu\n";
        assert_eq!(numbers(&preview(text)), ["2", "4"]);
    }

    /// UI-04.
    #[test]
    fn ascii_text_shows_its_first_cues() {
        assert_eq!(numbers(&preview(&expected("ascii-only"))), ["1", "2", "3"]);
    }

    /// UI-04, and UI-03 for the data: markup comes through as written.
    #[test]
    fn markup_is_kept_as_text() {
        let cues = preview(&expected("markup"));
        assert_eq!(cues[0].text, "<i>„Bună dimineaţa!” – mi-a spus ea…</i>");
    }

    /// UI-04: bare CR line endings still separate lines and cues.
    #[test]
    fn mixed_line_endings_are_read() {
        let cues = preview(&expected("line-endings-mixed"));
        assert_eq!(numbers(&cues), ["1", "2", "3", "4"]);
        assert_eq!(
            cues[2].timing.as_deref(),
            Some("00:00:07,200 --> 00:00:09,900")
        );
    }

    /// UI-04.
    #[test]
    fn long_fields_are_clipped() {
        let long_line = "ă".repeat(PREVIEW_MAXIMUM_CHARACTERS * 2);
        let cues = preview(&format!("1\n00:00:01,000 --> 00:00:02,000\n{long_line}\n"));
        assert!(cues[0].is_clipped);
        assert_eq!(cues[0].text.chars().count(), PREVIEW_MAXIMUM_CHARACTERS);
    }

    /// UI-14: windows-1252 leaves 0x81 unused, so it decodes to a C1 control.
    #[test]
    fn broken_line_shows_where_decoding_stops() {
        let bytes = b"1\r\n00:00:01,000 --> 00:00:02,000\r\nBad \x81 byte\r\n";
        let Err(problem) = decode_strictly(bytes, WINDOWS_1252) else {
            panic!("0x81 decoded in windows-1252");
        };
        assert_eq!(
            broken_line(bytes, WINDOWS_1252, problem.location()),
            BrokenLine {
                line: 3,
                text: String::from("Bad \u{FFFD} byte"),
            }
        );
        let utf32_little_endian = [0xFF, 0xFE, 0x00, 0x00, b'H', 0x00, 0x00, 0x00];
        let location = Location {
            byte_offset: 2,
            line: 1,
        };
        assert_eq!(
            broken_line(&utf32_little_endian, UTF_16LE, location).text,
            "\u{FFFD}H\u{FFFD}"
        );
    }

    /// ENC-22: the same line both ways; a C1 character shows as U+FFFD.
    #[test]
    fn repair_sample_shows_the_first_accented_line_both_ways() {
        let misreading = Misreading {
            via: MisreadVia::Windows1252,
            original: WINDOWS_1250,
        };
        assert_eq!(
            repair_sample(&source("garbled-romanian"), misreading),
            Some(RepairSample {
                as_is: String::from("Bunã dimineaþa! ªtii ce înseamnã asta?"),
                repaired: String::from("Bună dimineaţa! Ştii ce înseamnă asta?"),
            })
        );
        let dash = "1\n00:00:01,000 --> 00:00:02,000\nUn \u{96} doi\n";
        let through_latin1 = Misreading {
            via: MisreadVia::Latin1,
            original: WINDOWS_1252,
        };
        assert_eq!(
            repair_sample(dash.as_bytes(), through_latin1),
            Some(RepairSample {
                as_is: String::from("Un \u{FFFD} doi"),
                repaired: String::from("Un \u{2013} doi"),
            })
        );
    }
}
