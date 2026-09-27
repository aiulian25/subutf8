use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use subutf8_core::batch::{CollisionPolicy, Destination, Origin};
use subutf8_core::classification::Classification;
use subutf8_core::decoding::{ConversionFailure, ConversionWarning, Location, Reading};
use subutf8_core::detection::{Candidate, ManualChoiceRefusal, ReviewReason};
use subutf8_core::input_scan::{SkipReason, SkippedPath};
use subutf8_core::report::{FailureCause, Outcome, SkipCause};
use subutf8_core::srt_structure::StructureWarning;

use crate::constants::SINGLE_WARNING_COUNT;
use crate::defaults::{Defaults, DefaultsStore, StoreProblem};
use crate::folders::Listing;
use crate::history::HistoryRecord;
use crate::json_path::encode_hex;
use crate::session::{
    FileEntry, FilePreview, FileStatus, Gathered, PreviewProblem, ReadProblem, Refusal,
    ReviewCause, Session,
};
use crate::settings::{Mode, Settings};
use crate::update::{Package, Step, UpdateProblem, UpdateState};
use crate::watch::{WatchLog, WatchResult};

/// UI-02.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatusName {
    Ready,
    NeedsReview,
    Converted,
    Skipped,
    Failed,
}

/// Every reason the interface can show; `ui/constants.js` holds the wording (UI-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    TooLittleEvidence,
    GuessDoesNotDecode,
    LanguageHintDisagrees,
    LooksLikeUtf16,
    DamagedOrMixedUtf8,
    Empty,
    NotText,
    DamagedUtf8,
    DamagedUtf16,
    DoesNotDecode,
    RoundTripDiffers,
    Unreadable,
    TooLarge,
    NotRegularFile,
    SymbolicLink,
    NotSrt,
    OutsideAllowedArea,
    NotAFolder,
    InvalidPath,
    NameNotDecodable,
    AlreadyExists,
    NameTakenInList,
    ListedFile,
    ReadOnlyDestination,
    FolderNotWritable,
    NoFreeName,
    NameTooLong,
    VerificationFailed,
    WriteFailed,
    Cancelled,
    UnknownFile,
    ConversionRunning,
    TooMuchDropped,
    ListFull,
    NotOffered,
    NotApplicable,
    InvalidLanguage,
    UnusableDroppedName,
    OutputFolderUnavailable,
    NotAvailable,
    Internal,
    ConvertedCopy,
    FolderAgrees,
    LooksGarbled,
    ControlCharactersInUtf8,
    NotConverted,
    WaitingInList,
    SettingsDamaged,
    SettingsUnreadable,
    SettingsNotSaved,
    DataFolderReadOnly,
    HistoryNotSaved,
    TooManyWatchFolders,
    WatchFolderHoldsOutputFolder,
    UpdateCheckOff,
    UpdateCheckFailed,
    NoUpdate,
    UpdateBusy,
    UpdateNotDownloaded,
    UpdateNotInstalled,
    NoReleaseFile,
    DownloadFailed,
    NotGithub,
    ChecksumMismatch,
    NoChecksum,
    NoPrivilegeProgram,
    NotAuthorized,
    InstallFailed,
    ReplaceFailed,
}

/// A reason with the details that make it actionable: where a file stops decoding, the
/// system's own words for a read or write error, the file it concerns, the encoding and how
/// many files agree on it (ENC-20), or the encoding garbled text was and how it was misread
/// (ENC-22).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub misread_as: Option<&'static str>,
}

impl Problem {
    pub fn of(reason: Reason) -> Self {
        Self {
            reason,
            byte_offset: None,
            line: None,
            system_reason: None,
            related_name: None,
            count: None,
            encoding: None,
            misread_as: None,
        }
    }

    pub fn about(reason: Reason, name: &str) -> Self {
        Self {
            related_name: Some(name.to_owned()),
            ..Self::of(reason)
        }
    }

    fn at(reason: Reason, location: Location) -> Self {
        Self {
            byte_offset: Some(location.byte_offset),
            line: Some(location.line),
            ..Self::of(reason)
        }
    }

