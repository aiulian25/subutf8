use std::ffi::OsStr;
use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};

use crate::constants::{HIDDEN_NAME_PREFIX, SRT_EXTENSION};
use crate::output_naming::split_srt_name;

/// SAFE-13: the folders the app may read and write, by real location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedArea {
    roots: Vec<PathBuf>,
}

impl AllowedArea {
    /// Roots that do not exist are left out, so nothing is allowed there.
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let roots = roots
            .into_iter()
            .filter_map(|root| fs::canonicalize(root).ok())
            .collect();
        Self { roots }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// The real location of `path`, when it lies inside the area.
    pub fn real_location(&self, path: &Path) -> Result<PathBuf, SkipReason> {
        let real = fs::canonicalize(path).map_err(|error| SkipReason::Unreadable(error.kind()))?;
        if !self.roots.iter().any(|root| real.starts_with(root)) {
            return Err(SkipReason::OutsideAllowedArea);
        }
        Ok(real)
    }
}

/// A file ready to be added to the file list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    pub path: PathBuf,
    /// NAME-08: the added folder's name and any sub-folders, used in an output folder.
    pub relative_folder: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// SAFE-12.
    SymbolicLink,
    /// SAFE-11.
    NotRegularFile,
    /// NAME-01.
    NotSrt,
    /// SAFE-13.
    OutsideAllowedArea,
    NotAFolder,
    /// Linux allows any bytes in names; the app handles only UTF-8 ones.
    NameNotUtf8,
    Unreadable(io::ErrorKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedPath {
    pub path: PathBuf,
    pub reason: SkipReason,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Scan {
    pub files: Vec<ListedFile>,
    pub skipped: Vec<SkippedPath>,
    /// LIMIT-03: the scan stopped because the file list is full.
    pub limit_reached: bool,
}

/// SAFE-11 to SAFE-13 and NAME-01, for a file chosen directly or opened from the file manager.
pub fn check_chosen_file(path: &Path, area: &AllowedArea) -> Result<ListedFile, SkipReason> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| SkipReason::Unreadable(error.kind()))?;
    if metadata.file_type().is_symlink() {
        return Err(SkipReason::SymbolicLink);
    }
    if !metadata.is_file() {
        return Err(SkipReason::NotRegularFile);
    }
    let name = path.file_name().ok_or(SkipReason::NotRegularFile)?;
    check_srt_name(name)?;
    let folder = path.parent().ok_or(SkipReason::NotRegularFile)?;
    let real_folder = area.real_location(folder)?;
    Ok(ListedFile {
        path: real_folder.join(name),
        relative_folder: PathBuf::new(),
    })
}

/// SAFE-12 to SAFE-14 and LIMIT-03: lists a folder's `.srt` files in name order, skipping
/// hidden entries and never entering linked folders. `limit` is the room left in the list.
pub fn scan_folder(
    folder: &Path,
    include_subfolders: bool,
    area: &AllowedArea,
    limit: usize,
) -> Result<Scan, SkipReason> {
    let real_folder = area.real_location(folder)?;
    if !real_folder.is_dir() {
        return Err(SkipReason::NotAFolder);
    }
    let top_name = real_folder
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut scan = Scan::default();
    visit(
        &real_folder,
        &top_name,
        include_subfolders,
        limit,
        &mut scan,
    )
    .map_err(|error| SkipReason::Unreadable(error.kind()))?;
    Ok(scan)
}

fn visit(
    folder: &Path,
    relative_folder: &Path,
    include_subfolders: bool,
    limit: usize,
    scan: &mut Scan,
) -> io::Result<()> {
    let mut entries: Vec<DirEntry> = fs::read_dir(folder)?.collect::<io::Result<_>>()?;
    entries.sort_by_key(DirEntry::file_name);
    for entry in entries {
        if scan.files.len() >= limit {
            scan.limit_reached = true;
            return Ok(());
        }
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(HIDDEN_NAME_PREFIX) {
            continue;
        }
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() && include_subfolders {
            visit(
                &path,
                &relative_folder.join(&name),
                include_subfolders,
                limit,
                scan,
            )?;
            continue;
        }
        let Err(reason) = check_scanned_file(&name, file_type) else {
            scan.files.push(ListedFile {
                path,
                relative_folder: relative_folder.to_path_buf(),
            });
            continue;
        };
        if reason != SkipReason::NotSrt && reason != SkipReason::NotAFolder {
            scan.skipped.push(SkippedPath { path, reason });
        }
    }
    Ok(())
}

/// Folders, and files that are not `.srt`, are left out of a scan without a report.
fn check_scanned_file(name: &OsStr, file_type: fs::FileType) -> Result<(), SkipReason> {
    if file_type.is_dir() {
        return Err(SkipReason::NotAFolder);
    }
    check_srt_name(name)?;
    if file_type.is_symlink() {
        return Err(SkipReason::SymbolicLink);
    }
    if !file_type.is_file() {
        return Err(SkipReason::NotRegularFile);
    }
    Ok(())
}

