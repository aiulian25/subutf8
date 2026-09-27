use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use encoding_rs::{Encoding, UTF_8};
use subutf8_core::batch::{
    BatchSettings, CollisionPolicy, ConversionJob, Destination, Origin, run_batch,
};
use subutf8_core::classification::{Classification, classify};
use subutf8_core::constants::{MAXIMUM_FILE_BYTES, MAXIMUM_LISTED_FILES};
use subutf8_core::decoding::{
    ConversionFailure, Location, Misreading, Reading, convert_reading, decode_strictly, utf8_text,
};
use subutf8_core::detection::{
    Detection, ManualChoiceRefusal, ReviewReason, check_manual_reading, detect, find_misreading,
    takes_manual_choice,
};
use subutf8_core::input_scan::{
    AllowedArea, ListedFile, Scan, SkippedPath, check_chosen_file, scan_folder,
};
use subutf8_core::language::{SubtitleLanguage, written_text};
use subutf8_core::output_naming::{
    is_output_name, output_language, readable_name, split_srt_name_bytes,
};
use subutf8_core::preview::{
    BrokenLine, PreviewCue, RepairSample, broken_line, preview, repair_sample,
};
use subutf8_core::report::Outcome;
use subutf8_core::source_file::{ReadError, read_source};

use crate::clock;
use crate::constants::{FOLDER_AGREEMENT_MINIMUM_FILES, MAXIMUM_DROPPED_BYTES};

/// Why a file could not even be read when it was added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadProblem {
    Unreadable(io::ErrorKind),
    TooLarge,
    NotRegularFile,
    SymbolicLink,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub id: u64,
    pub origin: Origin,
    pub classification: Result<Classification, ReadProblem>,
    pub detection: Option<Detection>,
    /// SAFE-18: the name of the file beside it that this one is SubUTF8's output of.
    pub copy_of: Option<String>,
    pub folder_agreement: Option<FolderAgreement>,
    /// ENC-22: how a UTF-8 file was garbled, when it looks garbled.
    pub misreading: Option<Misreading>,
    /// ENC-12, ENC-21 and ENC-22: the reading chosen by hand.
    pub chosen_reading: Option<Reading>,
    /// UI-16: this file's own subtitle language, in place of the saved one.
    pub language: Option<SubtitleLanguage>,
    pub outcome: Option<Outcome>,
}

/// ENC-20: the one encoding that other files of a folder were detected with, and how many.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderAgreement {
    pub encoding: &'static Encoding,
    pub files: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCause {
    Detection(ReviewReason),
    LooksLikeUtf16,
    DamagedOrMixedUtf8,
    /// ENC-20: too little evidence alone, but the folder agrees on an encoding that fits.
    FolderAgrees(FolderAgreement),
    /// ENC-22: valid UTF-8 whose text turns back into the bytes of another encoding.
    LooksGarbled(Misreading),
}

/// Why a file cannot be converted at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Empty,
    NotText,
    DamagedUtf8 {
        location: Location,
    },
    Read(ReadProblem),
    /// NAME-11: the name is not UTF-8 and does not read in the encoding the file would be
    /// converted with.
    NameNotDecodable,
}

/// UI-02: a file's status, worked out from what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus<'entry> {
    Ready(Reading),
    NeedsReview {
        suggestion: Option<&'static Encoding>,
        cause: ReviewCause,
    },
    Finished(&'entry Outcome),
    Refused(Refusal),
    /// SAFE-18: SubUTF8's own output of the named file, left alone.
    ConvertedCopy(&'entry str),
}

impl FileEntry {
    pub fn status(&self) -> FileStatus<'_> {
        match &self.outcome {
            Some(outcome) => FileStatus::Finished(outcome),
            None => self.waiting_status(),
        }
    }

    /// What converting the file now would do, whatever happened in an earlier run.
    fn waiting_status(&self) -> FileStatus<'_> {
        let status = waiting_status(
            self.classification,
            self.detection,
            self.folder_agreement,
            self.misreading,
            self.copy_of.as_deref(),
            self.chosen_reading,
        );
        with_readable_name(status, &self.origin)
    }

    /// UI-05: a converted file is done; a skipped or failed one is tried again, for example
    /// after the collision setting changes or the folder becomes writable.
    fn is_retryable(&self) -> bool {
        !matches!(self.outcome, Some(Outcome::Converted { .. }))
    }

    /// The reading the next conversion uses for this file, when it includes the file.
    pub fn pending_reading(&self) -> Option<Reading> {
        if !self.is_retryable() {
            return None;
        }
        match self.waiting_status() {
            FileStatus::Ready(reading) => Some(reading),
            _ => None,
        }
    }

    /// ENC-12: a hand-chosen encoding applies to any file not yet converted.
    pub fn can_choose_encoding(&self) -> bool {
        self.is_retryable() && self.classification.is_ok_and(takes_manual_choice)
    }

    /// ENC-22: a garbled file can be repaired, or kept as it is, until it is converted.
    pub fn can_repair(&self) -> bool {
        self.is_retryable() && self.misreading.is_some()
    }

    /// ENC-13 and ENC-22: detection, and whether a UTF-8 file looks garbled, follow the
    /// subtitle language.
    pub fn depends_on_language(&self) -> bool {
        self.detection.is_some() || self.classification.is_ok_and(is_valid_utf8)
    }

    /// Adding a listed file again reads it afresh and makes it ready to convert again.
    fn refresh(&mut self, prepared: PreparedFile) {
        self.origin = prepared.origin;
        self.classification = prepared.classification;
        self.detection = prepared.detection;
        self.copy_of = prepared.copy_of;
        self.folder_agreement = prepared.folder_agreement;
        self.misreading = prepared.misreading;
        self.chosen_reading = None;
        self.language = None;
        self.outcome = None;
    }

    /// UI-16: the file's own subtitle language, or else the saved one.
    pub fn effective_language<'entry>(
        &'entry self,
        saved: Option<&'entry SubtitleLanguage>,
    ) -> Option<&'entry SubtitleLanguage> {
        self.language.as_ref().or(saved)
    }

    /// UI-04: the reading the preview decodes with, also after the file is converted. A mixed
    /// file that needs review keeps its UTF-8 lines, as the page offers by default (ENC-21), and
    /// a garbled one shows as it is (ENC-22).
    pub fn current_reading(&self) -> Option<Reading> {
        let classification = self.classification.ok()?;
        if self.chosen_reading.is_some() {
            return self.chosen_reading;
        }
        let is_mixed = classification == Classification::DamagedOrMixedUtf8;
        let status = status_of(
            classification,
            self.detection,
            self.folder_agreement,
            self.misreading,
        );
        match status {
            FileStatus::Ready(reading) => Some(reading),
            FileStatus::NeedsReview { suggestion, .. } if is_mixed => {
                suggestion.map(Reading::Utf8LinesElse)
            }
            FileStatus::NeedsReview { suggestion, .. } => suggestion.map(Reading::Whole),
            FileStatus::Finished(_) | FileStatus::Refused(_) | FileStatus::ConvertedCopy(_) => None,
        }
    }

    pub fn is_dropped(&self) -> bool {
        matches!(self.origin, Origin::Dropped { .. })
    }

    /// NAME-11: a name that is not UTF-8 is read with the file's current encoding, and else
    /// shown as far as it reads.
    pub fn display_name(&self) -> String {
        if let Some(name) = readable_origin_name(&self.origin, self.current_reading()) {
            return name;
        }
        match &self.origin {
            Origin::Disk { path, .. } => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Origin::Dropped { name, .. } => name.clone(),
        }
    }
}

