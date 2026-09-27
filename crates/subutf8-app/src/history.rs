use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use subutf8_core::batch::{ConversionJob, Origin};
use subutf8_core::language::SubtitleLanguage;
use subutf8_core::report::Outcome;

use crate::clock;
use crate::constants::{
    HISTORY_COMPACT_SLACK, HISTORY_FILE_NAME, MAXIMUM_HISTORY_RECORDS, MAXIMUM_SEARCH_RESULTS,
    PRIVATE_FILE_PERMISSIONS,
};
use crate::defaults::{create_private_folder, write_private_file};

/// WATCH-03: a file's size and modification time, which change whenever its content does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileStamp {
    pub size: u64,
    /// Nanoseconds since 1970.
    pub modified: u128,
}

impl FileStamp {
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = fs::symlink_metadata(path).ok()?;
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            size: metadata.len(),
            modified: modified.as_nanos(),
        })
    }
}

/// HIST-01: one converted file. Paths are personal data, so the history stays in SubUTF8's
/// private folder and is only ever shown through the app (SAFE-19). It never holds
/// subtitle text (SAFE-17).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub time: String,
    /// The original's path, or a dropped file's name.
    pub source: PathBuf,
    pub output: PathBuf,
    pub encoding: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// WATCH-01: converted by a watched folder.
    #[serde(default)]
    pub watched: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<FileStamp>,
}

impl HistoryRecord {
    /// HIST-01: only converted files are recorded.
    pub fn of(
        job: &ConversionJob,
        outcome: &Outcome,
        language: Option<&SubtitleLanguage>,
        watched: bool,
    ) -> Option<Self> {
        let Outcome::Converted { output, .. } = outcome else {
            return None;
        };
        let (source, stamp) = match &job.origin {
            Origin::Disk { path, .. } => (path.clone(), FileStamp::of(path)),
            Origin::Dropped { name, .. } => (PathBuf::from(name), None),
        };
        Some(Self {
            time: clock::timestamp(),
            source,
            output: output.clone(),
            encoding: job.encoding.name().to_owned(),
            language: language.map(|language| language.tag().to_owned()),
            watched,
            stamp,
        })
    }

    /// HIST-02: every word must appear, in any letter case, in the time, the paths, the
    /// encoding or the language.
    fn matches(&self, words: &[String]) -> bool {
        let text = format!(
            "{} {} {} {} {}",
            self.time,
            self.source.display(),
            self.output.display(),
            self.encoding,
            self.language.as_deref().unwrap_or_default()
        )
        .to_lowercase();
        words.iter().all(|word| text.contains(word.as_str()))
    }
}

#[derive(Debug)]
pub struct History {
    path: PathBuf,
    records: Vec<HistoryRecord>,
    pub problem: Option<io::ErrorKind>,
}

impl History {
    /// Lines that cannot be read, such as one cut short by a power cut, are left out.
    pub fn load(data_folder: &Path) -> Self {
        let path = data_folder.join(HISTORY_FILE_NAME);
        let records = fs::read_to_string(&path)
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect()
            })
            .unwrap_or_default();
        let mut history = Self {
            path,
            records,
            problem: None,
        };
        history.drop_oldest();
        history
    }

    fn drop_oldest(&mut self) {
        let excess = self.records.len().saturating_sub(MAXIMUM_HISTORY_RECORDS);
        self.records.drain(..excess);
    }

    /// HIST-01: added at the end of the file; once it holds too many, it is rewritten with
    /// the newest records only.
    pub fn append(&mut self, records: Vec<HistoryRecord>) {
        if records.is_empty() {
            return;
        }
        let new_lines = lines(&records);
        self.records.extend(records);
        let written = if self.records.len() > MAXIMUM_HISTORY_RECORDS + HISTORY_COMPACT_SLACK {
            self.drop_oldest();
            write_private_file(&self.path, &lines(&self.records))
        } else {
            append_private(&self.path, &new_lines)
        };
        self.problem = written.err().map(|error| error.kind());
    }

    /// HIST-02: the newest matches first.
    pub fn search(&self, query: &str) -> Vec<HistoryRecord> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.records
            .iter()
            .rev()
            .filter(|record| record.matches(&words))
            .take(MAXIMUM_SEARCH_RESULTS)
            .cloned()
            .collect()
    }

    /// WATCH-03: the originals already converted, as they were then.
    pub fn converted_stamps(&self) -> impl Iterator<Item = (PathBuf, FileStamp)> + '_ {
        self.records
            .iter()
            .filter_map(|record| Some((record.source.clone(), record.stamp?)))
    }

    /// WATCH-03: every file SubUTF8 wrote.
    pub fn outputs(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.records.iter().map(|record| record.output.clone())
    }
}