fn check_srt_name(name: &OsStr) -> Result<(), SkipReason> {
    let is_srt = name
        .to_string_lossy()
        .to_lowercase()
        .ends_with(SRT_EXTENSION);
    if !is_srt {
        return Err(SkipReason::NotSrt);
    }
    let name = name.to_str().ok_or(SkipReason::NameNotUtf8)?;
    split_srt_name(name).map(drop).ok_or(SkipReason::NotSrt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    use std::process::Command;

    const NO_LIMIT: usize = usize::MAX;

    fn create(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "1\n").unwrap();
    }

    fn whole_system() -> AllowedArea {
        AllowedArea::new([PathBuf::from("/")])
    }

    fn listed(scan: &Scan) -> Vec<(String, String)> {
        scan.files
            .iter()
            .map(|file| {
                (
                    file.path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    file.relative_folder.to_string_lossy().into_owned(),
                )
            })
            .collect()
    }

    fn pair(name: &str, folder: &str) -> (String, String) {
        (String::from(name), String::from(folder))
    }

    /// SAFE-14 and NAME-08.
    #[test]
    fn folders_list_srt_files_in_name_order() {
        let root = tempfile::tempdir().unwrap();
        let show = root.path().join("Show");
        for name in [
            "b.srt",
            "a.SRT",
            "c.txt",
            ".hidden.srt",
            "._a.srt",
            "S01/e1.srt",
        ] {
            create(&show.join(name));
        }
        create(&show.join(".hidden/e2.srt"));
        let flat = scan_folder(&show, false, &whole_system(), NO_LIMIT).unwrap();
        assert_eq!(
            listed(&flat),
            [pair("a.SRT", "Show"), pair("b.srt", "Show")]
        );
        let deep = scan_folder(&show, true, &whole_system(), NO_LIMIT).unwrap();
        assert_eq!(
            listed(&deep),
            [
                pair("e1.srt", "Show/S01"),
                pair("a.SRT", "Show"),
                pair("b.srt", "Show")
            ]
        );
        assert!(deep.skipped.is_empty() && !deep.limit_reached);
    }

    /// SAFE-12.
    #[test]
    fn symlinks_are_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        create(&outside.join("secret.srt"));
        let inside = root.path().join("inside");
        create(&inside.join("real.srt"));
        symlink(outside.join("secret.srt"), inside.join("link.srt")).unwrap();
        symlink(&outside, inside.join("linked-folder")).unwrap();
        let scan = scan_folder(&inside, true, &whole_system(), NO_LIMIT).unwrap();
        assert_eq!(listed(&scan), [pair("real.srt", "inside")]);
        assert_eq!(
            scan.skipped,
            [SkippedPath {
                path: fs::canonicalize(&inside).unwrap().join("link.srt"),
                reason: SkipReason::SymbolicLink
            }]
        );
        assert_eq!(
            check_chosen_file(&inside.join("link.srt"), &whole_system()),
            Err(SkipReason::SymbolicLink)
        );
    }

    /// SAFE-13.
    #[test]
    fn browse_cannot_leave_allowed_folder() {
        let root = tempfile::tempdir().unwrap();
        let allowed = root.path().join("data");
        let outside = root.path().join("other");
        create(&allowed.join("in.srt"));
        create(&outside.join("out.srt"));
        symlink(&outside, allowed.join("escape")).unwrap();
        let area = AllowedArea::new([allowed.clone()]);
        assert!(scan_folder(&allowed, true, &area, NO_LIMIT).is_ok());
        for folder in [outside.clone(), allowed.join("escape"), allowed.join("..")] {
            assert_eq!(
                scan_folder(&folder, false, &area, NO_LIMIT),
                Err(SkipReason::OutsideAllowedArea),
                "{}",
                folder.display()
            );
        }
        assert_eq!(
            check_chosen_file(&outside.join("out.srt"), &area),
            Err(SkipReason::OutsideAllowedArea)
        );
        assert_eq!(
            check_chosen_file(&allowed.join("escape/out.srt"), &area),
            Err(SkipReason::OutsideAllowedArea)
        );
        assert!(check_chosen_file(&allowed.join("in.srt"), &area).is_ok());
    }

    /// SAFE-11 and NAME-01.
    #[test]
    fn chosen_files_must_be_regular_srt_files() {
        let root = tempfile::tempdir().unwrap();
        let pipe = root.path().join("pipe.srt");
        assert!(
            Command::new("mkfifo")
                .arg(&pipe)
                .status()
                .unwrap()
                .success()
        );
        create(&root.path().join("notes.txt"));
        let area = whole_system();
        assert_eq!(
            check_chosen_file(&pipe, &area),
            Err(SkipReason::NotRegularFile)
        );
        assert_eq!(
            check_chosen_file(&root.path().join("notes.txt"), &area),
            Err(SkipReason::NotSrt)
        );
        assert_eq!(
            check_chosen_file(&root.path().join("missing.srt"), &area),
            Err(SkipReason::Unreadable(io::ErrorKind::NotFound))
        );
        let scan = scan_folder(root.path(), false, &area, NO_LIMIT).unwrap();
        assert_eq!(scan.skipped[0].reason, SkipReason::NotRegularFile);
    }

    /// Linux allows any bytes in names; such names are reported instead of mangled.
    #[test]
    fn names_that_are_not_utf8_are_reported() {
        let root = tempfile::tempdir().unwrap();
        let name = OsStr::from_bytes(b"Fat\xe3.srt");
        create(&root.path().join(name));
        let scan = scan_folder(root.path(), false, &whole_system(), NO_LIMIT).unwrap();
        assert!(scan.files.is_empty());
        assert_eq!(scan.skipped[0].reason, SkipReason::NameNotUtf8);
    }

    /// LIMIT-03.
    #[test]
    fn scan_stops_when_the_list_is_full() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a.srt", "b.srt", "c.srt"] {
            create(&root.path().join(name));
        }
        let scan = scan_folder(root.path(), false, &whole_system(), 2).unwrap();
        assert_eq!(scan.files.len(), 2);
        assert!(scan.limit_reached);
    }
}