/// NAME-11: the name a file is shown and its outputs are named by.
fn readable_origin_name(origin: &Origin, reading: Option<Reading>) -> Option<String> {
    let path = match origin {
        Origin::Dropped { name, .. } => return Some(name.clone()),
        Origin::Disk { path, .. } => path,
    };
    let name = path.file_name()?;
    match reading {
        Some(reading) => readable_name(name, reading.encoding()),
        None => name.to_str().map(str::to_owned),
    }
}

/// NAME-11: a file whose name does not read in the encoding it would be converted with is left
/// alone.
fn with_readable_name<'entry>(status: FileStatus<'entry>, origin: &Origin) -> FileStatus<'entry> {
    let is_unreadable = match &status {
        FileStatus::Ready(reading) => readable_origin_name(origin, Some(*reading)).is_none(),
        _ => false,
    };
    if is_unreadable {
        return FileStatus::Refused(Refusal::NameNotDecodable);
    }
    status
}

/// A hand-chosen reading is only ever set on a file that takes one (ENC-12) or looks garbled
/// (ENC-22), and reading the file afresh clears it.
fn waiting_status<'entry>(
    classification: Result<Classification, ReadProblem>,
    detection: Option<Detection>,
    folder_agreement: Option<FolderAgreement>,
    misreading: Option<Misreading>,
    copy_of: Option<&'entry str>,
    chosen_reading: Option<Reading>,
) -> FileStatus<'entry> {
    let classification = match classification {
        Ok(classification) => classification,
        Err(problem) => return FileStatus::Refused(Refusal::Read(problem)),
    };
    if let Some(original) = copy_of {
        return FileStatus::ConvertedCopy(original);
    }
    if let Some(reading) = chosen_reading {
        return FileStatus::Ready(reading);
    }
    status_of(classification, detection, folder_agreement, misreading)
}

fn status_of(
    classification: Classification,
    detection: Option<Detection>,
    folder_agreement: Option<FolderAgreement>,
    misreading: Option<Misreading>,
) -> FileStatus<'static> {
    let suggestion = detection.map(|detection| match detection {
        Detection::Certain(encoding)
        | Detection::NeedsReview {
            suggestion: encoding,
            ..
        } => encoding,
    });
    match classification {
        Classification::Empty => FileStatus::Refused(Refusal::Empty),
        Classification::Utf8WithByteOrderMark | Classification::Utf8WithoutByteOrderMark => {
            utf8_status(misreading)
        }
        Classification::DamagedUtf8WithByteOrderMark { location } => {
            FileStatus::Refused(Refusal::DamagedUtf8 { location })
        }
        Classification::Utf16WithByteOrderMark(encoding) => {
            FileStatus::Ready(Reading::Whole(encoding))
        }
        Classification::NotText => FileStatus::Refused(Refusal::NotText),
        Classification::LooksLikeUtf16WithoutByteOrderMark(encoding) => FileStatus::NeedsReview {
            suggestion: Some(encoding),
            cause: ReviewCause::LooksLikeUtf16,
        },
        Classification::DamagedOrMixedUtf8 => FileStatus::NeedsReview {
            suggestion,
            cause: ReviewCause::DamagedOrMixedUtf8,
        },
        Classification::NeedsDetection => match (detection, folder_agreement) {
            (Some(Detection::Certain(encoding)), _) => FileStatus::Ready(Reading::Whole(encoding)),
            (Some(detection), Some(agreement)) if has_too_little_evidence(Some(detection)) => {
                FileStatus::NeedsReview {
                    suggestion: Some(agreement.encoding),
                    cause: ReviewCause::FolderAgrees(agreement),
                }
            }
            (Some(Detection::NeedsReview { suggestion, reason }), _) => FileStatus::NeedsReview {
                suggestion: Some(suggestion),
                cause: ReviewCause::Detection(reason),
            },
            (None, _) => FileStatus::NeedsReview {
                suggestion: None,
                cause: ReviewCause::Detection(ReviewReason::GuessDoesNotDecode),
            },
        },
    }
}

/// ENC-06 and ENC-22: valid UTF-8 is Ready as it is, unless it looks garbled; it then shows as
/// it is until a person repairs it or keeps it.
fn utf8_status(misreading: Option<Misreading>) -> FileStatus<'static> {
    let Some(misreading) = misreading else {
        return FileStatus::Ready(Reading::Whole(UTF_8));
    };
    FileStatus::NeedsReview {
        suggestion: Some(UTF_8),
        cause: ReviewCause::LooksGarbled(misreading),
    }
}

fn is_valid_utf8(classification: Classification) -> bool {
    matches!(
        classification,
        Classification::Utf8WithByteOrderMark | Classification::Utf8WithoutByteOrderMark
    )
}

fn has_too_little_evidence(detection: Option<Detection>) -> bool {
    matches!(
        detection,
        Some(Detection::NeedsReview {
            reason: ReviewReason::TooLittleEvidence,
            ..
        })
    )
}

/// SET-01: the saved settings the list converts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSettings {
    pub language: Option<SubtitleLanguage>,
    pub destination: Destination,
    pub output_folder: PathBuf,
    /// NAME-10.
    pub organise_by_day: bool,
    pub collision_policy: CollisionPolicy,
    /// ENC-23.
    pub romanian_comma_letters: bool,
}

