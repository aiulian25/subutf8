use std::iter;
use std::path::{Component, Path};

use crate::constants::{
    DROPPED_NAME_PATH_SEPARATORS, FIRST_OUTPUT_NUMBER, LANGUAGE_TAG_SEPARATOR, MAXIMUM_NAME_BYTES,
    SRT_EXTENSION,
};
use crate::language::SubtitleLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    BesideOriginal,
    OutputFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingProblem {
    /// NAME-01.
    NotAnSrtFile,
    /// NAME-07.
    UnusableDroppedName,
    /// NAME-09.
    NameTooLong,
}

/// NAME-01: the name part and the `.srt` extension, keeping their letter case.
pub fn split_srt_name(file_name: &str) -> Option<(&str, &str)> {
    let extension_start = file_name.len().checked_sub(SRT_EXTENSION.len())?;
    if !file_name.is_char_boundary(extension_start) {
        return None;
    }
    let (name, extension) = file_name.split_at(extension_start);
    if name.is_empty() || !extension.eq_ignore_ascii_case(SRT_EXTENSION) {
        return None;
    }
    Some((name, extension))
}

/// NAME-02 to NAME-04 and NAME-06: output names in the order to try. The first is the
/// plain output name; the rest carry the lowest free numbers, for the rename setting.
pub fn candidate_names<'input>(
    original: &'input str,
    language: Option<&'input SubtitleLanguage>,
    placement: Placement,
) -> Result<impl Iterator<Item = String> + 'input, NamingProblem> {
    let (name, extension) = split_srt_name(original).ok_or(NamingProblem::NotAnSrtFile)?;
    let takes_number = language.is_none() && placement == Placement::BesideOriginal;
    let plain_number = takes_number.then_some(FIRST_OUTPUT_NUMBER);
    let plain = compose(name, plain_number, language, extension);
    let numbered = (FIRST_OUTPUT_NUMBER..)
        .filter(move |number| Some(*number) != plain_number)
        .map(move |number| compose(name, Some(number), language, extension));
    Ok(iter::once(plain).chain(numbered))
}

fn compose(
    name: &str,
    number: Option<u32>,
    language: Option<&SubtitleLanguage>,
    extension: &str,
) -> String {
    let mut composed = String::from(name);
    if let Some(number) = number {
        composed.push_str(&number.to_string());
    }
    if let Some(language) = language {
        composed.push(LANGUAGE_TAG_SEPARATOR);
        composed.push_str(language.tag());
    }
    composed.push_str(extension);
    composed
}

/// NAME-09.
pub fn check_name_length(name: &str) -> Result<(), NamingProblem> {
    if name.len() > MAXIMUM_NAME_BYTES {
        return Err(NamingProblem::NameTooLong);
    }
    Ok(())
}

