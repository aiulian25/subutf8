use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use encoding_rs::{Encoding, UTF_8};
use subutf8_core::batch::{
    BatchSettings, CollisionPolicy, ConversionJob, Destination, Origin, run_batch,
};
use subutf8_core::classification::{Classification, classify};
use subutf8_core::constants::{MAXIMUM_FILE_BYTES, MAXIMUM_LISTED_FILES};
use subutf8_core::decoding::{ConversionFailure, convert};
use subutf8_core::detection::{
    Detection, ManualChoiceRefusal, ReviewReason, check_manual_choice, detect, takes_manual_choice,
};
use subutf8_core::input_scan::{
    AllowedArea, ListedFile, Scan, SkippedPath, check_chosen_file, scan_folder,
};
use subutf8_core::language::SubtitleLanguage;
use subutf8_core::preview::{PreviewCue, preview};
use subutf8_core::report::Outcome;
use subutf8_core::source_file::{ReadError, read_source};

use crate::constants::MAXIMUM_DROPPED_BYTES;

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
    pub chosen_encoding: Option<&'static Encoding>,
    pub outcome: Option<Outcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCause {
    Detection(ReviewReason),
    LooksLikeUtf16,
    DamagedOrMixedUtf8,
}

/// Why a file cannot be converted at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Empty,
    NotText,
    DamagedUtf8 { byte_offset: usize },
    Read(ReadProblem),
}

/// UI-02: a file's status, worked out from what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus<'entry> {
    Ready(&'static Encoding),
    NeedsReview {
        suggestion: Option<&'static Encoding>,
        cause: ReviewCause,
    },
    Finished(&'entry Outcome),
    Refused(Refusal),
}

impl FileEntry {
    pub fn status(&self) -> FileStatus<'_> {
        match &self.outcome {
            Some(outcome) => FileStatus::Finished(outcome),
            None => self.waiting_status(),
        }
    }

    /// What converting the file now would do, whatever happened in an earlier run.
    fn waiting_status(&self) -> FileStatus<'static> {
        let classification = match self.classification {
            Ok(classification) => classification,
            Err(problem) => return FileStatus::Refused(Refusal::Read(problem)),
        };
        if let (Some(encoding), true) = (self.chosen_encoding, takes_manual_choice(classification))
        {
            return FileStatus::Ready(encoding);
        }
        status_of(classification, self.detection)
    }

    /// UI-05: a converted file is done; a skipped or failed one is tried again, for example
    /// after the collision setting changes or the folder becomes writable.
    fn is_retryable(&self) -> bool {
        !matches!(self.outcome, Some(Outcome::Converted { .. }))
    }

    /// The encoding the next conversion uses for this file, when it includes the file.
    pub fn pending_encoding(&self) -> Option<&'static Encoding> {
        if !self.is_retryable() {
            return None;
        }
        match self.waiting_status() {
            FileStatus::Ready(encoding) => Some(encoding),
            _ => None,
        }
    }

    /// ENC-12: a hand-chosen encoding applies to any file not yet converted.
    pub fn can_choose_encoding(&self) -> bool {
        self.is_retryable() && self.classification.is_ok_and(takes_manual_choice)
    }

    /// Adding a listed file again reads it afresh and makes it ready to convert again.
    fn refresh(&mut self, prepared: PreparedFile) {
        self.origin = prepared.origin;
        self.classification = prepared.classification;
        self.detection = prepared.detection;
        self.chosen_encoding = None;
        self.outcome = None;
    }

    /// UI-04: the encoding the preview decodes with, also after the file is converted.
    pub fn current_encoding(&self) -> Option<&'static Encoding> {
        let classification = self.classification.ok()?;
        if takes_manual_choice(classification) && self.chosen_encoding.is_some() {
            return self.chosen_encoding;
        }
        match status_of(classification, self.detection) {
            FileStatus::Ready(encoding) => Some(encoding),
            FileStatus::NeedsReview { suggestion, .. } => suggestion,
            FileStatus::Finished(_) | FileStatus::Refused(_) => None,
        }
    }

    pub fn is_dropped(&self) -> bool {
        matches!(self.origin, Origin::Dropped { .. })
    }

    pub fn display_name(&self) -> String {
        match &self.origin {
            Origin::Disk { path, .. } => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Origin::Dropped { name, .. } => name.clone(),
        }
    }
}