impl SessionSettings {
    /// NAME-10: the day is the day the conversion starts.
    pub fn batch_settings(&self, collision_policy: CollisionPolicy) -> BatchSettings {
        BatchSettings {
            destination: self.destination,
            output_folder: self.output_folder.clone(),
            day_folder: self.organise_by_day.then(clock::today),
            collision_policy,
            romanian_comma_letters: self.romanian_comma_letters,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunningConversion {
    pub finished: usize,
    pub total: usize,
    pub file_ids: Vec<u64>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionProblem {
    UnknownFile,
    ConversionRunning,
    /// LIMIT-02.
    TooMuchDropped,
    /// LIMIT-03.
    ListFull,
    ManualChoice(ManualChoiceRefusal),
}

/// Everything a conversion needs, taken from the session when it starts, so the batch runs
/// without holding the session lock.
#[derive(Debug)]
pub struct ConversionStart {
    jobs: Vec<ConversionJob>,
    settings: BatchSettings,
    listed_paths: HashSet<PathBuf>,
    cancel: Arc<AtomicBool>,
}

impl ConversionStart {
    /// `on_outcome` receives the number of files finished so far, and the latest file and
    /// outcome.
    pub fn run(&self, mut on_outcome: impl FnMut(usize, &ConversionJob, &Outcome)) {
        run_batch(
            &self.jobs,
            &self.settings,
            &self.listed_paths,
            &self.cancel,
            |progress, outcome| {
                let job = &self.jobs[progress.finished - 1];
                on_outcome(progress.finished, job, outcome);
            },
        );
    }
}

/// ACCESS-08: one file list per running app, shared by every tab.
#[derive(Debug)]
pub struct Session {
    pub files: Vec<FileEntry>,
    pub settings: SessionSettings,
    pub conversion: Option<RunningConversion>,
    next_id: u64,
}

/// A file read and classified outside the session lock, ready to be listed.
#[derive(Debug)]
pub struct PreparedFile {
    pub origin: Origin,
    classification: Result<Classification, ReadProblem>,
    detection: Option<Detection>,
    copy_of: Option<String>,
    folder_agreement: Option<FolderAgreement>,
    misreading: Option<Misreading>,
}

impl PreparedFile {
    /// What converting the file would do, before it is listed.
    pub fn status(&self) -> FileStatus<'_> {
        let status = waiting_status(
            self.classification,
            self.detection,
            self.folder_agreement,
            self.misreading,
            self.copy_of.as_deref(),
            None,
        );
        with_readable_name(status, &self.origin)
    }
}

/// Reads, classifies and detects a file before it is listed (ENC-01 to ENC-10, ENC-22,
/// SAFE-18).
pub fn prepare(origin: Origin, language: Option<&SubtitleLanguage>) -> PreparedFile {
    let bytes = origin_bytes(&origin);
    let classification = bytes.as_deref().map(classify).map_err(|problem| *problem);
    let (detection, misreading) = match (&bytes, classification) {
        (Ok(bytes), Ok(classification)) => (
            detection_for(bytes, classification, language),
            misreading_for(bytes, classification, language),
        ),
        _ => (None, None),
    };
    let copy_of = match (&origin, &bytes, classification) {
        (Origin::Disk { path, .. }, Ok(bytes), Ok(classification))
            if is_valid_utf8(classification) =>
        {
            converted_copy_of(path, bytes, language)
        }
        _ => None,
    };
    PreparedFile {
        origin,
        classification,
        detection,
        copy_of,
        folder_agreement: None,
        misreading,
    }
}

fn detection_for(
    bytes: &[u8],
    classification: Classification,
    language: Option<&SubtitleLanguage>,
) -> Option<Detection> {
    let needs_detection = matches!(
        classification,
        Classification::NeedsDetection | Classification::DamagedOrMixedUtf8
    );
    needs_detection.then(|| detect(bytes, language))
}

/// ENC-22: whether a valid UTF-8 file looks garbled.
fn misreading_for(
    bytes: &[u8],
    classification: Classification,
    language: Option<&SubtitleLanguage>,
) -> Option<Misreading> {
    if !is_valid_utf8(classification) {
        return None;
    }
    find_misreading(utf8_text(bytes)?, language)
}

/// SAFE-18: a UTF-8 file is SubUTF8's own output when a `.srt` beside it has a name it would
/// give that file's output, and converting that file gives exactly this text. Returns the
/// original's name.
fn converted_copy_of(
    path: &Path,
    bytes: &[u8],
    language: Option<&SubtitleLanguage>,
) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let text = utf8_text(bytes)?;
    let folder = path.parent()?;
    let tagged = output_language(name);
    fs::read_dir(folder).ok()?.flatten().find_map(|entry| {
        let original = folder.join(entry.file_name());
        output_of(&original, name, text, language, tagged.as_ref())
    })
}

/// The original's readable name when `name` and `text` are one of its outputs. A garbled
/// original may have been repaired or kept as it is (ENC-22), an output tagged as Romanian may
/// have taken the comma letters (ENC-23), and a name that is not UTF-8 is read with the
/// original's encoding (NAME-11).
fn output_of(
    original: &Path,
    name: &str,
    text: &str,
    language: Option<&SubtitleLanguage>,
    tagged: Option<&SubtitleLanguage>,
) -> Option<String> {
    let original_name = original.file_name()?;
    let may_be_named_after = match original_name.to_str() {
        Some(original_name) => is_output_name(name, original_name),
        None => split_srt_name_bytes(original_name.as_bytes()).is_some(),
    };
    if !may_be_named_after {
        return None;
    }
    let bytes = read_source(original).ok()?;
    let classification = classify(&bytes);
    let detection = detection_for(&bytes, classification, language);
    let misreading = misreading_for(&bytes, classification, language);
    let readings = match status_of(classification, detection, None, misreading) {
        FileStatus::Ready(reading) => vec![reading],
        FileStatus::NeedsReview {
            cause: ReviewCause::LooksGarbled(misreading),
            ..
        } => vec![Reading::Whole(UTF_8), Reading::RepairMisreading(misreading)],
        _ => return None,
    };
    readings.into_iter().find_map(|reading| {
        let original_name = readable_name(original_name, reading.encoding())?;
        let is_output = is_output_name(name, &original_name)
            && convert_reading(&bytes, reading).is_ok_and(|conversion| {
                conversion.text == text || written_text(&conversion.text, tagged, true) == text
            });
        is_output.then_some(original_name)
    })
}

pub fn prepare_listed(file: ListedFile, language: Option<&SubtitleLanguage>) -> PreparedFile {
    prepare(
        Origin::Disk {
            path: file.path,
            relative_folder: file.relative_folder,
        },
        language,
    )
}

/// Files and folders chosen in the Browse view or passed by "Open with", ready to be listed.
#[derive(Debug, Default)]
pub struct Gathered {
    pub prepared: Vec<PreparedFile>,
    pub skipped: Vec<SkippedPath>,
    pub limit_reached: bool,
}

/// SAFE-11 to SAFE-14 and LIMIT-03: checks each chosen path, scans chosen folders, and reads
/// and classifies what is found. `room` is how many more files the list can take.
pub fn gather(
    paths: &[PathBuf],
    include_subfolders: bool,
    area: &AllowedArea,
    room: usize,
    language: Option<&SubtitleLanguage>,
) -> Gathered {
    let mut listed = Vec::new();
    let mut gathered = Gathered::default();
    for path in paths {
        let room_left = room.saturating_sub(listed.len());
        if room_left == 0 {
            gathered.limit_reached = true;
            break;
        }
        let found = if path.is_dir() {
            scan_folder(path, include_subfolders, area, room_left)
        } else {
            check_chosen_file(path, area).map(|file| Scan {
                files: vec![file],
                ..Scan::default()
            })
        };
        match found {
            Ok(scan) => {
                listed.extend(scan.files);
                gathered.skipped.extend(scan.skipped);
                gathered.limit_reached |= scan.limit_reached;
            }
            Err(reason) => gathered.skipped.push(SkippedPath {
                path: path.clone(),
                reason,
            }),
        }
    }
    gathered.prepared = listed
        .into_iter()
        .map(|file| prepare_listed(file, language))
        .collect();
    let files: Vec<(&Origin, Option<Detection>)> = gathered
        .prepared
        .iter()
        .map(|file| (&file.origin, file.detection))
        .collect();
    let agreements = folder_agreements(&files);
    for (file, agreement) in gathered.prepared.iter_mut().zip(agreements) {
        file.folder_agreement = agreement;
    }
    gathered
}

/// What detecting a file again found.
#[derive(Debug, Clone, Copy)]
pub struct Redetection {
    id: u64,
    detection: Option<Detection>,
    misreading: Option<Misreading>,
    folder_agreement: Option<FolderAgreement>,
}

/// ENC-13, ENC-20 and ENC-22: detection runs again when a subtitle language changes, each file
/// with the language it goes by (UI-16), and so do the agreement within each folder and whether
/// a UTF-8 file looks garbled.
pub fn detect_again(files: Vec<(u64, Origin, Option<SubtitleLanguage>)>) -> Vec<Redetection> {
    let detected: Vec<(u64, Origin, Option<Detection>, Option<Misreading>)> = files
        .into_iter()
        .filter_map(|(id, origin, language)| {
            let bytes = origin_bytes(&origin).ok()?;
            let classification = classify(&bytes);
            let detection = detection_for(&bytes, classification, language.as_ref());
            let misreading = misreading_for(&bytes, classification, language.as_ref());
            Some((id, origin, detection, misreading))
        })
        .collect();
    let files: Vec<(&Origin, Option<Detection>)> = detected
        .iter()
        .map(|(_, origin, detection, _)| (origin, *detection))
        .collect();
    let agreements = folder_agreements(&files);
    detected
        .iter()
        .zip(agreements)
        .map(
            |(&(id, _, detection, misreading), folder_agreement)| Redetection {
                id,
                detection,
                misreading,
                folder_agreement,
            },
        )
        .collect()
}

/// ENC-20: for each file, the agreement of its folder, when the file needs review only for too
/// little evidence and the agreed encoding decodes it strictly.
fn folder_agreements(files: &[(&Origin, Option<Detection>)]) -> Vec<Option<FolderAgreement>> {
    let mut agreements = vec![None; files.len()];
    for members in files_by_folder(files).values() {
        let Some(agreement) = agreement_among(members.iter().map(|&index| files[index].1)) else {
            continue;
        };
        for &index in members {
            let (origin, detection) = files[index];
            let fits = has_too_little_evidence(detection) && decodes(origin, agreement.encoding);
            agreements[index] = fits.then_some(agreement);
        }
    }
    agreements
}

fn files_by_folder<'origin>(
    files: &[(&'origin Origin, Option<Detection>)],
) -> HashMap<&'origin Path, Vec<usize>> {
    let mut folders: HashMap<&Path, Vec<usize>> = HashMap::new();
    for (index, (origin, _)) in files.iter().enumerate() {
        let Origin::Disk { path, .. } = origin else {
            continue;
        };
        let Some(folder) = path.parent() else {
            continue;
        };
        folders.entry(folder).or_default().push(index);
    }
    folders
}

/// ENC-20: the files detected with certainty must all agree, and there must be enough of them.
fn agreement_among(detections: impl Iterator<Item = Option<Detection>>) -> Option<FolderAgreement> {
    let certain: Vec<&'static Encoding> = detections
        .filter_map(|detection| match detection {
            Some(Detection::Certain(encoding)) => Some(encoding),
            _ => None,
        })
        .collect();
    let encoding = *certain.first()?;
    let agrees = certain.len() >= FOLDER_AGREEMENT_MINIMUM_FILES
        && certain.iter().all(|other| *other == encoding);
    agrees.then_some(FolderAgreement {
        encoding,
        files: certain.len(),
    })
}

fn decodes(origin: &Origin, encoding: &'static Encoding) -> bool {
    origin_bytes(origin).is_ok_and(|bytes| decode_strictly(&bytes, encoding).is_ok())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewProblem {
    Read(ReadProblem),
    /// UI-14: with the line where decoding stops.
    Conversion {
        failure: ConversionFailure,
        broken_line: BrokenLine,
    },
}

/// UI-04 and ENC-22: what the preview shows of a file.
#[derive(Debug)]
pub struct FilePreview {
    pub cues: Result<Vec<PreviewCue>, PreviewProblem>,
    pub repair_sample: Option<RepairSample>,
}

/// UI-04: the preview decodes exactly as the conversion would, and shows the Romanian letters it
/// writes (ENC-23). A garbled file also shows its first accented line as it is and repaired
/// (ENC-22).
pub fn preview_file(
    origin: &Origin,
    reading: Reading,
    misreading: Option<Misreading>,
    language: Option<&SubtitleLanguage>,
    comma_letters: bool,
) -> FilePreview {
    let bytes = match origin_bytes(origin) {
        Ok(bytes) => bytes,
        Err(problem) => {
            return FilePreview {
                cues: Err(PreviewProblem::Read(problem)),
                repair_sample: None,
            };
        }
    };
    let cues = convert_reading(&bytes, reading)
        .map(|conversion| preview(&written_text(&conversion.text, language, comma_letters)))
        .map_err(|failure| {
            let broken_line = broken_line(&bytes, reading.encoding(), failure.location());
            PreviewProblem::Conversion {
                failure,
                broken_line,
            }
        });
    let repair_sample = misreading
        .and_then(|misreading| repair_sample(&bytes, misreading))
        .map(|sample| RepairSample {
            repaired: written_text(&sample.repaired, language, comma_letters).into_owned(),
            ..sample
        });
    FilePreview {
        cues,
        repair_sample,
    }
}

/// The bytes of a file: read again from disk, or held in memory for a dropped file.
pub fn origin_bytes(origin: &Origin) -> Result<Vec<u8>, ReadProblem> {
    match origin {
        Origin::Dropped { bytes, .. } => Ok(bytes.to_vec()),
        Origin::Disk { path, .. } => read_file(path),
    }
}

/// A file on disk, read as sources are: never through a link, never a special file, and within
/// LIMIT-01.
pub fn read_file(path: &Path) -> Result<Vec<u8>, ReadProblem> {
    read_source(path).map_err(|error| match error {
        ReadError::Unreadable(error) => ReadProblem::Unreadable(error.kind()),
        ReadError::TooLarge { .. } => ReadProblem::TooLarge,
        ReadError::NotRegularFile => ReadProblem::NotRegularFile,
        ReadError::SymbolicLink => ReadProblem::SymbolicLink,
    })
}

impl Session {
    pub fn new(settings: SessionSettings) -> Self {
        Self {
            files: Vec::new(),
            settings,
            conversion: None,
            next_id: 1,
        }
    }

    pub fn room_left(&self) -> usize {
        MAXIMUM_LISTED_FILES.saturating_sub(self.files.len())
    }

    /// Lists prepared files; a file already in the list appears once (SAFE-14) and is read
    /// afresh, so it can be converted again. Files taking part in a running conversion are
    /// left alone. Returns how many files were listed or refreshed.
    pub fn add(&mut self, prepared: Vec<PreparedFile>) -> usize {
        let mut positions: HashMap<PathBuf, usize> = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(position, file)| match &file.origin {
                Origin::Disk { path, .. } => Some((path.clone(), position)),
                Origin::Dropped { .. } => None,
            })
            .collect();
        let mut added = 0;
        for file in prepared {
            let listed_position = match &file.origin {
                Origin::Disk { path, .. } => positions.get(path).copied(),
                Origin::Dropped { .. } => None,
            };
            if let Some(position) = listed_position {
                if self.conversion.is_none() {
                    self.files[position].refresh(file);
                    added += 1;
                }
                continue;
            }
            if self.room_left() == 0 {
                continue;
            }
            if let Origin::Disk { path, .. } = &file.origin {
                positions.insert(path.clone(), self.files.len());
            }
            self.files.push(FileEntry {
                id: self.next_id,
                origin: file.origin,
                classification: file.classification,
                detection: file.detection,
                copy_of: file.copy_of,
                folder_agreement: file.folder_agreement,
                misreading: file.misreading,
                chosen_reading: None,
                language: None,
                outcome: None,
            });
            self.next_id += 1;
            added += 1;
        }
        added
    }

    /// LIMIT-01 and LIMIT-02: dropped files live in memory, so their total is capped.
    pub fn check_dropped_room(&self, size: usize) -> Result<(), SessionProblem> {
        if self.room_left() == 0 {
            return Err(SessionProblem::ListFull);
        }
        let held: usize = self
            .files
            .iter()
            .map(|file| match &file.origin {
                Origin::Dropped { bytes, .. } => bytes.len(),
                Origin::Disk { .. } => 0,
            })
            .sum();
        let is_too_large = size as u64 > MAXIMUM_FILE_BYTES || held + size > MAXIMUM_DROPPED_BYTES;
        if is_too_large {
            return Err(SessionProblem::TooMuchDropped);
        }
        Ok(())
    }

    pub fn listed_paths(&self) -> HashSet<PathBuf> {
        self.files
            .iter()
            .filter_map(|file| match &file.origin {
                Origin::Disk { path, .. } => Some(path.clone()),
                Origin::Dropped { .. } => None,
            })
            .collect()
    }

    pub fn find(&self, id: u64) -> Result<&FileEntry, SessionProblem> {
        self.files
            .iter()
            .find(|file| file.id == id)
            .ok_or(SessionProblem::UnknownFile)
    }

    fn find_mut(&mut self, id: u64) -> Result<&mut FileEntry, SessionProblem> {
        self.files
            .iter_mut()
            .find(|file| file.id == id)
            .ok_or(SessionProblem::UnknownFile)
    }

    fn check_idle(&self) -> Result<(), SessionProblem> {
        if self.conversion.is_some() {
            return Err(SessionProblem::ConversionRunning);
        }
        Ok(())
    }

    /// ENC-12 and ENC-21: applies a hand-chosen reading once the file converts with it.
    pub fn choose_reading(
        &mut self,
        id: u64,
        reading: Reading,
        bytes: &[u8],
    ) -> Result<(), SessionProblem> {
        self.check_idle()?;
        let file = self.find_mut(id)?;
        let classification = file
            .classification
            .map_err(|_| SessionProblem::ManualChoice(ManualChoiceRefusal::NotApplicable))?;
        if !file.can_choose_encoding() {
            return Err(SessionProblem::ManualChoice(
                ManualChoiceRefusal::NotApplicable,
            ));
        }
        check_manual_reading(bytes, classification, reading)
            .map_err(SessionProblem::ManualChoice)?;
        file.chosen_reading = Some(reading);
        file.outcome = None;
        Ok(())
    }

    /// ENC-22: repairs a garbled file, or keeps it as it is.
    pub fn choose_repair(&mut self, id: u64, repair: bool) -> Result<(), SessionProblem> {
        self.check_idle()?;
        let file = self.find_mut(id)?;
        let Some(misreading) = file.misreading.filter(|_| file.can_repair()) else {
            return Err(SessionProblem::ManualChoice(
                ManualChoiceRefusal::NotApplicable,
            ));
        };
        let reading = if repair {
            Reading::RepairMisreading(misreading)
        } else {
            Reading::Whole(UTF_8)
        };
        file.chosen_reading = Some(reading);
        file.outcome = None;
        Ok(())
    }

    /// UI-06: files cannot be removed while a conversion runs.
    pub fn remove(&mut self, id: u64) -> Result<(), SessionProblem> {
        self.check_idle()?;
        self.find(id)?;
        self.files.retain(|file| file.id != id);
        Ok(())
    }

    pub fn clear(&mut self) -> Result<(), SessionProblem> {
        self.check_idle()?;
        self.files.clear();
        Ok(())
    }

    /// ENC-13 and ENC-22: files whose detection follows the saved language, with it, to detect
    /// again when it changes. Files with their own language keep it (UI-16).
    pub fn detection_dependent(&self) -> Vec<(u64, Origin, Option<SubtitleLanguage>)> {
        self.files
            .iter()
            .filter(|file| {
                file.outcome.is_none() && file.language.is_none() && file.depends_on_language()
            })
            .map(|file| (file.id, file.origin.clone(), self.settings.language.clone()))
            .collect()
    }

    /// UI-16: gives a file its own subtitle language, or takes it away. Returns whether it
    /// changed, and so whether the file is detected again (ENC-13).
    pub fn set_file_language(
        &mut self,
        id: u64,
        language: Option<SubtitleLanguage>,
    ) -> Result<bool, SessionProblem> {
        self.check_idle()?;
        let file = self.find_mut(id)?;
        let changed = file.language != language;
        file.language = language;
        Ok(changed)
    }

    /// ENC-22: a repair chosen for another misreading is chosen again.
    pub fn set_detection(&mut self, redetection: Redetection) {
        let Ok(file) = self.find_mut(redetection.id) else {
            return;
        };
        if file.misreading != redetection.misreading {
            file.chosen_reading = None;
        }
        file.detection = redetection.detection;
        file.misreading = redetection.misreading;
        file.folder_agreement = redetection.folder_agreement;
    }

    /// UI-05: settings cannot change during a conversion. Returns whether the language
    /// changed, in which case detection runs again (ENC-13).
    pub fn update_settings(&mut self, settings: SessionSettings) -> Result<bool, SessionProblem> {
        self.check_idle()?;
        let language_changed = self.settings.language != settings.language;
        self.settings = settings;
        Ok(language_changed)
    }

    /// SAFE-02: dropped files always go to the output folder, browsed ones when chosen.
    pub fn needs_output_folder(&self) -> bool {
        let has_pending_dropped = self
            .files
            .iter()
            .any(|file| file.is_dropped() && file.pending_reading().is_some());
        self.settings.destination == Destination::OutputFolder || has_pending_dropped
    }

    pub fn cancel_conversion(&self) {
        if let Some(conversion) = &self.conversion {
            conversion.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// UI-05: every Ready file, and every one skipped or failed before, with the encoding to
    /// convert it with.
    pub fn start_conversion(&mut self) -> Result<ConversionStart, SessionProblem> {
        self.check_idle()?;
        let (file_ids, jobs): (Vec<u64>, Vec<ConversionJob>) = self
            .files
            .iter()
            .filter_map(|file| {
                let reading = file.pending_reading()?;
                let language = file.effective_language(self.settings.language.as_ref());
                let job = ConversionJob {
                    origin: file.origin.clone(),
                    reading,
                    language: language.cloned(),
                };
                Some((file.id, job))
            })
            .unzip();
        let cancel = Arc::new(AtomicBool::new(false));
        self.conversion = Some(RunningConversion {
            finished: 0,
            total: jobs.len(),
            file_ids,
            cancel: Arc::clone(&cancel),
        });
        Ok(ConversionStart {
            jobs,
            settings: self.settings.batch_settings(self.settings.collision_policy),
            listed_paths: self.listed_paths(),
            cancel,
        })
    }

    /// Records one finished file; a file that was not reached stays Ready.
    pub fn record_outcome(&mut self, finished: usize, outcome: &Outcome) {
        let Some(conversion) = self.conversion.as_mut() else {
            return;
        };
        conversion.finished = finished;
        let Some(id) = conversion.file_ids.get(finished.saturating_sub(1)).copied() else {
            return;
        };
        if *outcome == Outcome::NotReached {
            return;
        }
        if let Ok(file) = self.find_mut(id) {
            file.outcome = Some(outcome.clone());
        }
    }

    pub fn finish_conversion(&mut self) {
        self.conversion = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{UTF_16LE, WINDOWS_1250};
    use std::ffi::OsStr;
    use std::path::Path;
    use subutf8_core::constants::UTF8_BYTE_ORDER_MARK;
    use subutf8_core::decoding::MisreadVia;
    use subutf8_core::language::with_romanian_comma_letters;
    use subutf8_core::report::{FailureCause, SkipCause};

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../subutf8-core/tests/fixtures/source"
    );

    fn fixture(name: &str) -> Origin {
        Origin::Disk {
            path: Path::new(FIXTURES).join(format!("{name}.srt")),
            relative_folder: PathBuf::new(),
        }
    }

    fn session() -> Session {
        Session::new(SessionSettings {
            language: None,
            destination: Destination::BesideOriginals,
            output_folder: PathBuf::from("/tmp"),
            organise_by_day: false,
            collision_policy: CollisionPolicy::Skip,
            romanian_comma_letters: false,
        })
    }

    #[test]
    fn statuses_follow_the_rules() {
        let mut session = session();
        let names = [
            "windows-1250-romanian",
            "short-romanian",
            "utf8-romanian",
            "utf8-bom-romanian",
            "utf16le-no-bom",
            "binary",
            "empty",
            "ascii-only",
            "garbled-romanian",
        ];
        let prepared = names
            .iter()
            .map(|name| prepare(fixture(name), None))
            .collect();
        assert_eq!(session.add(prepared), names.len());
        let statuses: Vec<FileStatus> = session.files.iter().map(FileEntry::status).collect();
        assert_eq!(statuses[0], FileStatus::Ready(Reading::Whole(WINDOWS_1250)));
        assert!(matches!(
            statuses[1],
            FileStatus::NeedsReview {
                cause: ReviewCause::Detection(ReviewReason::TooLittleEvidence),
                ..
            }
        ));
        assert_eq!(statuses[2], FileStatus::Ready(Reading::Whole(UTF_8)));
        assert_eq!(statuses[3], FileStatus::Ready(Reading::Whole(UTF_8)));
        assert_eq!(
            statuses[4],
            FileStatus::NeedsReview {
                suggestion: Some(UTF_16LE),
                cause: ReviewCause::LooksLikeUtf16
            }
        );
        assert_eq!(statuses[5], FileStatus::Refused(Refusal::NotText));
        assert_eq!(statuses[6], FileStatus::Refused(Refusal::Empty));
        assert_eq!(statuses[7], FileStatus::Ready(Reading::Whole(UTF_8)));
        assert_eq!(
            statuses[8],
            FileStatus::NeedsReview {
                suggestion: Some(UTF_8),
                cause: ReviewCause::LooksGarbled(Misreading {
                    via: MisreadVia::Windows1252,
                    original: WINDOWS_1250
                })
            }
        );
    }

    /// ENC-22: a garbled file waits for Repair or Keep as it is; other files take neither.
    #[test]
    fn garbled_file_is_repaired_or_kept() {
        let mut session = session();
        session.add(vec![
            prepare(fixture("garbled-romanian"), None),
            prepare(fixture("utf8-romanian"), None),
        ]);
        let (garbled_id, utf8_id) = (session.files[0].id, session.files[1].id);
        let misreading = session.files[0].misreading.unwrap();
        session.choose_repair(garbled_id, true).unwrap();
        assert_eq!(
            session.find(garbled_id).unwrap().status(),
            FileStatus::Ready(Reading::RepairMisreading(misreading))
        );
        session.choose_repair(garbled_id, false).unwrap();
        assert_eq!(
            session.find(garbled_id).unwrap().status(),
            FileStatus::Ready(Reading::Whole(UTF_8))
        );
        assert_eq!(
            session.choose_repair(utf8_id, true),
            Err(SessionProblem::ManualChoice(
                ManualChoiceRefusal::NotApplicable
            ))
        );
    }

    /// SAFE-18 and ENC-22: outputs of a garbled file, repaired or kept as it was, are left
    /// alone.
    #[test]
    fn outputs_of_garbled_files_are_left_alone() {
        let folder = tempfile::tempdir().unwrap();
        let garbled = fs::read(Path::new(FIXTURES).join("garbled-romanian.srt")).unwrap();
        let repaired =
            fs::read_to_string(Path::new(FIXTURES).join("../expected/windows-1250-romanian.txt"))
                .unwrap();
        let kept = String::from_utf8(garbled.clone()).unwrap();
        fs::write(folder.path().join("Film.srt"), garbled).unwrap();
        fs::write(
            folder.path().join("Film1.srt"),
            [UTF8_BYTE_ORDER_MARK, &repaired].concat(),
        )
        .unwrap();
        fs::write(
            folder.path().join("Film2.srt"),
            [UTF8_BYTE_ORDER_MARK, &kept].concat(),
        )
        .unwrap();
        let area = AllowedArea::new([folder.path().to_path_buf()]);
        let gathered = gather(&[folder.path().to_path_buf()], false, &area, 100, None);
        let mut session = session();
        session.add(gathered.prepared);
        for name in ["Film1.srt", "Film2.srt"] {
            assert_eq!(
                status_by_name(&session, name),
                FileStatus::ConvertedCopy("Film.srt"),
                "{name}"
            );
        }
    }

    /// SAFE-14: listed once; adding it again makes it ready to convert again.
    #[test]
    fn adding_a_listed_file_again_refreshes_it() {
        let mut session = session();
        let first = vec![prepare(fixture("windows-1250-romanian"), None)];
        let second = vec![
            prepare(fixture("windows-1250-romanian"), None),
            prepare(fixture("windows-1250-romanian"), None),
        ];
        assert_eq!(session.add(first), 1);
        session.files[0].outcome = Some(Outcome::Skipped(SkipCause::AlreadyExists));
        session.files[0].chosen_reading = Some(Reading::Whole(WINDOWS_1250));
        assert_eq!(session.add(second), 2);
        assert_eq!(session.files.len(), 1);
        assert_eq!(session.files[0].outcome, None);
        assert_eq!(session.files[0].chosen_reading, None);
    }

    /// UI-05: skipped and failed files are converted again; converted ones are done.
    #[test]
    fn only_converted_files_leave_the_next_conversion() {
        let mut session = session();
        session.add(vec![prepare(fixture("windows-1250-romanian"), None)]);
        let file = &mut session.files[0];
        file.outcome = Some(Outcome::Skipped(SkipCause::AlreadyExists));
        assert_eq!(file.pending_reading(), Some(Reading::Whole(WINDOWS_1250)));
        file.outcome = Some(Outcome::Failed(FailureCause::VerificationFailed));
        assert_eq!(file.pending_reading(), Some(Reading::Whole(WINDOWS_1250)));
        assert!(file.can_choose_encoding());
        file.outcome = Some(Outcome::Converted {
            output: PathBuf::from("/tmp/out.srt"),
            warnings: Vec::new(),
            structure_warnings: Vec::new(),
        });
        assert_eq!(file.pending_reading(), None);
        assert!(!file.can_choose_encoding());
        let start = session.start_conversion().unwrap();
        assert!(start.jobs.is_empty());
    }

    /// ENC-12.
    #[test]
    fn manual_choice_makes_a_file_ready_or_is_refused() {
        let mut session = session();
        session.add(vec![prepare(fixture("short-romanian"), None)]);
        session.add(vec![prepare(fixture("utf8-romanian"), None)]);
        let short_id = session.files[0].id;
        let utf8_id = session.files[1].id;
        let short_bytes = origin_bytes(&session.files[0].origin).unwrap();
        session
            .choose_reading(short_id, Reading::Whole(WINDOWS_1250), &short_bytes)
            .unwrap();
        assert_eq!(
            session.find(short_id).unwrap().status(),
            FileStatus::Ready(Reading::Whole(WINDOWS_1250))
        );
        let utf8_bytes = origin_bytes(&session.files[1].origin).unwrap();
        assert_eq!(
            session.choose_reading(utf8_id, Reading::Whole(WINDOWS_1250), &utf8_bytes),
            Err(SessionProblem::ManualChoice(
                ManualChoiceRefusal::NotApplicable
            ))
        );
    }

    /// SAFE-18: an output of a file beside it is left alone, and only when its text is exactly
    /// what converting that file gives; a translation with a tag-like name is converted.
    #[test]
    fn outputs_of_files_beside_them_are_left_alone() {
        let folder = tempfile::tempdir().unwrap();
        let original = fs::read(Path::new(FIXTURES).join("windows-1250-romanian.srt")).unwrap();
        let expected =
            fs::read_to_string(Path::new(FIXTURES).join("../expected/windows-1250-romanian.txt"))
                .unwrap();
        let other = fs::read_to_string(Path::new(FIXTURES).join("utf8-romanian.srt")).unwrap();
        fs::write(folder.path().join("Film.srt"), original).unwrap();
        for (name, text) in [
            ("Film1.srt", [UTF8_BYTE_ORDER_MARK, &expected].concat()),
            ("Film.ro.srt", expected.clone()),
            ("Film.en.srt", [UTF8_BYTE_ORDER_MARK, &other].concat()),
        ] {
            fs::write(folder.path().join(name), text).unwrap();
        }
        let area = AllowedArea::new([folder.path().to_path_buf()]);
        let gathered = gather(&[folder.path().to_path_buf()], false, &area, 100, None);
        let mut session = session();
        session.add(gathered.prepared);
        let statuses: HashMap<String, FileStatus> = session
            .files
            .iter()
            .map(|file| (file.display_name(), file.status()))
            .collect();
        assert_eq!(statuses["Film1.srt"], FileStatus::ConvertedCopy("Film.srt"));
        assert_eq!(
            statuses["Film.ro.srt"],
            FileStatus::ConvertedCopy("Film.srt")
        );
        assert_eq!(
            statuses["Film.en.srt"],
            FileStatus::Ready(Reading::Whole(UTF_8))
        );
        assert_eq!(
            statuses["Film.srt"],
            FileStatus::Ready(Reading::Whole(WINDOWS_1250))
        );
        assert_eq!(session.start_conversion().unwrap().jobs.len(), 2);
    }

    /// SAFE-18 and ENC-23: an output written with the comma letters is recognised too.
    #[test]
    fn outputs_with_romanian_comma_letters_are_left_alone() {
        let folder = tempfile::tempdir().unwrap();
        let expected =
            fs::read_to_string(Path::new(FIXTURES).join("../expected/windows-1250-romanian.txt"))
                .unwrap();
        fs::copy(
            Path::new(FIXTURES).join("windows-1250-romanian.srt"),
            folder.path().join("Film.srt"),
        )
        .unwrap();
        let comma_output = [
            UTF8_BYTE_ORDER_MARK,
            &with_romanian_comma_letters(&expected),
        ]
        .concat();
        fs::write(folder.path().join("Film.ro.srt"), comma_output).unwrap();
        let area = AllowedArea::new([folder.path().to_path_buf()]);
        let gathered = gather(&[folder.path().to_path_buf()], false, &area, 100, None);
        let mut session = session();
        session.add(gathered.prepared);
        assert_eq!(
            status_by_name(&session, "Film.ro.srt"),
            FileStatus::ConvertedCopy("Film.srt")
        );
    }

    /// NAME-11: 0x81 means nothing in windows-1250, so this name does not read in the file's
    /// encoding, and the file is left alone.
    #[test]
    fn unreadable_legacy_names_are_left_alone() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join(OsStr::from_bytes(b"Bad\x81.srt"));
        fs::copy(Path::new(FIXTURES).join("windows-1250-romanian.srt"), &path).unwrap();
        let origin = Origin::Disk {
            path,
            relative_folder: PathBuf::new(),
        };
        let mut session = session();
        session.add(vec![prepare(origin, None)]);
        let file = &session.files[0];
        assert_eq!(
            file.status(),
            FileStatus::Refused(Refusal::NameNotDecodable)
        );
        assert_eq!(file.display_name(), "Bad\u{FFFD}.srt");
        assert!(session.start_conversion().unwrap().jobs.is_empty());
    }

    /// LIMIT-02.
    #[test]
    fn dropped_files_have_a_total_limit() {
        let session = session();
        assert_eq!(session.check_dropped_room(1_000), Ok(()));
        assert_eq!(
            session.check_dropped_room(MAXIMUM_DROPPED_BYTES + 1),
            Err(SessionProblem::TooMuchDropped)
        );
    }

    /// Copies fixtures into a folder under the given names, then adds the whole folder.
    fn folder_session(folder: &Path, files: &[(&str, &str)]) -> Session {
        for (name, fixture) in files {
            fs::copy(
                Path::new(FIXTURES).join(format!("{fixture}.srt")),
                folder.join(name),
            )
            .unwrap();
        }
        let area = AllowedArea::new([folder.to_path_buf()]);
        let gathered = gather(&[folder.to_path_buf()], false, &area, 100, None);
        let mut session = session();
        session.add(gathered.prepared);
        session
    }

    fn status_by_name<'session>(session: &'session Session, name: &str) -> FileStatus<'session> {
        let file = session
            .files
            .iter()
            .find(|file| file.display_name() == name);
        file.unwrap().status()
    }

    /// ENC-20, and again after a language change (ENC-13).
    #[test]
    fn folder_agreement_suggests_the_siblings_encoding() {
        let folder = tempfile::tempdir().unwrap();
        let mut session = folder_session(
            folder.path(),
            &[
                ("a.srt", "windows-1250-romanian"),
                ("b.srt", "windows-1250-romanian"),
                ("c.srt", "short-romanian"),
            ],
        );
        let agreed = FileStatus::NeedsReview {
            suggestion: Some(WINDOWS_1250),
            cause: ReviewCause::FolderAgrees(FolderAgreement {
                encoding: WINDOWS_1250,
                files: 2,
            }),
        };
        assert_eq!(status_by_name(&session, "c.srt"), agreed);
        assert_eq!(
            status_by_name(&session, "a.srt"),
            FileStatus::Ready(Reading::Whole(WINDOWS_1250))
        );

        let mut settings = session.settings.clone();
        settings.language = SubtitleLanguage::parse("ro").ok();
        assert_eq!(session.update_settings(settings), Ok(true));
        for redetection in detect_again(session.detection_dependent()) {
            session.set_detection(redetection);
        }
        assert_eq!(status_by_name(&session, "c.srt"), agreed);
    }

    /// ENC-20: two different encodings, or a single file, are no agreement.
    #[test]
    fn folder_agreement_needs_one_encoding() {
        let too_little_evidence = FileStatus::NeedsReview {
            suggestion: Some(WINDOWS_1250),
            cause: ReviewCause::Detection(ReviewReason::TooLittleEvidence),
        };
        let two_encodings = tempfile::tempdir().unwrap();
        let session = folder_session(
            two_encodings.path(),
            &[
                ("a.srt", "windows-1250-romanian"),
                ("b.srt", "windows-1251-russian"),
                ("c.srt", "short-romanian"),
            ],
        );
        assert_eq!(status_by_name(&session, "c.srt"), too_little_evidence);

        let one_file = tempfile::tempdir().unwrap();
        let session = folder_session(
            one_file.path(),
            &[
                ("a.srt", "windows-1250-romanian"),
                ("c.srt", "short-romanian"),
            ],
        );
        assert_eq!(status_by_name(&session, "c.srt"), too_little_evidence);
    }
}