    pub fn system(reason: Reason, kind: io::ErrorKind) -> Self {
        Self {
            system_reason: Some(kind.to_string()),
            ..Self::of(reason)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningName {
    RoundTripDiffers,
    ContainsReplacementCharacters,
    NoCues,
    MissingCueNumber,
    CueNumberOutOfOrder,
    MalformedTiming,
    TimingUsesDot,
}

/// A warning, and how many of its kind it stands for (ENC-19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarningView {
    warning: WarningName,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    id: u64,
    name: String,
    /// The folder of a browsed file; dropped files have none (SAFE-02).
    folder: Option<String>,
    status: StatusName,
    /// The encoding the file is read with, as the Encoding menu names it.
    encoding: Option<&'static str>,
    /// ENC-21: how the file is read, as shown: `windows-1250` or `UTF-8 + windows-1250`.
    reading: Option<String>,
    problem: Option<Problem>,
    output: Option<String>,
    warnings: Vec<WarningView>,
    can_choose_encoding: bool,
    /// ENC-21: a damaged or mixed file can keep its lines that are UTF-8, and whether it does.
    can_keep_utf8_lines: bool,
    keeps_utf8_lines: bool,
    /// ENC-22: a garbled file can be repaired or kept as it is, and whether it is repaired.
    can_repair: bool,
    is_repaired: bool,
    /// UI-05: the next conversion includes this file.
    is_pending: bool,
    /// UI-16: the file's own subtitle language only; the saved one is in the defaults.
    language: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModeName {
    Desktop,
    Container,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DestinationName {
    BesideOriginals,
    OutputFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollisionName {
    Skip,
    Rename,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProgressView {
    finished: usize,
    total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateView {
    mode: ModeName,
    version: &'static str,
    files: Vec<FileView>,
    conversion: Option<ProgressView>,
    browse_start: String,
    /// The folders the app may use; in Docker, the mounted ones (TARGET-02).
    roots: Vec<String>,
    /// The desktop app shows itself in its own window, where closing the window quits.
    window_open: bool,
    /// SET-01: the saved settings, which every conversion uses.
    defaults: Defaults,
    /// SET-02: why settings or the history cannot be kept.
    data_problem: Option<Problem>,
    update: UpdateView,
}

/// What the page is told beside the file list.
pub struct StateExtras {
    pub window_open: bool,
    pub defaults: Defaults,
    pub data_problem: Option<Problem>,
    pub update: UpdateView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageName {
    Docker,
    Deb,
    Rpm,
    Appimage,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateStepName {
    Idle,
    Downloading,
    Installing,
    Installed,
    Failed,
}

/// UPDATE-01 to UPDATE-05.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    allowed: bool,
    current: &'static str,
    checked: bool,
    check_failed: bool,
    latest: Option<String>,
    release_page: Option<String>,
    package: PackageName,
    can_install: bool,
    step: UpdateStepName,
    received: u64,
    total: u64,
    downloaded: Option<String>,
    problem: Option<Problem>,
}

/// WATCH-04.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchLogView {
    entries: Vec<WatchEntryView>,
    unavailable_folders: Vec<String>,
    output_folder_unavailable: bool,
    interval_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchEntryView {
    time: String,
    name: String,
    folder: Option<String>,
    status: StatusName,
    problem: Option<Problem>,
    output: Option<String>,
}

/// HIST-02.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryView {
    time: String,
    name: String,
    source: String,
    output: String,
    encoding: String,
    language: Option<String>,
    watched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingView {
    path: String,
    parent: Option<String>,
    folders: Vec<String>,
    files: Vec<String>,
    /// NAME-11: `.srt` files whose names are not UTF-8.
    raw_files: Vec<RawEntryView>,
    is_truncated: bool,
}

/// NAME-11: a name shown as far as it reads, and its path's bytes to add it by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RawEntryView {
    display: String,
    hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedView {
    path: String,
    problem: Problem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedView {
    added: usize,
    skipped: Vec<SkippedView>,
    limit_reached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CueView {
    number: Option<String>,
    timing: Option<String>,
    text: String,
    is_clipped: bool,
}

/// UI-14.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrokenLineView {
    line: usize,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    encoding: Option<&'static str>,
    cues: Vec<CueView>,
    problem: Option<Problem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    broken_line: Option<BrokenLineView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repair: Option<RepairView>,
}

/// ENC-22: a garbled file's first accented line, as it is and repaired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairView {
    as_is: String,
    repaired: String,
}

/// UI-19: the languages that steer detection (ENC-13); the page adds common ones that only name
/// outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguagesView {
    pub hinted: Vec<&'static str>,
}

/// UI-15.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CandidatesView {
    candidates: Vec<CandidateView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateView {
    encoding: &'static str,
    sample: String,
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl From<DestinationName> for Destination {
    fn from(destination: DestinationName) -> Self {
        match destination {
            DestinationName::BesideOriginals => Self::BesideOriginals,
            DestinationName::OutputFolder => Self::OutputFolder,
        }
    }
}

impl From<CollisionName> for CollisionPolicy {
    fn from(policy: CollisionName) -> Self {
        match policy {
            CollisionName::Skip => Self::Skip,
            CollisionName::Rename => Self::Rename,
            CollisionName::Overwrite => Self::Overwrite,
        }
    }
}

pub fn state_view(settings: &Settings, session: &Session, extras: StateExtras) -> StateView {
    let mode = match settings.mode {
        Mode::Desktop => ModeName::Desktop,
        Mode::Container => ModeName::Container,
    };
    StateView {
        mode,
        version: env!("CARGO_PKG_VERSION"),
        files: session.files.iter().map(file_view).collect(),
        conversion: session.conversion.as_ref().map(|conversion| ProgressView {
            finished: conversion.finished,
            total: conversion.total,
        }),
        browse_start: text_of(&settings.browse_start),
        roots: settings
            .allowed_area
            .roots()
            .iter()
            .map(|root| text_of(root))
            .collect(),
        window_open: extras.window_open,
        defaults: extras.defaults,
        data_problem: extras.data_problem,
        update: extras.update,
    }
}

/// SET-02: why settings or the history are not kept; a folder that cannot be written explains
/// the rest.
pub fn data_problem(
    store: &DefaultsStore,
    history_problem: Option<io::ErrorKind>,
    data_folder_writable: bool,
) -> Option<Problem> {
    let store_problem = store.problem.map(store_problem);
    let history_problem =
        history_problem.map(|kind| Problem::system(Reason::HistoryNotSaved, kind));
    let read_only = (!data_folder_writable).then(|| Problem::of(Reason::DataFolderReadOnly));
    read_only.or(store_problem).or(history_problem)
}

pub fn store_problem(problem: StoreProblem) -> Problem {
    match problem {
        StoreProblem::Damaged => Problem::of(Reason::SettingsDamaged),
        StoreProblem::Unreadable(kind) => Problem::system(Reason::SettingsUnreadable, kind),
        StoreProblem::NotSaved(kind) => Problem::system(Reason::SettingsNotSaved, kind),
    }
}

pub fn update_view(update: &UpdateState, allowed: bool) -> UpdateView {
    let (step, received, total, problem) = match &update.step {
        Step::Idle => (UpdateStepName::Idle, 0, 0, None),
        Step::Downloading { received, total } => {
            (UpdateStepName::Downloading, *received, *total, None)
        }
        Step::Installing => (UpdateStepName::Installing, 0, 0, None),
        Step::Installed => (UpdateStepName::Installed, 0, 0, None),
        Step::Failed(problem) => (UpdateStepName::Failed, 0, 0, Some(update_problem(*problem))),
    };
    let package = match update.package {
        Package::Docker => PackageName::Docker,
        Package::Deb => PackageName::Deb,
        Package::Rpm => PackageName::Rpm,
        Package::AppImage(_) => PackageName::Appimage,
        Package::Other => PackageName::Other,
    };
    UpdateView {
        allowed,
        current: env!("CARGO_PKG_VERSION"),
        checked: update.last_check.is_some(),
        check_failed: update.check_failed,
        latest: update.newer.as_ref().map(|release| release.version.clone()),
        release_page: update
            .newer
            .as_ref()
            .and_then(|release| release.page.clone()),
        package,
        can_install: update.release_file().is_some(),
        step,
        received,
        total,
        downloaded: update
            .downloaded
            .as_ref()
            .map(|downloaded| text_of(&downloaded.path)),
        problem,
    }
}

pub fn update_problem(problem: UpdateProblem) -> Problem {
    Problem::of(match problem {
        UpdateProblem::CheckFailed => Reason::UpdateCheckFailed,
        UpdateProblem::NoReleaseFile => Reason::NoReleaseFile,
        UpdateProblem::DownloadFailed => Reason::DownloadFailed,
        UpdateProblem::NotGithub => Reason::NotGithub,
        UpdateProblem::ChecksumMismatch => Reason::ChecksumMismatch,
        UpdateProblem::NoChecksum => Reason::NoChecksum,
        UpdateProblem::NoPrivilegeProgram => Reason::NoPrivilegeProgram,
        UpdateProblem::NotAuthorized => Reason::NotAuthorized,
        UpdateProblem::InstallFailed => Reason::InstallFailed,
        UpdateProblem::ReplaceFailed => Reason::ReplaceFailed,
    })
}

/// WATCH-04: newest first.
pub fn watch_log_view(log: &WatchLog, interval: Duration) -> WatchLogView {
    let entries = log
        .entries
        .iter()
        .rev()
        .map(|entry| {
            let description = match &entry.result {
                WatchResult::Finished(outcome) => describe_outcome(outcome),
                WatchResult::Listed => Description::with_problem(
                    StatusName::NeedsReview,
                    Problem::of(Reason::WaitingInList),
                ),
                WatchResult::ConvertedCopy(original) => Description::with_problem(
                    StatusName::Skipped,
                    Problem::about(Reason::ConvertedCopy, original),
                ),
            };
            WatchEntryView {
                time: entry.time.clone(),
                name: file_name_of(&entry.path),
                folder: entry.path.parent().map(text_of),
                status: description.status,
                problem: description.problem,
                output: description.output,
            }
        })
        .collect();
    WatchLogView {
        entries,
        unavailable_folders: log
            .unavailable_folders
            .iter()
            .map(|folder| text_of(folder))
            .collect(),
        output_folder_unavailable: log.output_folder_unavailable,
        interval_seconds: interval.as_secs(),
    }
}

/// HIST-02.
pub fn history_view(records: Vec<HistoryRecord>) -> Vec<HistoryEntryView> {
    records
        .into_iter()
        .map(|record| HistoryEntryView {
            name: file_name_of(&record.source),
            source: text_of(&record.source),
            output: text_of(&record.output),
            time: record.time,
            encoding: record.encoding,
            language: record.language,
            watched: record.watched,
        })
        .collect()
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| text_of(path), |name| name.to_string_lossy().into_owned())
}

struct Description {
    status: StatusName,
    problem: Option<Problem>,
    output: Option<String>,
    warnings: Vec<WarningView>,
}

impl Description {
    fn plain(status: StatusName) -> Self {
        Self {
            status,
            problem: None,
            output: None,
            warnings: Vec::new(),
        }
    }

    fn with_problem(status: StatusName, problem: Problem) -> Self {
        Self {
            problem: Some(problem),
            ..Self::plain(status)
        }
    }
}

fn file_view(file: &FileEntry) -> FileView {
    let description = describe(file.status());
    let folder = match &file.origin {
        Origin::Disk { path, .. } => path.parent().map(text_of),
        Origin::Dropped { .. } => None,
    };
    let reading = file.current_reading();
    let is_mixed = file.classification == Ok(Classification::DamagedOrMixedUtf8);
    FileView {
        id: file.id,
        name: file.display_name(),
        folder,
        status: description.status,
        encoding: reading.map(|reading| reading.encoding().name()),
        reading: reading.map(Reading::name),
        problem: description.problem,
        output: description.output,
        warnings: description.warnings,
        can_choose_encoding: file.can_choose_encoding(),
        can_keep_utf8_lines: file.can_choose_encoding() && is_mixed,
        keeps_utf8_lines: matches!(reading, Some(Reading::Utf8LinesElse(_))),
        can_repair: file.can_repair(),
        is_repaired: matches!(reading, Some(Reading::RepairMisreading(_))),
        is_pending: file.pending_reading().is_some(),
        language: file
            .language
            .as_ref()
            .map(|language| language.tag().to_owned()),
    }
}

fn describe(status: FileStatus<'_>) -> Description {
    match status {
        FileStatus::Ready(_) => Description::plain(StatusName::Ready),
        FileStatus::NeedsReview { cause, .. } => {
            Description::with_problem(StatusName::NeedsReview, review_problem(cause))
        }
        FileStatus::Refused(refusal) => describe_refusal(refusal),
        FileStatus::Finished(outcome) => describe_outcome(outcome),
        FileStatus::ConvertedCopy(original) => Description::with_problem(
            StatusName::Skipped,
            Problem::about(Reason::ConvertedCopy, original),
        ),
    }
}

fn review_problem(cause: ReviewCause) -> Problem {
    match cause {
        ReviewCause::FolderAgrees(agreement) => Problem {
            count: Some(agreement.files),
            encoding: Some(agreement.encoding.name()),
            ..Problem::of(Reason::FolderAgrees)
        },
        ReviewCause::LooksGarbled(misreading) => Problem {
            encoding: Some(misreading.original.name()),
            misread_as: Some(misreading.via.name()),
            ..Problem::of(Reason::LooksGarbled)
        },
        _ => Problem::of(review_reason(cause)),
    }
}

fn review_reason(cause: ReviewCause) -> Reason {
    match cause {
        ReviewCause::Detection(ReviewReason::TooLittleEvidence) => Reason::TooLittleEvidence,
        ReviewCause::Detection(ReviewReason::GuessDoesNotDecode) => Reason::GuessDoesNotDecode,
        ReviewCause::Detection(ReviewReason::LanguageHintDisagrees) => {
            Reason::LanguageHintDisagrees
        }
        ReviewCause::LooksLikeUtf16 => Reason::LooksLikeUtf16,
        ReviewCause::DamagedOrMixedUtf8 => Reason::DamagedOrMixedUtf8,
        ReviewCause::FolderAgrees(_) => Reason::FolderAgrees,
        ReviewCause::LooksGarbled(_) => Reason::LooksGarbled,
    }
}

fn describe_refusal(refusal: Refusal) -> Description {
    match refusal {
        Refusal::Empty => {
            Description::with_problem(StatusName::Skipped, Problem::of(Reason::Empty))
        }
        Refusal::NotText => {
            Description::with_problem(StatusName::Failed, Problem::of(Reason::NotText))
        }
        Refusal::DamagedUtf8 { location } => Description::with_problem(
            StatusName::Failed,
            Problem::at(Reason::DamagedUtf8, location),
        ),
        Refusal::NameNotDecodable => {
            Description::with_problem(StatusName::Skipped, Problem::of(Reason::NameNotDecodable))
        }
        Refusal::Read(problem) => describe_read_problem(problem),
    }
}

/// ENC-01, ENC-02, SAFE-11 and SAFE-12.
pub fn read_problem_description(problem: ReadProblem) -> (StatusName, Problem) {
    match problem {
        ReadProblem::Unreadable(kind) => (
            StatusName::Failed,
            Problem::system(Reason::Unreadable, kind),
        ),
        ReadProblem::TooLarge => (StatusName::Failed, Problem::of(Reason::TooLarge)),
        ReadProblem::NotRegularFile => (StatusName::Skipped, Problem::of(Reason::NotRegularFile)),
        ReadProblem::SymbolicLink => (StatusName::Skipped, Problem::of(Reason::SymbolicLink)),
    }
}

fn describe_read_problem(problem: ReadProblem) -> Description {
    let (status, problem) = read_problem_description(problem);
    Description::with_problem(status, problem)
}

fn describe_outcome(outcome: &Outcome) -> Description {
    match outcome {
        Outcome::Converted {
            output,
            warnings,
            structure_warnings,
        } => Description {
            output: Some(text_of(output)),
            warnings: warnings
                .iter()
                .map(|warning| conversion_warning_view(*warning))
                .chain(grouped_structure_warnings(structure_warnings))
                .collect(),
            ..Description::plain(StatusName::Converted)
        },
        Outcome::Skipped(cause) => {
            Description::with_problem(StatusName::Skipped, Problem::of(skip_reason(*cause)))
        }
        Outcome::Failed(cause) => {
            Description::with_problem(StatusName::Failed, failure_problem(*cause))
        }
        Outcome::NotReached => {
            Description::with_problem(StatusName::Skipped, Problem::of(Reason::Cancelled))
        }
    }
}

fn skip_reason(cause: SkipCause) -> Reason {
    match cause {
        SkipCause::AlreadyExists => Reason::AlreadyExists,
        SkipCause::NameTakenInList => Reason::NameTakenInList,
        SkipCause::ListedFile => Reason::ListedFile,
        SkipCause::SymbolicLink => Reason::SymbolicLink,
        SkipCause::ReadOnlyDestination => Reason::ReadOnlyDestination,
        SkipCause::FolderNotWritable => Reason::FolderNotWritable,
        SkipCause::NotRegularFile => Reason::NotRegularFile,
        SkipCause::NoFreeName => Reason::NoFreeName,
        SkipCause::NameNotDecodable => Reason::NameNotDecodable,
    }
}

fn failure_problem(cause: FailureCause) -> Problem {
    match cause {
        FailureCause::Unreadable(kind) => Problem::system(Reason::Unreadable, kind),
        FailureCause::TooLarge => Problem::of(Reason::TooLarge),
        FailureCause::Conversion(failure) => conversion_problem(failure),
        FailureCause::NameTooLong => Problem::of(Reason::NameTooLong),
        FailureCause::VerificationFailed => Problem::of(Reason::VerificationFailed),
        FailureCause::WriteFailed(kind) => Problem::system(Reason::WriteFailed, kind),
    }
}

fn conversion_problem(failure: ConversionFailure) -> Problem {
    let reason = match failure {
        ConversionFailure::DamagedUtf8 { .. } => Reason::DamagedUtf8,
        ConversionFailure::DamagedUtf16 { .. } => Reason::DamagedUtf16,
        ConversionFailure::DoesNotDecode(_) => Reason::DoesNotDecode,
        ConversionFailure::RoundTripDiffers { .. } => Reason::RoundTripDiffers,
        ConversionFailure::ControlCharactersInUtf8 { .. } => Reason::ControlCharactersInUtf8,
    };
    Problem::at(reason, failure.location())
}

fn conversion_warning_view(warning: ConversionWarning) -> WarningView {
    match warning {
        ConversionWarning::RoundTripDiffers { location } => WarningView {
            warning: WarningName::RoundTripDiffers,
            byte_offset: Some(location.byte_offset),
            line: Some(location.line),
            count: SINGLE_WARNING_COUNT,
        },
        ConversionWarning::ContainsReplacementCharacters => WarningView {
            warning: WarningName::ContainsReplacementCharacters,
            byte_offset: None,
            line: None,
            count: SINGLE_WARNING_COUNT,
        },
    }
}

fn structure_warning_view(warning: StructureWarning) -> WarningView {
    let (warning, line) = match warning {
        StructureWarning::NoCues => (WarningName::NoCues, None),
        StructureWarning::MissingCueNumber { line } => (WarningName::MissingCueNumber, Some(line)),
        StructureWarning::CueNumberOutOfOrder { line } => {
            (WarningName::CueNumberOutOfOrder, Some(line))
        }
        StructureWarning::MalformedTiming { line } => (WarningName::MalformedTiming, Some(line)),
        StructureWarning::TimingUsesDot { line } => (WarningName::TimingUsesDot, Some(line)),
    };
    WarningView {
        warning,
        byte_offset: None,
        line,
        count: SINGLE_WARNING_COUNT,
    }
}

/// ENC-19: each kind of warning shows once, at its first line, with how many there are, so a
/// file with a thousand cues cannot flood the page.
fn grouped_structure_warnings(warnings: &[StructureWarning]) -> Vec<WarningView> {
    let mut grouped: Vec<WarningView> = Vec::new();
    for view in warnings
        .iter()
        .map(|warning| structure_warning_view(*warning))
    {
        match grouped
            .iter_mut()
            .find(|group| group.warning == view.warning)
        {
            Some(group) => group.count += 1,
            None => grouped.push(view),
        }
    }
    grouped
}

/// SAFE-11 to SAFE-13 and NAME-01, for a path that cannot be browsed or added.
pub fn skip_reason_problem(reason: SkipReason) -> Problem {
    match reason {
        SkipReason::SymbolicLink => Problem::of(Reason::SymbolicLink),
        SkipReason::NotRegularFile => Problem::of(Reason::NotRegularFile),
        SkipReason::NotSrt => Problem::of(Reason::NotSrt),
        SkipReason::OutsideAllowedArea => Problem::of(Reason::OutsideAllowedArea),
        SkipReason::NotAFolder => Problem::of(Reason::NotAFolder),
        SkipReason::Unreadable(kind) => Problem::system(Reason::Unreadable, kind),
    }
}

/// ENC-12.
pub fn manual_choice_problem(refusal: ManualChoiceRefusal) -> Problem {
    match refusal {
        ManualChoiceRefusal::NotOffered => Problem::of(Reason::NotOffered),
        ManualChoiceRefusal::NotApplicable => Problem::of(Reason::NotApplicable),
        ManualChoiceRefusal::DoesNotDecode(problem) => {
            Problem::at(Reason::DoesNotDecode, problem.location())
        }
    }
}

pub fn listing_view(listing: Listing) -> ListingView {
    let raw_files = listing
        .raw_files
        .iter()
        .map(|name| RawEntryView {
            display: name.to_string_lossy().into_owned(),
            hex: encode_hex(listing.path.join(name).as_os_str().as_bytes()),
        })
        .collect();
    ListingView {
        path: text_of(&listing.path),
        parent: listing.parent.as_deref().map(text_of),
        folders: listing.folders,
        files: listing.files,
        raw_files,
        is_truncated: listing.is_truncated,
    }
}

pub fn added_view(added: usize, skipped: Vec<SkippedPath>, limit_reached: bool) -> AddedView {
    AddedView {
        added,
        skipped: skipped
            .into_iter()
            .map(|skipped| SkippedView {
                path: text_of(&skipped.path),
                problem: skip_reason_problem(skipped.reason),
            })
            .collect(),
        limit_reached,
    }
}

pub fn gathered_view(added: usize, gathered: Gathered) -> AddedView {
    added_view(added, gathered.skipped, gathered.limit_reached)
}

pub fn preview_view(encoding: Option<&'static Encoding>, preview: FilePreview) -> PreviewView {
    let (cues, problem, broken_line) = match preview.cues {
        Ok(cues) => (cues, None, None),
        Err(PreviewProblem::Read(problem)) => {
            (Vec::new(), Some(read_problem_description(problem).1), None)
        }
        Err(PreviewProblem::Conversion {
            failure,
            broken_line,
        }) => (
            Vec::new(),
            Some(conversion_problem(failure)),
            Some(BrokenLineView {
                line: broken_line.line,
                text: broken_line.text,
            }),
        ),
    };
    PreviewView {
        encoding: encoding.map(Encoding::name),
        cues: cues
            .into_iter()
            .map(|cue| CueView {
                number: cue.number,
                timing: cue.timing,
                text: cue.text,
                is_clipped: cue.is_clipped,
            })
            .collect(),
        problem,
        broken_line,
        repair: preview.repair_sample.map(|sample| RepairView {
            as_is: sample.as_is,
            repaired: sample.repaired,
        }),
    }
}

pub fn candidates_view(candidates: Vec<Candidate>) -> CandidatesView {
    CandidatesView {
        candidates: candidates
            .into_iter()
            .map(|candidate| CandidateView {
                encoding: candidate.encoding.name(),
                sample: candidate.sample,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ENC-19.
    #[test]
    fn structure_warnings_are_grouped_by_kind() {
        let warnings = [
            StructureWarning::MalformedTiming { line: 2 },
            StructureWarning::MissingCueNumber { line: 5 },
            StructureWarning::MalformedTiming { line: 6 },
            StructureWarning::MalformedTiming { line: 10 },
        ];
        let grouped: Vec<(WarningName, Option<usize>, usize)> =
            grouped_structure_warnings(&warnings)
                .into_iter()
                .map(|view| (view.warning, view.line, view.count))
                .collect();
        assert_eq!(
            grouped,
            [
                (WarningName::MalformedTiming, Some(2), 3),
                (WarningName::MissingCueNumber, Some(5), 1),
            ]
        );
    }
}