fn status_of(classification: Classification, detection: Option<Detection>) -> FileStatus<'static> {
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
            FileStatus::Ready(UTF_8)
        }
        Classification::DamagedUtf8WithByteOrderMark { byte_offset } => {
            FileStatus::Refused(Refusal::DamagedUtf8 { byte_offset })
        }
        Classification::Utf16WithByteOrderMark(encoding) => FileStatus::Ready(encoding),
        Classification::NotText => FileStatus::Refused(Refusal::NotText),
        Classification::LooksLikeUtf16WithoutByteOrderMark(encoding) => FileStatus::NeedsReview {
            suggestion: Some(encoding),
            cause: ReviewCause::LooksLikeUtf16,
        },
        Classification::DamagedOrMixedUtf8 => FileStatus::NeedsReview {
            suggestion,
            cause: ReviewCause::DamagedOrMixedUtf8,
        },
        Classification::NeedsDetection => match detection {
            Some(Detection::Certain(encoding)) => FileStatus::Ready(encoding),
            Some(Detection::NeedsReview { suggestion, reason }) => FileStatus::NeedsReview {
                suggestion: Some(suggestion),
                cause: ReviewCause::Detection(reason),
            },
            None => FileStatus::NeedsReview {
                suggestion: None,
                cause: ReviewCause::Detection(ReviewReason::GuessDoesNotDecode),
            },
        },
    }
}

