use std::ops::RangeInclusive;

const MEBIBYTE: u64 = 1024 * 1024;

/// LIMIT-01.
pub const MAXIMUM_FILE_BYTES: u64 = 16 * MEBIBYTE;

/// LIMIT-05.
pub(crate) const MINIMUM_DETECTION_EVIDENCE_BYTES: usize = 24;

pub(crate) const HUNDRED_PERCENT: usize = 100;
pub(crate) const UTF16_CODE_UNIT_BYTES: usize = 2;
pub(crate) const UTF8_MAXIMUM_CHARACTER_BYTES: usize = 4;

/// ENC-11: control characters that only appear when a file is decoded with the wrong encoding.
pub(crate) const C1_CONTROL_CHARACTERS: RangeInclusive<char> = '\u{80}'..='\u{9F}';

/// ENC-14: every output starts with it, which is how MKVToolNix, Windows and many players
/// recognise UTF-8.
pub const UTF8_BYTE_ORDER_MARK: &str = "\u{FEFF}";
/// ENC-21: a mixed reading is named `UTF-8 + windows-1250`.
pub(crate) const MIXED_READING_SEPARATOR: &str = " + ";
/// ENC-22: a repaired reading is named `windows-1250 (repaired)`.
pub(crate) const REPAIRED_READING_SUFFIX: &str = " (repaired)";
/// ENC-22: text read one byte per character, as ISO-8859-1 defines it.
pub(crate) const LATIN1_NAME: &str = "Latin-1";
/// ENC-23: the cedilla letters that windows-1250 and ISO-8859-2 give, each with the comma
/// letter Romanian writes.
pub(crate) const ROMANIAN_COMMA_LETTERS: [(char, char); 4] =
    [('ş', 'ș'), ('ţ', 'ț'), ('Ş', 'Ș'), ('Ţ', 'Ț')];
/// ENC-23: Romanian's language subtag, with or without a region.
pub(crate) const ROMANIAN_LANGUAGE: &str = "ro";

/// NAME-01.
pub(crate) const SRT_EXTENSION: &str = ".srt";
/// NAME-02.
pub(crate) const LANGUAGE_TAG_SEPARATOR: char = '.';
/// NAME-03 and NAME-06.
pub(crate) const FIRST_OUTPUT_NUMBER: u32 = 1;
/// NAME-07: browsers may send a path in either style.
pub(crate) const DROPPED_NAME_PATH_SEPARATORS: [char; 2] = ['/', '\\'];
/// LIMIT-04: the Linux file-name limit.
pub const MAXIMUM_NAME_BYTES: usize = 255;

/// LIMIT-03.
pub const MAXIMUM_LISTED_FILES: usize = 10_000;
/// SAFE-14: hidden files and folders, such as the `._` files macOS leaves on drives.
pub const HIDDEN_NAME_PREFIX: char = '.';

/// SAFE-06 and SAFE-08: leftovers after a crash are recognisable and safe to delete.
pub(crate) const TEMPORARY_FILE_PREFIX: &str = ".subutf8-";
pub(crate) const TEMPORARY_FILE_SUFFIX: &str = ".tmp";
/// SAFE-09: read and write for owner, group and others; never execute or special bits.
pub(crate) const READ_WRITE_PERMISSION_BITS: u32 = 0o666;
/// SAFE-09: outputs of dropped files, which have no original.
pub(crate) const NEW_FILE_PERMISSIONS: u32 = 0o644;

/// NAME-05: a language subtag and a region subtag, as in `pt-BR`.
pub(crate) const SUBTAG_SEPARATOR: char = '-';
/// NAME-05.
pub(crate) const LANGUAGE_SUBTAG_LETTERS: RangeInclusive<usize> = 2..=3;
/// NAME-05.
pub(crate) const REGION_SUBTAG_LETTERS: usize = 2;
/// NAME-05.
pub(crate) const REGION_SUBTAG_DIGITS: usize = 3;

/// ENC-15: subtitle files mix all three line endings.
pub(crate) const LINE_TERMINATORS: [char; 2] = ['\r', '\n'];
pub(crate) const CRLF: &str = "\r\n";
/// UI-14: line breaks in a file's bytes, before decoding.
pub(crate) const LINE_FEED: u8 = b'\n';
pub(crate) const CARRIAGE_RETURN: u8 = b'\r';
pub(crate) const UTF16_LINE_FEED: u16 = 0x000A;
pub(crate) const UTF16_CARRIAGE_RETURN: u16 = 0x000D;
pub(crate) const FIRST_LINE_NUMBER: usize = 1;

/// ENC-19.
pub(crate) const TIMING_ARROW: &str = "-->";
/// ENC-19.
pub(crate) const CLOCK_SEPARATOR: char = ':';
/// ENC-19.
pub(crate) const MILLISECONDS_SEPARATOR: char = ',';
/// ENC-19: WebVTT writes `00:00:01.000`, and most players accept it in SRT too.
pub(crate) const WEBVTT_MILLISECONDS_SEPARATOR: char = '.';
/// ENC-19: minutes and seconds.
pub(crate) const CLOCK_FIELD_DIGITS: usize = 2;
/// ENC-19.
pub(crate) const MILLISECONDS_DIGITS: usize = 3;

/// UI-15: encodings suggested for a file that needs review.
pub(crate) const CANDIDATE_LIMIT: usize = 6;
/// UI-15: each suggestion's sample line is cut here.
pub(crate) const CANDIDATE_SAMPLE_CHARACTERS: usize = 80;

/// UI-04.
pub(crate) const PREVIEW_CUES: usize = 5;
/// UI-04: longer fields are cut, so one malformed giant line cannot flood the interface.
pub(crate) const PREVIEW_MAXIMUM_CHARACTERS: usize = 400;
pub(crate) const PREVIEW_LINE_BREAK: &str = "\n";

/// ENC-07.
pub(crate) const UTF16_MINIMUM_NUL_PERCENT: usize = 20;
/// ENC-07.
pub(crate) const UTF16_MINIMUM_SAME_PARITY_PERCENT: usize = 90;
/// ENC-08.
pub(crate) const MIXED_UTF8_MINIMUM_VALID_PERCENT: usize = 50;