/// NAME-07: keeps the part after the last `/` or `\`, and refuses names that are then
/// empty, `.` or `..`, or contain control characters.
pub fn dropped_file_name(raw: &str) -> Result<&str, NamingProblem> {
    let name = raw
        .rsplit(DROPPED_NAME_PATH_SEPARATORS)
        .next()
        .unwrap_or_default();
    let components: Vec<Component> = Path::new(name).components().collect();
    let is_plain_name = matches!(components.as_slice(), [Component::Normal(_)]);
    if !is_plain_name || name.chars().any(char::is_control) {
        return Err(NamingProblem::UnusableDroppedName);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language(tag: &str) -> SubtitleLanguage {
        SubtitleLanguage::parse(tag).unwrap()
    }

    fn names(original: &str, tag: Option<&str>, placement: Placement, count: usize) -> Vec<String> {
        let language = tag.map(language);
        candidate_names(original, language.as_ref(), placement)
            .unwrap()
            .take(count)
            .collect()
    }

    /// NAME-02 to NAME-04: the examples in the behaviour document, first name only.
    #[test]
    fn output_names_follow_the_examples() {
        let examples = [
            (
                "Film.srt",
                Some("ro"),
                Placement::BesideOriginal,
                "Film.ro.srt",
            ),
            ("Film.srt", None, Placement::BesideOriginal, "Film1.srt"),
            ("Film.srt", None, Placement::OutputFolder, "Film.srt"),
            (
                "Film.en.srt",
                Some("ro"),
                Placement::BesideOriginal,
                "Film.en.ro.srt",
            ),
            (
                "Film.en.srt",
                Some("ro"),
                Placement::OutputFolder,
                "Film.en.ro.srt",
            ),
            (
                "Film.ro.srt",
                Some("ro"),
                Placement::BesideOriginal,
                "Film.ro.ro.srt",
            ),
            (
                "Film.SRT",
                Some("ro"),
                Placement::BesideOriginal,
                "Film.ro.SRT",
            ),
            (
                "Film.srt",
                Some("pt-br"),
                Placement::OutputFolder,
                "Film.pt-BR.srt",
            ),
        ];
        for (original, tag, placement, expected) in examples {
            assert_eq!(names(original, tag, placement, 1), [expected], "{original}");
        }
    }

    /// NAME-06: the rename setting takes the lowest free number, straight after the name.
    #[test]
    fn rename_setting_numbers_after_the_name() {
        assert_eq!(
            names("Film.srt", Some("ro"), Placement::BesideOriginal, 3),
            ["Film.ro.srt", "Film1.ro.srt", "Film2.ro.srt"]
        );
        assert_eq!(
            names("Film.srt", None, Placement::BesideOriginal, 3),
            ["Film1.srt", "Film2.srt", "Film3.srt"]
        );
        assert_eq!(
            names("Film.srt", None, Placement::OutputFolder, 3),
            ["Film.srt", "Film1.srt", "Film2.srt"]
        );
    }

    /// NAME-02 and NAME-03: beside the original, no candidate can take its name.
    #[test]
    fn no_output_beside_the_original_takes_its_name() {
        for original in ["Film.srt", "Film.ro.srt", "Film.SRT", "a.srt", "1.srt"] {
            for tag in [None, Some("ro"), Some("en")] {
                let candidates = names(original, tag, Placement::BesideOriginal, 50);
                assert!(
                    !candidates.iter().any(|name| name == original),
                    "{original}"
                );
            }
        }
    }

    /// NAME-01.
    #[test]
    fn only_srt_files_are_named() {
        for original in ["Film.txt", "Film.srt.bak", ".srt", "srt", "", "Film.sub"] {
            assert!(
                candidate_names(original, None, Placement::OutputFolder).is_err(),
                "{original:?}"
            );
        }
        assert_eq!(split_srt_name("Film.Srt"), Some(("Film", ".Srt")));
    }

    /// NAME-09 and LIMIT-04.
    #[test]
    fn overlong_names_are_refused() {
        let at_limit = format!(
            "{}.srt",
            "a".repeat(MAXIMUM_NAME_BYTES - SRT_EXTENSION.len())
        );
        assert_eq!(check_name_length(&at_limit), Ok(()));
        assert_eq!(
            check_name_length(&format!("a{at_limit}")),
            Err(NamingProblem::NameTooLong)
        );
    }

    /// NAME-07.
    #[test]
    fn upload_name_is_reduced_to_base_name() {
        let reduced = [
            ("Film.srt", "Film.srt"),
            ("../../etc/Film.srt", "Film.srt"),
            ("C:\\Users\\ana\\Film.srt", "Film.srt"),
            ("/home/ana/Film.ro.srt", "Film.ro.srt"),
        ];
        for (raw, name) in reduced {
            assert_eq!(dropped_file_name(raw), Ok(name), "{raw}");
        }
        for raw in ["", "..", ".", "a/", "a\\", "Fi\u{7}lm.srt", "a\nb.srt"] {
            assert_eq!(
                dropped_file_name(raw),
                Err(NamingProblem::UnusableDroppedName),
                "{raw:?}"
            );
        }
    }
}
