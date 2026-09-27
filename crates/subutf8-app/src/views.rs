use std::io;
use std::path::Path;
use std::time::Duration;

use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use subutf8_core::batch::{CollisionPolicy, Destination, Origin};
use subutf8_core::decoding::{ConversionFailure, ConversionWarning};
use subutf8_core::detection::{ManualChoiceRefusal, ReviewReason};
use subutf8_core::input_scan::{SkipReason, SkippedPath};
use subutf8_core::preview::PreviewCue;
use subutf8_core::report::{FailureCause, Outcome, SkipCause};
use subutf8_core::srt_structure::StructureWarning;

use crate::defaults::{Defaults, DefaultsStore, StoreProblem};
use crate::folders::Listing;
use crate::history::HistoryRecord;
use crate::session::{
    FileEntry, FileStatus, Gathered, PreviewProblem, ReadProblem, Refusal, ReviewCause, Session,
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
    NameNotUtf8,
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
/// system's own words for a read or write error, or the file it concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_name: Option<String>,
}

impl Problem {
    pub fn of(reason: Reason) -> Self {
        Self {
            reason,
            byte_offset: None,
            system_reason: None,
            related_name: None,
        }
    }

    pub fn about(reason: Reason, name: &str) -> Self {
        Self {
            related_name: Some(name.to_owned()),
            ..Self::of(reason)
        }
    }