/// The settings shown in the bottom bar (UI-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSettings {
    pub language: Option<SubtitleLanguage>,
    pub destination: Destination,
    pub output_folder: PathBuf,
    pub collision_policy: CollisionPolicy,
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
    /// `on_outcome` receives the number of files finished so far and the latest outcome.
    pub fn run(&self, mut on_outcome: impl FnMut(usize, &Outcome)) {
        run_batch(
            &self.jobs,
            &self.settings,
            &self.listed_paths,
            &self.cancel,
            |progress, outcome| on_outcome(progress.finished, outcome),
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
    origin: Origin,
    classification: Result<Classification, ReadProblem>,
    detection: Option<Detection>,
}

/// Reads, classifies and detects a file before it is listed (ENC-01 to ENC-10).
pub fn prepare(origin: Origin, language: Option<&SubtitleLanguage>) -> PreparedFile {
    let bytes = origin_bytes(&origin);
    let classification = bytes.as_deref().map(classify).map_err(|problem| *problem);
    let detection = match (&bytes, classification) {
        (Ok(bytes), Ok(Classification::NeedsDetection | Classification::DamagedOrMixedUtf8)) => {
            Some(detect(bytes, language))
        }
        _ => None,
    };
    PreparedFile {
        origin,
        classification,
        detection,
    }
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
    gathered
}

/// ENC-13: detection runs again when the subtitle language changes.
pub fn detect_again(
    files: Vec<(u64, Origin)>,
    language: Option<&SubtitleLanguage>,
) -> Vec<(u64, Detection)> {
    files
        .into_iter()
        .filter_map(|(id, origin)| {
            let bytes = origin_bytes(&origin).ok()?;
            Some((id, detect(&bytes, language)))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewProblem {
    Read(ReadProblem),
    Conversion(ConversionFailure),
}

/// UI-04: the preview decodes exactly as the conversion would.
pub fn preview_file(
    origin: &Origin,
    encoding: &'static Encoding,
) -> Result<Vec<PreviewCue>, PreviewProblem> {
    let bytes = origin_bytes(origin).map_err(PreviewProblem::Read)?;
    let conversion = convert(&bytes, encoding).map_err(PreviewProblem::Conversion)?;
    Ok(preview(&conversion.text))
}

/// The bytes of a file: read again from disk, or held in memory for a dropped file.
pub fn origin_bytes(origin: &Origin) -> Result<Vec<u8>, ReadProblem> {
    match origin {
        Origin::Dropped { bytes, .. } => Ok(bytes.to_vec()),
        Origin::Disk { path, .. } => read_source(path).map_err(|error| match error {
            ReadError::Unreadable(error) => ReadProblem::Unreadable(error.kind()),
            ReadError::TooLarge { .. } => ReadProblem::TooLarge,
            ReadError::NotRegularFile => ReadProblem::NotRegularFile,
            ReadError::SymbolicLink => ReadProblem::SymbolicLink,
        }),
    }
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
                chosen_encoding: None,
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

    /// ENC-12: applies a hand-chosen encoding after strict decoding succeeds.
    pub fn choose_encoding(
        &mut self,
        id: u64,
        encoding: &'static Encoding,
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
        check_manual_choice(bytes, classification, encoding)
            .map_err(SessionProblem::ManualChoice)?;
        file.chosen_encoding = Some(encoding);
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

    /// Files whose detection depends on the language, to detect again when it changes.
    pub fn detection_dependent(&self) -> Vec<(u64, Origin)> {
        self.files
            .iter()
            .filter(|file| file.outcome.is_none() && file.detection.is_some())
            .map(|file| (file.id, file.origin.clone()))
            .collect()
    }

    pub fn set_detection(&mut self, id: u64, detection: Detection) {
        if let Ok(file) = self.find_mut(id) {
            file.detection = Some(detection);
        }
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
            .any(|file| file.is_dropped() && file.pending_encoding().is_some());
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
                let encoding = file.pending_encoding()?;
                let job = ConversionJob {
                    origin: file.origin.clone(),
                    encoding,
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
        let settings = BatchSettings {
            language: self.settings.language.clone(),
            destination: self.settings.destination,
            output_folder: self.settings.output_folder.clone(),
            collision_policy: self.settings.collision_policy,
        };
        Ok(ConversionStart {
            jobs,
            settings,
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
    use std::path::Path;
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
            collision_policy: CollisionPolicy::Skip,
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
        ];
        let prepared = names
            .iter()
            .map(|name| prepare(fixture(name), None))
            .collect();
        assert_eq!(session.add(prepared), names.len());
        let statuses: Vec<FileStatus> = session.files.iter().map(FileEntry::status).collect();
        assert_eq!(statuses[0], FileStatus::Ready(WINDOWS_1250));
        assert!(matches!(
            statuses[1],
            FileStatus::NeedsReview {
                cause: ReviewCause::Detection(ReviewReason::TooLittleEvidence),
                ..
            }
        ));
        assert_eq!(statuses[2], FileStatus::Ready(UTF_8));
        assert_eq!(statuses[3], FileStatus::Ready(UTF_8));
        assert_eq!(
            statuses[4],
            FileStatus::NeedsReview {
                suggestion: Some(UTF_16LE),
                cause: ReviewCause::LooksLikeUtf16
            }
        );
        assert_eq!(statuses[5], FileStatus::Refused(Refusal::NotText));
        assert_eq!(statuses[6], FileStatus::Refused(Refusal::Empty));
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
        session.files[0].chosen_encoding = Some(WINDOWS_1250);
        assert_eq!(session.add(second), 2);
        assert_eq!(session.files.len(), 1);
        assert_eq!(session.files[0].outcome, None);
        assert_eq!(session.files[0].chosen_encoding, None);
    }

    /// UI-05: skipped and failed files are converted again; converted ones are done.
    #[test]
    fn only_converted_files_leave_the_next_conversion() {
        let mut session = session();
        session.add(vec![prepare(fixture("windows-1250-romanian"), None)]);
        let file = &mut session.files[0];
        file.outcome = Some(Outcome::Skipped(SkipCause::AlreadyExists));
        assert_eq!(file.pending_encoding(), Some(WINDOWS_1250));
        file.outcome = Some(Outcome::Failed(FailureCause::VerificationFailed));
        assert_eq!(file.pending_encoding(), Some(WINDOWS_1250));
        assert!(file.can_choose_encoding());
        file.outcome = Some(Outcome::Converted {
            output: PathBuf::from("/tmp/out.srt"),
            warnings: Vec::new(),
            structure_warnings: Vec::new(),
        });
        assert_eq!(file.pending_encoding(), None);
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
            .choose_encoding(short_id, WINDOWS_1250, &short_bytes)
            .unwrap();
        assert_eq!(
            session.find(short_id).unwrap().status(),
            FileStatus::Ready(WINDOWS_1250)
        );
        let utf8_bytes = origin_bytes(&session.files[1].origin).unwrap();
        assert_eq!(
            session.choose_encoding(utf8_id, WINDOWS_1250, &utf8_bytes),
            Err(SessionProblem::ManualChoice(
                ManualChoiceRefusal::NotApplicable
            ))
        );
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
}
