use crate::constants::{PREVIEW_CUES, PREVIEW_LINE_BREAK, PREVIEW_MAXIMUM_CHARACTERS};
use crate::srt_structure::{Cue, parse_cues};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::expected;

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
}