    fn at(reason: Reason, byte_offset: usize) -> Self {
        Self {
            byte_offset: Some(byte_offset),
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarningView {
    warning: WarningName,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    id: u64,
    name: String,
    /// The folder of a browsed file; dropped files have none (SAFE-02).
    folder: Option<String>,
    status: StatusName,
    encoding: Option<&'static str>,
    problem: Option<Problem>,
    output: Option<String>,
    warnings: Vec<WarningView>,
    can_choose_encoding: bool,
    /// UI-05: the next conversion includes this file.
    is_pending: bool,
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

/// UI-05: the bottom bar, as shown and as sent back when changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsView {
    pub language: Option<String>,
    pub destination: DestinationName,
    pub output_folder: String,
    pub organise_by_day: bool,
    pub collision_policy: CollisionName,
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
    settings: SettingsView,
    conversion: Option<ProgressView>,
    browse_start: String,
    /// The folders the app may use; in Docker, the mounted ones (TARGET-02).
    roots: Vec<String>,
    /// The desktop app shows itself in its own window, where closing the window quits.
    window_open: bool,
    /// SET-01: what the Settings dialog shows.
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
    is_truncated: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewView {
    encoding: Option<&'static str>,
    cues: Vec<CueView>,
    problem: Option<Problem>,
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl From<Destination> for DestinationName {
    fn from(destination: Destination) -> Self {
        match destination {
            Destination::BesideOriginals => Self::BesideOriginals,
            Destination::OutputFolder => Self::OutputFolder,
        }
    }
}

impl From<DestinationName> for Destination {
    fn from(destination: DestinationName) -> Self {
        match destination {
            DestinationName::BesideOriginals => Self::BesideOriginals,
            DestinationName::OutputFolder => Self::OutputFolder,
        }
    }
}

impl From<CollisionPolicy> for CollisionName {
    fn from(policy: CollisionPolicy) -> Self {
        match policy {
            CollisionPolicy::Skip => Self::Skip,
            CollisionPolicy::Rename => Self::Rename,
            CollisionPolicy::Overwrite => Self::Overwrite,
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
        settings: SettingsView {
            language: session
                .settings
                .language
                .as_ref()
                .map(|language| language.tag().to_owned()),
            destination: session.settings.destination.into(),
            output_folder: text_of(&session.settings.output_folder),
            organise_by_day: session.settings.organise_by_day,
            collision_policy: session.settings.collision_policy.into(),
        },
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
    FileView {
        id: file.id,
        name: file.display_name(),
        folder,
        status: description.status,
        encoding: file.current_encoding().map(Encoding::name),
        problem: description.problem,
        output: description.output,
        warnings: description.warnings,
        can_choose_encoding: file.can_choose_encoding(),
        is_pending: file.pending_encoding().is_some(),
    }
}

fn describe(status: FileStatus<'_>) -> Description {
    match status {
        FileStatus::Ready(_) => Description::plain(StatusName::Ready),
        FileStatus::NeedsReview { cause, .. } => {
            Description::with_problem(StatusName::NeedsReview, Problem::of(review_reason(cause)))
        }
        FileStatus::Refused(refusal) => describe_refusal(refusal),
        FileStatus::Finished(outcome) => describe_outcome(outcome),
        FileStatus::ConvertedCopy(original) => Description::with_problem(
            StatusName::Skipped,
            Problem::about(Reason::ConvertedCopy, original),
        ),
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
        Refusal::DamagedUtf8 { byte_offset } => Description::with_problem(
            StatusName::Failed,
            Problem::at(Reason::DamagedUtf8, byte_offset),
        ),
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
                .chain(
                    structure_warnings
                        .iter()
                        .map(|warning| structure_warning_view(*warning)),
                )
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
    match failure {
        ConversionFailure::DamagedUtf8 { byte_offset } => {
            Problem::at(Reason::DamagedUtf8, byte_offset)
        }
        ConversionFailure::DamagedUtf16 { byte_offset } => {
            Problem::at(Reason::DamagedUtf16, byte_offset)
        }
        ConversionFailure::DoesNotDecode(problem) => {
            Problem::at(Reason::DoesNotDecode, problem.byte_offset())
        }
        ConversionFailure::RoundTripDiffers { byte_offset } => {
            Problem::at(Reason::RoundTripDiffers, byte_offset)
        }
    }
}

fn conversion_warning_view(warning: ConversionWarning) -> WarningView {
    match warning {
        ConversionWarning::RoundTripDiffers { byte_offset } => WarningView {
            warning: WarningName::RoundTripDiffers,
            byte_offset: Some(byte_offset),
            line: None,
        },
        ConversionWarning::ContainsReplacementCharacters => WarningView {
            warning: WarningName::ContainsReplacementCharacters,
            byte_offset: None,
            line: None,
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
    };
    WarningView {
        warning,
        byte_offset: None,
        line,
    }
}

/// SAFE-11 to SAFE-13 and NAME-01, for a path that cannot be browsed or added.
pub fn skip_reason_problem(reason: SkipReason) -> Problem {
    match reason {
        SkipReason::SymbolicLink => Problem::of(Reason::SymbolicLink),
        SkipReason::NotRegularFile => Problem::of(Reason::NotRegularFile),
        SkipReason::NotSrt => Problem::of(Reason::NotSrt),
        SkipReason::OutsideAllowedArea => Problem::of(Reason::OutsideAllowedArea),
        SkipReason::NotAFolder => Problem::of(Reason::NotAFolder),
        SkipReason::NameNotUtf8 => Problem::of(Reason::NameNotUtf8),
        SkipReason::Unreadable(kind) => Problem::system(Reason::Unreadable, kind),
    }
}

/// ENC-12.
pub fn manual_choice_problem(refusal: ManualChoiceRefusal) -> Problem {
    match refusal {
        ManualChoiceRefusal::NotOffered => Problem::of(Reason::NotOffered),
        ManualChoiceRefusal::NotApplicable => Problem::of(Reason::NotApplicable),
        ManualChoiceRefusal::DoesNotDecode(problem) => {
            Problem::at(Reason::DoesNotDecode, problem.byte_offset())
        }
    }
}

pub fn listing_view(listing: Listing) -> ListingView {
    ListingView {
        path: text_of(&listing.path),
        parent: listing.parent.as_deref().map(text_of),
        folders: listing.folders,
        files: listing.files,
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

pub fn preview_view(
    encoding: Option<&'static Encoding>,
    preview: Result<Vec<PreviewCue>, PreviewProblem>,
) -> PreviewView {
    let (cues, problem) = match preview {
        Ok(cues) => (cues, None),
        Err(PreviewProblem::Read(problem)) => {
            (Vec::new(), Some(read_problem_description(problem).1))
        }
        Err(PreviewProblem::Conversion(failure)) => (Vec::new(), Some(conversion_problem(failure))),
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
    }
}