fn lines(records: &[HistoryRecord]) -> String {
    records
        .iter()
        .filter_map(|record| serde_json::to_string(record).ok())
        .map(|line| line + "\n")
        .collect()
}

fn append_private(path: &Path, text: &str) -> io::Result<()> {
    create_private_folder(path)?;
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(PRIVATE_FILE_PERMISSIONS)
        .open(path)?
        .write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const PERMISSION_BITS: u32 = 0o777;
    const DATE_LENGTH: usize = "2026-09-27".len();

    fn record(time: &str, name: &str) -> HistoryRecord {
        HistoryRecord {
            time: String::from(time),
            source: PathBuf::from(format!("/media/Show/{name}")),
            output: PathBuf::from(format!("/output/{}/{name}", &time[..DATE_LENGTH])),
            encoding: String::from("windows-1250"),
            language: Some(String::from("ro")),
            watched: false,
            stamp: Some(FileStamp {
                size: 10,
                modified: 20,
            }),
        }
    }

    /// HIST-01 and HIST-02.
    #[test]
    fn records_are_kept_and_found_newest_first() {
        let data = tempfile::tempdir().unwrap();
        let mut history = History::load(data.path());
        history.append(vec![record("2026-09-26T10:00:00+03:00", "Film.srt")]);
        history.append(vec![
            record("2026-09-27T09:00:00+03:00", "Other.srt"),
            record("2026-09-27T11:00:00+03:00", "Film.srt"),
        ]);
        assert_eq!(history.problem, None);
        let file = data.path().join(HISTORY_FILE_NAME);
        let mode = fs::metadata(&file).unwrap().permissions().mode() & PERMISSION_BITS;
        assert_eq!(mode, PRIVATE_FILE_PERMISSIONS);
        fs::write(
            &file,
            fs::read_to_string(&file).unwrap() + "{\"time\":\"cut sho",
        )
        .unwrap();
        let history = History::load(data.path());
        let times = |query: &str| -> Vec<String> {
            history
                .search(query)
                .into_iter()
                .map(|record| record.time)
                .collect()
        };
        assert_eq!(
            times("film.SRT"),
            ["2026-09-27T11:00:00+03:00", "2026-09-26T10:00:00+03:00"]
        );
        assert_eq!(times("2026-09-27 film"), ["2026-09-27T11:00:00+03:00"]);
        assert_eq!(times("").len(), 3);
        assert!(times("windows-1251").is_empty());
        assert_eq!(history.converted_stamps().count(), 3);
    }

    /// HIST-01.
    #[test]
    fn only_the_newest_records_are_kept() {
        let data = tempfile::tempdir().unwrap();
        let mut history = History::load(data.path());
        let many = MAXIMUM_HISTORY_RECORDS + HISTORY_COMPACT_SLACK + 1;
        history.append(
            (0..many)
                .map(|number| record("2026-09-27T11:00:00+03:00", &format!("{number}.srt")))
                .collect(),
        );
        assert_eq!(history.records.len(), MAXIMUM_HISTORY_RECORDS);
        let reloaded = History::load(data.path());
        assert_eq!(reloaded.records, history.records);
        assert_eq!(
            reloaded.records.last().unwrap().source,
            Path::new(&format!("/media/Show/{}.srt", many - 1))
        );
    }
}
