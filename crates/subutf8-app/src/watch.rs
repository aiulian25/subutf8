use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use subutf8_core::batch::{CollisionPolicy, ConversionJob, Origin, run_batch};
use subutf8_core::constants::MAXIMUM_LISTED_FILES;
use subutf8_core::input_scan::{ListedFile, scan_folder};
use subutf8_core::report::Outcome;
use tokio::time::{MissedTickBehavior, interval};

use crate::clock;
use crate::constants::WATCH_LOG_ENTRIES;
use crate::defaults::Defaults;
use crate::history::{FileStamp, HistoryRecord};
use crate::server::{AppState, lock};
use crate::session::{FileStatus, prepare_listed};
use crate::views::DestinationName;

/// WATCH-04: what happened to one watched file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchResult {
    Finished(Outcome),
    /// It needs a person, so it waits in the page's list.
    Listed,
    /// SAFE-18: SubUTF8's own output of the named file.
    ConvertedCopy(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEntry {
    pub time: String,
    pub path: PathBuf,
    pub result: WatchResult,
}

/// WATCH-04: the latest entries, oldest first, and what holds watching up.
#[derive(Debug, Default)]
pub struct WatchLog {
    pub entries: VecDeque<WatchEntry>,
    pub unavailable_folders: Vec<PathBuf>,
    pub output_folder_unavailable: bool,
}

impl WatchLog {
    fn push(&mut self, path: PathBuf, result: WatchResult) {
        if self.entries.len() == WATCH_LOG_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(WatchEntry {
            time: clock::timestamp(),
            path,
            result,
        });
    }
}

/// WATCH-02 and WATCH-03: files seen once and waiting to settle, and files already handled
/// as they are now.
#[derive(Debug, Default)]
pub struct Progress {
    first_seen: HashMap<PathBuf, FileStamp>,
    handled: HashSet<(PathBuf, FileStamp)>,
}

impl Progress {
    /// WATCH-02: a file is taken once its size and time are the same as at the last look,
    /// so a file still being copied is never read.
    fn is_settled(&mut self, path: &Path, stamp: FileStamp) -> bool {
        let is_settled = self.first_seen.get(path) == Some(&stamp);
        if !is_settled {
            self.first_seen.insert(path.to_path_buf(), stamp);
        }
        is_settled
    }

    /// Files that are gone are forgotten, so memory stays small; the history keeps what was
    /// converted (WATCH-03).
    fn keep_only(&mut self, present: &HashSet<PathBuf>) {
        self.first_seen.retain(|path, _| present.contains(path));
        self.handled.retain(|(path, _)| present.contains(path));
    }
}

/// WATCH-01: Docker looks at the watched folders at every interval.
pub async fn watch_loop(app: AppState) {
    let mut ticks = interval(app.settings.watch_interval);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut progress = Progress::default();
    loop {
        ticks.tick().await;
        let worker = app.clone();
        progress = tokio::task::spawn_blocking(move || {
            look_once(&worker, &mut progress);
            progress
        })
        .await
        .unwrap_or_default();
    }
}

/// WATCH-01 to WATCH-03: finds settled new files and converts them with the saved defaults.
pub fn look_once(app: &AppState, progress: &mut Progress) {
    let defaults = lock(&app.defaults).current.clone();
    let area = &app.settings.allowed_area;
    let needs_output_folder = defaults.destination == DestinationName::OutputFolder;
    let output_folder = area.real_location(&defaults.output_folder).ok();
    let mut present = HashSet::new();
    let mut settled = Vec::new();
    let mut unavailable_folders = Vec::new();
    for folder in &defaults.watch_folders {
        let scan = area
            .real_location(folder)
            .and_then(|real| Ok((scan_folder(&real, true, area, MAXIMUM_LISTED_FILES)?, real)));
        let Ok((scan, real)) = scan else {
            unavailable_folders.push(folder.clone());
            continue;
        };
        // WATCH-05: when the output folder lies inside this watched folder, the outputs written
        // there are never taken as new files. A watched folder inside the output folder is
        // watched as usual, since outputs never land in it.
        let outputs_inside = output_folder
            .as_ref()
            .filter(|output| needs_output_folder && output.starts_with(&real));
        for file in scan.files {
            let is_output = outputs_inside.is_some_and(|output| file.path.starts_with(output));
            let Some(stamp) = FileStamp::of(&file.path).filter(|_| !is_output) else {
                continue;
            };
            present.insert(file.path.clone());
            let is_handled = progress.handled.contains(&(file.path.clone(), stamp));
            if is_handled || !progress.is_settled(&file.path, stamp) {
                continue;
            }
            let relative_folder = file
                .path
                .parent()
                .and_then(|parent| parent.strip_prefix(&real).ok())
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let listed = ListedFile {
                path: file.path,
                relative_folder,
            };
            settled.push((listed, stamp));
        }
    }
    progress.keep_only(&present);
    let output_folder_unavailable = needs_output_folder && output_folder.is_none();
    {
        let mut log = lock(&app.watch_log);
        log.unavailable_folders = unavailable_folders;
        log.output_folder_unavailable = output_folder_unavailable;
    }
    if settled.is_empty() || output_folder_unavailable {
        return;
    }
    convert_settled(app, &defaults, settled, progress);
}

/// WATCH-03: never converts what SubUTF8 converted or wrote before, and never replaces an
/// existing file, so a restart rewrites nothing.
fn convert_settled(
    app: &AppState,
    defaults: &Defaults,
    settled: Vec<(ListedFile, FileStamp)>,
    progress: &mut Progress,
) {
    let (converted, outputs): (HashSet<(PathBuf, FileStamp)>, HashSet<PathBuf>) = {
        let history = lock(&app.history);
        (
            history.converted_stamps().collect(),
            history.outputs().collect(),
        )
    };
    let session_settings = defaults.session_settings();
    let language = session_settings.language.as_ref();
    let settings = session_settings.batch_settings(CollisionPolicy::Skip);
    let mut jobs = Vec::new();
    let mut for_review = Vec::new();
    let mut entries = Vec::new();
    for (file, stamp) in settled {
        let key = (file.path.clone(), stamp);
        progress.handled.insert(key.clone());
        if outputs.contains(&file.path) || converted.contains(&key) {
            continue;
        }
        let path = file.path.clone();
        let prepared = prepare_listed(file, language);
        let reading = match prepared.status() {
            FileStatus::Ready(reading) => Some(reading),
            FileStatus::ConvertedCopy(original) => {
                entries.push((path, WatchResult::ConvertedCopy(original.to_owned())));
                continue;
            }
            _ => None,
        };
        let Some(reading) = reading else {
            entries.push((path, WatchResult::Listed));
            for_review.push(prepared);
            continue;
        };
        jobs.push(ConversionJob {
            origin: prepared.origin,
            reading,
            language: language.cloned(),
        });
    }
    if !for_review.is_empty() {
        app.session().add(for_review);
    }
    let outcomes = run_batch(
        &jobs,
        &settings,
        &HashSet::new(),
        &AtomicBool::new(false),
        |_, _| {},
    );
    let records = jobs
        .iter()
        .zip(&outcomes)
        .filter_map(|(job, outcome)| HistoryRecord::of(job, outcome, true))
        .collect();
    lock(&app.history).append(records);
    for (job, outcome) in jobs.iter().zip(outcomes) {
        if let Origin::Disk { path, .. } = &job.origin {
            entries.push((path.clone(), WatchResult::Finished(outcome)));
        }
    }
    let mut log = lock(&app.watch_log);
    for (path, result) in entries {
        log.push(path, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{fixture_output, fixture_source, test_state};
    use std::fs;

    fn watching(app: &AppState, folder: &Path) {
        let mut store = lock(&app.defaults);
        let mut defaults = store.current.clone();
        defaults.watch_folders = vec![fs::canonicalize(folder).unwrap()];
        store.save(defaults).unwrap();
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// WATCH-01 to WATCH-04.
    #[test]
    fn settled_files_convert_once_and_outputs_are_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let watched = root.path().join("incoming");
        fs::create_dir_all(watched.join("Show")).unwrap();
        let app = test_state(root.path(), root.path());
        watching(&app, &watched);
        let mut progress = Progress::default();
        fs::copy(
            fixture_source("windows-1250-romanian"),
            watched.join("Show/Film.srt"),
        )
        .unwrap();
        fs::copy(fixture_source("short-romanian"), watched.join("Short.srt")).unwrap();
        look_once(&app, &mut progress);
        assert!(
            lock(&app.watch_log).entries.is_empty(),
            "nothing settled yet"
        );
        look_once(&app, &mut progress);
        assert_eq!(
            fs::read_to_string(watched.join("Show/Film1.srt")).unwrap(),
            fixture_output("windows-1250-romanian")
        );
        assert_eq!(
            app.session().files.len(),
            1,
            "the short file waits in the list"
        );
        for _ in 0..3 {
            look_once(&app, &mut progress);
        }
        assert_eq!(names(&watched.join("Show")), ["Film.srt", "Film1.srt"]);
        let log = lock(&app.watch_log);
        assert_eq!(log.entries.len(), 2);
        assert!(
            log.entries
                .iter()
                .any(|entry| entry.result == WatchResult::Listed)
        );
        drop(log);

        // A restart forgets what it saw; the history still says what was converted.
        let mut restarted = Progress::default();
        for _ in 0..3 {
            look_once(&app, &mut restarted);
        }
        assert_eq!(names(&watched.join("Show")), ["Film.srt", "Film1.srt"]);
        assert_eq!(lock(&app.history).search("Film.srt").len(), 1);
    }

    /// WATCH-05: a watched folder inside the output folder is watched as usual, while an output
    /// folder inside a watched folder is left out of it.
    #[test]
    fn output_folders_and_watched_folders_can_nest() {
        let root = tempfile::tempdir().unwrap();
        let watched = root.path().join("incoming");
        fs::create_dir_all(&watched).unwrap();
        let app = test_state(root.path(), root.path());
        {
            let mut store = lock(&app.defaults);
            let mut defaults = store.current.clone();
            defaults.destination = DestinationName::OutputFolder;
            defaults.watch_folders = vec![fs::canonicalize(&watched).unwrap()];
            store.save(defaults).unwrap();
        }
        fs::copy(
            fixture_source("windows-1250-romanian"),
            watched.join("Film.srt"),
        )
        .unwrap();
        let mut progress = Progress::default();
        for _ in 0..4 {
            look_once(&app, &mut progress);
        }
        assert_eq!(
            fs::read_to_string(root.path().join("Film.srt")).unwrap(),
            fixture_output("windows-1250-romanian")
        );
        assert_eq!(names(&watched), ["Film.srt"]);

        // The output folder inside the watched one: what lands there is never taken.
        let output = watched.join("converted");
        fs::create_dir_all(&output).unwrap();
        fs::copy(fixture_source("markup"), output.join("Other.srt")).unwrap();
        {
            let mut store = lock(&app.defaults);
            let mut defaults = store.current.clone();
            defaults.output_folder = fs::canonicalize(&output).unwrap();
            store.save(defaults).unwrap();
        }
        for _ in 0..4 {
            look_once(&app, &mut progress);
        }
        assert_eq!(names(&output), ["Other.srt"]);
    }
}
