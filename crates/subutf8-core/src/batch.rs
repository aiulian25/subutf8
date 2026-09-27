use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use encoding_rs::Encoding;
use tempfile::Builder;

use crate::constants::{
    MAXIMUM_LISTED_FILES, TEMPORARY_FILE_PREFIX, TEMPORARY_FILE_SUFFIX, UTF8_BYTE_ORDER_MARK,
};
use crate::decoding::convert;
use crate::language::SubtitleLanguage;
use crate::output_naming::{Placement, candidate_names, check_name_length};
use crate::report::{FailureCause, Outcome, SkipCause};
use crate::safe_write::{WriteError, WriteMode, output_permissions, write_verified};
use crate::source_file::{ReadError, read_source};
use crate::srt_structure::check_structure;

/// SAFE-02: where browsed files go. Dropped files always go to the output folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    BesideOriginals,
    OutputFolder,
}

/// SAFE-04.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionPolicy {
    Skip,
    Rename,
    Overwrite,
}

/// NAME-10: the folder for one day's outputs, such as `2026-09-27`. Built from numbers only, so
/// it is always one plain folder name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayFolder(String);

impl DayFolder {
    pub fn new(year: i16, month: i8, day: i8) -> Self {
        Self(format!("{year:04}-{month:02}-{day:02}"))
    }

