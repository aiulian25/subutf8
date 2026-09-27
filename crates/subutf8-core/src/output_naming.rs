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
    let (name, is_already_tagged) = without_language_tag(name, language);
    let takes_number =
        placement == Placement::BesideOriginal && (language.is_none() || is_already_tagged);
    let plain_number = takes_number.then_some(FIRST_OUTPUT_NUMBER);
    let plain = compose(name, plain_number, language, extension);
    let numbered = (FIRST_OUTPUT_NUMBER..)
        .filter(move |number| Some(*number) != plain_number)
        .map(move |number| compose(name, Some(number), language, extension));
    Ok(iter::once(plain).chain(numbered))
}

/// NAME-02: a name that already ends with the chosen tag, in any letter case, keeps it once.
fn without_language_tag<'name>(
    name: &'name str,
    language: Option<&SubtitleLanguage>,
) -> (&'name str, bool) {
    let tagged = language.and_then(|language| {
        let (base, tag) = name.rsplit_once(LANGUAGE_TAG_SEPARATOR)?;
        let is_same_tag = !base.is_empty() && tag.eq_ignore_ascii_case(language.tag());
        is_same_tag.then_some(base)
    });
    match tagged {
        Some(base) => (base, true),
        None => (name, false),
    }
}

/// SAFE-18: whether `candidate` is a name SubUTF8 gives to an output of `original` beside it
/// (NAME-02, NAME-03 and NAME-06). Name parts are compared with their letter case (NAME-04).
pub fn is_output_name(candidate: &str, original: &str) -> bool {
    let (Some((candidate_name, candidate_extension)), Some((original_name, original_extension))) =
        (split_srt_name(candidate), split_srt_name(original))
    else {
        return false;
    };
    if candidate == original || !candidate_extension.eq_ignore_ascii_case(original_extension) {
        return false;
    }
    let is_numbered_or_tagged = candidate_name
        .strip_prefix(original_name)
        .is_some_and(|rest| !rest.is_empty() && is_number_then_tag(rest));
    is_numbered_or_tagged || is_renumbered_tagged_name(candidate_name, original_name)
}

/// What follows the original's name in an output: a number, a language tag, or both.
fn is_number_then_tag(rest: &str) -> bool {
    let tag_start = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());
    let tag = &rest[tag_start..];
    tag.is_empty()
        || tag
            .strip_prefix(LANGUAGE_TAG_SEPARATOR)
            .is_some_and(|tag| SubtitleLanguage::parse(tag).is_ok())
}

/// NAME-02: `Film.ro.srt` converted with `ro` gives `Film1.ro.srt`, `Film2.ro.srt` and so on.
fn is_renumbered_tagged_name(candidate_name: &str, original_name: &str) -> bool {
    let Some((base, tag)) = original_name.rsplit_once(LANGUAGE_TAG_SEPARATOR) else {
        return false;
    };
    let renumbered = candidate_name
        .strip_prefix(base)
        .and_then(|rest| rest.rsplit_once(LANGUAGE_TAG_SEPARATOR));
    let Some((number, candidate_tag)) = renumbered else {
        return false;
    };
    let is_number = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
    is_number && candidate_tag.eq_ignore_ascii_case(tag) && SubtitleLanguage::parse(tag).is_ok()
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
                "Film1.ro.srt",
            ),
            (
                "Film.RO.srt",
                Some("ro"),
                Placement::BesideOriginal,
                "Film1.ro.srt",
            ),
            (
                "Film.ro.srt",
                Some("ro"),
                Placement::OutputFolder,
                "Film.ro.srt",
            ),
            (
                "Film.pt-br.srt",
                Some("pt-BR"),
                Placement::BesideOriginal,
                "Film1.pt-BR.srt",
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
        assert_eq!(
            names("Film.ro.srt", Some("ro"), Placement::BesideOriginal, 3),
            ["Film1.ro.srt", "Film2.ro.srt", "Film3.ro.srt"]
        );
    }

    /// SAFE-18.
    #[test]
    fn outputs_are_recognised_by_name() {
        for (candidate, original) in [
            ("Film1.srt", "Film.srt"),
            ("Film.ro.srt", "Film.srt"),
            ("Film12.ro.srt", "Film.srt"),
            ("Film1.ro.srt", "Film.ro.srt"),
            ("Film.ro1.srt", "Film.ro.srt"),
            ("Film.en.ro.srt", "Film.en.srt"),
            ("Film1.SRT", "Film.srt"),
        ] {
            assert!(
                is_output_name(candidate, original),
                "{candidate} {original}"
            );
        }
        for (candidate, original) in [
            ("Film.srt", "Film.srt"),
            ("Filmx.srt", "Film.srt"),
            ("Film.ro.srt", "Film1.srt"),
            ("Film 2.srt", "Film.srt"),
            ("film1.srt", "Film.srt"),
            ("Film1.txt", "Film.srt"),
            ("Film.ro.en.srt", "Film.srt"),
            ("Film.srt", "Film1.srt"),
        ] {
            assert!(
                !is_output_name(candidate, original),
                "{candidate} {original}"
            );
        }
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