    pub fn name(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchSettings {
    pub language: Option<SubtitleLanguage>,
    pub destination: Destination,
    pub output_folder: PathBuf,
    /// NAME-10: outputs in the output folder go into this day's folder.
    pub day_folder: Option<DayFolder>,
    pub collision_policy: CollisionPolicy,
}

impl BatchSettings {
    fn output_folder(&self) -> PathBuf {
        match &self.day_folder {
            Some(day) => self.output_folder.join(day.name()),
            None => self.output_folder.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A browsed file, read from disk when it is converted.
    Disk {
        path: PathBuf,
        relative_folder: PathBuf,
    },
    /// A dropped file, held in memory only (SAFE-15).
    Dropped { name: String, bytes: Arc<[u8]> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionJob {
    pub origin: Origin,
    /// The detected or chosen encoding; a byte-order mark overrides it (ENC-12).
    pub encoding: &'static Encoding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub finished: usize,
    pub total: usize,
}

struct PlannedWrite {
    destination: PathBuf,
    write_mode: WriteMode,
}

/// Converts every job in order (SAFE-05 to SAFE-07, UI-06). Destinations are all worked out
/// before anything is written; `listed_paths` are the files in the list, which overwrite never
/// replaces; `cancel` stops the batch between files. One failure never stops the rest.
pub fn run_batch(
    jobs: &[ConversionJob],
    settings: &BatchSettings,
    listed_paths: &HashSet<PathBuf>,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress, &Outcome),
) -> Vec<Outcome> {
    let plans = plan_destinations(jobs, settings, listed_paths);
    let total = jobs.len();
    let mut outcomes = Vec::with_capacity(total);
    for (job, plan) in jobs.iter().zip(plans) {
        let outcome = match plan {
            _ if cancel.load(Ordering::Relaxed) => Outcome::NotReached,
            Ok(planned) => convert_and_write(job, &planned),
            Err(outcome) => outcome,
        };
        let progress = Progress {
            finished: outcomes.len() + 1,
            total,
        };
        on_progress(progress, &outcome);
        outcomes.push(outcome);
    }
    outcomes
}

fn plan_destinations(
    jobs: &[ConversionJob],
    settings: &BatchSettings,
    listed_paths: &HashSet<PathBuf>,
) -> Vec<Result<PlannedWrite, Outcome>> {
    let mut taken = HashSet::new();
    let mut writable_folders = HashMap::new();
    jobs.iter()
        .map(|job| {
            let (folder, name, placement) = destination_parts(job, settings);
            let is_writable = *writable_folders
                .entry(folder.clone())
                .or_insert_with(|| folder_is_writable(&folder));
            if !is_writable {
                return Err(Outcome::Skipped(SkipCause::FolderNotWritable));
            }
            let planned = choose_name(&folder, &name, placement, settings, listed_paths, &taken)?;
            taken.insert(planned.destination.clone());
            Ok(planned)
        })
        .collect()
}

fn destination_parts(
    job: &ConversionJob,
    settings: &BatchSettings,
) -> (PathBuf, String, Placement) {
    match &job.origin {
        Origin::Disk { path, .. } if settings.destination == Destination::BesideOriginals => (
            path.parent().map(Path::to_path_buf).unwrap_or_default(),
            file_name(path),
            Placement::BesideOriginal,
        ),
        Origin::Disk {
            path,
            relative_folder,
        } => (
            settings.output_folder().join(relative_folder),
            file_name(path),
            Placement::OutputFolder,
        ),
        Origin::Dropped { name, .. } => (
            settings.output_folder(),
            name.clone(),
            Placement::OutputFolder,
        ),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// SAFE-03: a folder that does not exist yet is checked through its nearest existing parent,
/// by creating and removing a temporary file exactly as a real write would.
pub fn folder_is_writable(folder: &Path) -> bool {
    let Some(existing) = folder.ancestors().find(|ancestor| ancestor.is_dir()) else {
        return false;
    };
    Builder::new()
        .prefix(TEMPORARY_FILE_PREFIX)
        .suffix(TEMPORARY_FILE_SUFFIX)
        .tempfile_in(existing)
        .is_ok()
}

/// SAFE-04 and SAFE-05: an earlier file in the list keeps a name, and later ones are
/// treated as if it already existed.
fn choose_name(
    folder: &Path,
    original_name: &str,
    placement: Placement,
    settings: &BatchSettings,
    listed_paths: &HashSet<PathBuf>,
    taken: &HashSet<PathBuf>,
) -> Result<PlannedWrite, Outcome> {
    let mut candidates = candidate_names(original_name, settings.language.as_ref(), placement)
        .map_err(|_| Outcome::Failed(FailureCause::NameTooLong))?
        .map(|name| folder.join(name));
    let first = candidates.next().unwrap_or_default();
    check_length(&first)?;
    let is_free = |path: &PathBuf| !taken.contains(path) && fs::symlink_metadata(path).is_err();
    match settings.collision_policy {
        CollisionPolicy::Skip if is_free(&first) => Ok(new_file(first)),
        CollisionPolicy::Skip => Err(Outcome::Skipped(SkipCause::AlreadyExists)),
        CollisionPolicy::Rename => std::iter::once(first)
            .chain(candidates)
            .take(MAXIMUM_LISTED_FILES)
            .find(is_free)
            .ok_or(Outcome::Skipped(SkipCause::NoFreeName))
            .and_then(|path| check_length(&path).map(|()| new_file(path))),
        CollisionPolicy::Overwrite if taken.contains(&first) => {
            Err(Outcome::Skipped(SkipCause::NameTakenInList))
        }
        CollisionPolicy::Overwrite if is_listed(&first, listed_paths) => {
            Err(Outcome::Skipped(SkipCause::ListedFile))
        }
        CollisionPolicy::Overwrite => Ok(PlannedWrite {
            destination: first,
            write_mode: WriteMode::Replace,
        }),
    }
}

fn new_file(destination: PathBuf) -> PlannedWrite {
    PlannedWrite {
        destination,
        write_mode: WriteMode::CreateNew,
    }
}

fn check_length(path: &Path) -> Result<(), Outcome> {
    check_name_length(&file_name(path)).map_err(|_| Outcome::Failed(FailureCause::NameTooLong))
}

fn is_listed(path: &Path, listed_paths: &HashSet<PathBuf>) -> bool {
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    listed_paths.contains(&real) || listed_paths.contains(path)
}

fn convert_and_write(job: &ConversionJob, planned: &PlannedWrite) -> Outcome {
    let (bytes, original) = match read_origin(&job.origin) {
        Ok(read) => read,
        Err(outcome) => return outcome,
    };
    let conversion = match convert(&bytes, job.encoding) {
        Ok(conversion) => conversion,
        Err(failure) => return Outcome::Failed(FailureCause::Conversion(failure)),
    };
    let folder = planned.destination.parent().unwrap_or(Path::new(""));
    if let Err(error) = fs::create_dir_all(folder) {
        return Outcome::Failed(FailureCause::WriteFailed(error.kind()));
    }
    let permissions = output_permissions(original.as_ref());
    let output = [UTF8_BYTE_ORDER_MARK, &conversion.text].concat();
    let written = write_verified(
        &planned.destination,
        &output,
        permissions,
        planned.write_mode,
    );
    match written {
        Ok(()) => Outcome::Converted {
            output: planned.destination.clone(),
            structure_warnings: check_structure(&conversion.text),
            warnings: conversion.warnings,
        },
        Err(error) => write_error_outcome(error),
    }
}

fn read_origin(origin: &Origin) -> Result<(Vec<u8>, Option<fs::Metadata>), Outcome> {
    match origin {
        Origin::Dropped { bytes, .. } => Ok((bytes.to_vec(), None)),
        Origin::Disk { path, .. } => {
            let bytes = read_source(path).map_err(read_error_outcome)?;
            Ok((bytes, fs::metadata(path).ok()))
        }
    }
}

fn read_error_outcome(error: ReadError) -> Outcome {
    match error {
        ReadError::Unreadable(error) => Outcome::Failed(FailureCause::Unreadable(error.kind())),
        ReadError::TooLarge { .. } => Outcome::Failed(FailureCause::TooLarge),
        ReadError::NotRegularFile => Outcome::Skipped(SkipCause::NotRegularFile),
        ReadError::SymbolicLink => Outcome::Skipped(SkipCause::SymbolicLink),
    }
}

fn write_error_outcome(error: WriteError) -> Outcome {
    match error {
        WriteError::AlreadyExists => Outcome::Skipped(SkipCause::AlreadyExists),
        WriteError::DestinationIsSymbolicLink => Outcome::Skipped(SkipCause::SymbolicLink),
        WriteError::DestinationIsReadOnly => Outcome::Skipped(SkipCause::ReadOnlyDestination),
        WriteError::VerificationFailed => Outcome::Failed(FailureCause::VerificationFailed),
        WriteError::Io(error) => Outcome::Failed(FailureCause::WriteFailed(error.kind())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoding::{ConversionFailure, DecodingProblem};
    use crate::detection::{Detection, detect};
    use crate::report::Summary;
    use crate::test_fixtures::{expected, source, source_path};
    use encoding_rs::{ISO_8859_2, UTF_8, WINDOWS_1250};
    use std::os::unix::fs::PermissionsExt;

    const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/golden.sha256");

    fn settings(output_folder: &Path, collision_policy: CollisionPolicy) -> BatchSettings {
        BatchSettings {
            language: None,
            destination: Destination::OutputFolder,
            output_folder: output_folder.to_path_buf(),
            day_folder: None,
            collision_policy,
        }
    }

    fn disk_job(path: &Path, encoding: &'static Encoding) -> ConversionJob {
        ConversionJob {
            origin: Origin::Disk {
                path: path.to_path_buf(),
                relative_folder: PathBuf::new(),
            },
            encoding,
        }
    }

    fn dropped_job(name: &str, fixture: &str, encoding: &'static Encoding) -> ConversionJob {
        ConversionJob {
            origin: Origin::Dropped {
                name: String::from(name),
                bytes: Arc::from(source(fixture)),
            },
            encoding,
        }
    }

    fn run(jobs: &[ConversionJob], settings: &BatchSettings) -> Vec<Outcome> {
        run_batch(
            jobs,
            settings,
            &HashSet::new(),
            &AtomicBool::new(false),
            |_, _| {},
        )
    }

    /// ENC-14: what a converted fixture's output holds.
    fn expected_output(fixture: &str) -> String {
        [UTF8_BYTE_ORDER_MARK, &expected(fixture)].concat()
    }

    fn output_names(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// TARGET-01: the parity set converts to exactly the outputs listed in golden.sha256.
    #[test]
    fn parity_set_matches_the_golden_list() {
        let output = tempfile::tempdir().unwrap();
        let golden = fs::read_to_string(GOLDEN).unwrap();
        let names: Vec<&str> = golden
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1))
            .collect();
        let jobs: Vec<ConversionJob> = names
            .iter()
            .map(|name| {
                let fixture = name.trim_end_matches(".srt");
                let bytes = source(fixture);
                let encoding = match detect(&bytes, None) {
                    Detection::Certain(encoding) => encoding,
                    Detection::NeedsReview { .. } => UTF_8,
                };
                disk_job(&source_path(fixture), encoding)
            })
            .collect();
        let outcomes = run(&jobs, &settings(output.path(), CollisionPolicy::Skip));
        assert_eq!(Summary::of(&outcomes).converted, names.len());
        for name in names {
            let written = fs::read_to_string(output.path().join(name)).unwrap();
            assert_eq!(
                written,
                expected_output(name.trim_end_matches(".srt")),
                "{name}"
            );
        }
    }

    /// SAFE-07 and UI-07.
    #[test]
    fn batch_continues_after_failure() {
        let output = tempfile::tempdir().unwrap();
        let jobs = [
            disk_job(&source_path("windows-1250-romanian"), WINDOWS_1250),
            disk_job(&source_path("markup"), ISO_8859_2),
            disk_job(&source_path("nonstandard"), WINDOWS_1250),
        ];
        let outcomes = run(&jobs, &settings(output.path(), CollisionPolicy::Skip));
        assert!(matches!(outcomes[0], Outcome::Converted { .. }));
        assert!(matches!(
            outcomes[1],
            Outcome::Failed(FailureCause::Conversion(ConversionFailure::DoesNotDecode(
                DecodingProblem::ControlCharacter { .. }
            )))
        ));
        assert!(matches!(outcomes[2], Outcome::Converted { .. }));
        assert_eq!(
            output_names(output.path()),
            ["nonstandard.srt", "windows-1250-romanian.srt"]
        );
    }

    /// SAFE-04 and NAME-06.
    #[test]
    fn collision_rename() {
        let output = tempfile::tempdir().unwrap();
        fs::write(output.path().join("Film.srt"), "existing").unwrap();
        let jobs = [dropped_job(
            "Film.srt",
            "windows-1250-romanian",
            WINDOWS_1250,
        )];
        let outcomes = run(&jobs, &settings(output.path(), CollisionPolicy::Rename));
        assert_eq!(
            outcomes[0],
            Outcome::Converted {
                output: output.path().join("Film1.srt"),
                warnings: Vec::new(),
                structure_warnings: Vec::new()
            }
        );
        assert_eq!(
            fs::read_to_string(output.path().join("Film.srt")).unwrap(),
            "existing"
        );
    }

    /// SAFE-05.
    #[test]
    fn duplicate_names_do_not_collide() {
        let output = tempfile::tempdir().unwrap();
        let jobs = [
            dropped_job("Film.srt", "windows-1250-romanian", WINDOWS_1250),
            dropped_job("Film.srt", "nonstandard", WINDOWS_1250),
        ];
        let skipped = run(&jobs, &settings(output.path(), CollisionPolicy::Skip));
        assert_eq!(skipped[1], Outcome::Skipped(SkipCause::AlreadyExists));
        let renamed_output = tempfile::tempdir().unwrap();
        run(
            &jobs,
            &settings(renamed_output.path(), CollisionPolicy::Rename),
        );
        assert_eq!(
            output_names(renamed_output.path()),
            ["Film.srt", "Film1.srt"]
        );
        let overwritten_output = tempfile::tempdir().unwrap();
        let overwritten = run(
            &jobs,
            &settings(overwritten_output.path(), CollisionPolicy::Overwrite),
        );
        assert_eq!(overwritten[1], Outcome::Skipped(SkipCause::NameTakenInList));
        assert_eq!(
            fs::read_to_string(overwritten_output.path().join("Film.srt")).unwrap(),
            expected_output("windows-1250-romanian")
        );
    }

    /// SAFE-04: an output folder that is the original's own folder cannot replace it.
    #[test]
    fn overwrite_never_replaces_a_listed_file() {
        let folder = tempfile::tempdir().unwrap();
        let original = folder.path().join("Film.srt");
        fs::write(&original, source("windows-1250-romanian")).unwrap();
        let listed = HashSet::from([fs::canonicalize(&original).unwrap()]);
        let outcomes = run_batch(
            &[disk_job(&original, WINDOWS_1250)],
            &settings(folder.path(), CollisionPolicy::Overwrite),
            &listed,
            &AtomicBool::new(false),
            |_, _| {},
        );
        assert_eq!(outcomes[0], Outcome::Skipped(SkipCause::ListedFile));
        assert_eq!(
            fs::read(&original).unwrap(),
            source("windows-1250-romanian")
        );
    }

    /// SAFE-03: nothing is attempted in a folder that cannot be written.
    #[test]
    fn read_only_folder_is_flagged_before_converting() {
        let folder = tempfile::tempdir().unwrap();
        let original = folder.path().join("Film.srt");
        fs::write(&original, source("windows-1250-romanian")).unwrap();
        fs::set_permissions(folder.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let mut beside = settings(folder.path(), CollisionPolicy::Skip);
        beside.destination = Destination::BesideOriginals;
        let outcomes = run(&[disk_job(&original, WINDOWS_1250)], &beside);
        fs::set_permissions(folder.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(outcomes[0], Outcome::Skipped(SkipCause::FolderNotWritable));
        assert_eq!(output_names(folder.path()), ["Film.srt"]);
    }

    /// UI-06.
    #[test]
    fn cancellation_stops_between_files() {
        let output = tempfile::tempdir().unwrap();
        let jobs = [
            dropped_job("a.srt", "windows-1250-romanian", WINDOWS_1250),
            dropped_job("b.srt", "windows-1250-romanian", WINDOWS_1250),
            dropped_job("c.srt", "windows-1250-romanian", WINDOWS_1250),
        ];
        let cancel = AtomicBool::new(false);
        let outcomes = run_batch(
            &jobs,
            &settings(output.path(), CollisionPolicy::Skip),
            &HashSet::new(),
            &cancel,
            |progress, _| {
                if progress.finished == 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(matches!(outcomes[0], Outcome::Converted { .. }));
        assert_eq!(outcomes[1..], [Outcome::NotReached, Outcome::NotReached]);
        assert_eq!(output_names(output.path()), ["a.srt"]);
    }

    /// NAME-02, NAME-03 and NAME-08.
    #[test]
    fn destinations_follow_the_naming_rules() {
        let folder = tempfile::tempdir().unwrap();
        let original = folder.path().join("Film.srt");
        fs::write(&original, source("windows-1250-romanian")).unwrap();
        let mut beside = settings(folder.path(), CollisionPolicy::Skip);
        beside.destination = Destination::BesideOriginals;
        run(&[disk_job(&original, WINDOWS_1250)], &beside);
        beside.language = Some(SubtitleLanguage::parse("ro").unwrap());
        run(&[disk_job(&original, WINDOWS_1250)], &beside);
        assert_eq!(
            output_names(folder.path()),
            ["Film.ro.srt", "Film.srt", "Film1.srt"]
        );
        let output = tempfile::tempdir().unwrap();
        let nested = ConversionJob {
            origin: Origin::Disk {
                path: original,
                relative_folder: PathBuf::from("Show/S01"),
            },
            encoding: WINDOWS_1250,
        };
        run(&[nested], &settings(output.path(), CollisionPolicy::Skip));
        assert!(output.path().join("Show/S01/Film.srt").is_file());
    }

    /// NAME-10: the day's folder holds everything written to the output folder, and nothing
    /// written beside the originals.
    #[test]
    fn outputs_go_into_the_day_folder() {
        let folder = tempfile::tempdir().unwrap();
        let original = folder.path().join("Film.srt");
        fs::write(&original, source("windows-1250-romanian")).unwrap();
        let output = tempfile::tempdir().unwrap();
        let mut by_day = settings(output.path(), CollisionPolicy::Skip);
        by_day.day_folder = Some(DayFolder::new(2026, 9, 7));
        let nested = ConversionJob {
            origin: Origin::Disk {
                path: original.clone(),
                relative_folder: PathBuf::from("Show"),
            },
            encoding: WINDOWS_1250,
        };
        let jobs = [
            nested,
            dropped_job("Dropped.srt", "windows-1250-romanian", WINDOWS_1250),
        ];
        run(&jobs, &by_day);
        assert!(output.path().join("2026-09-07/Show/Film.srt").is_file());
        assert!(output.path().join("2026-09-07/Dropped.srt").is_file());
        by_day.destination = Destination::BesideOriginals;
        run(&[disk_job(&original, WINDOWS_1250)], &by_day);
        assert_eq!(output_names(folder.path()), ["Film.srt", "Film1.srt"]);
    }
}
