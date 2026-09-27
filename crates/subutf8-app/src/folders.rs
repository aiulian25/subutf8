use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use subutf8_core::constants::HIDDEN_NAME_PREFIX;
use subutf8_core::input_scan::{AllowedArea, SkipReason};
use subutf8_core::output_naming::split_srt_name_bytes;

use crate::constants::BROWSE_MAXIMUM_ENTRIES;

/// One folder as the Browse view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub path: PathBuf,
    pub parent: Option<PathBuf>,
    pub folders: Vec<String>,
    pub files: Vec<String>,
    /// NAME-11: `.srt` files whose names are not UTF-8, which are read with the file.
    pub raw_files: Vec<OsString>,
    pub is_truncated: bool,
}

/// SAFE-12 to SAFE-14 for the Browse view: the folder's real location must be inside the
/// allowed area. Linked folders are listed, and their real location is checked when opened;
/// linked files are not, because they are never read. Folders whose names are not UTF-8 are
/// left out (NAME-11).
pub fn list_folder(path: &Path, area: &AllowedArea) -> Result<Listing, SkipReason> {
    let real = area.real_location(path)?;
    if !real.is_dir() {
        return Err(SkipReason::NotAFolder);
    }
    let entries = fs::read_dir(&real).map_err(|error| SkipReason::Unreadable(error.kind()))?;
    let mut folders = Vec::new();
    let mut files = Vec::new();
    let mut raw_files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(HIDDEN_NAME_PREFIX) {
            continue;
        }
        let is_folder = entry.path().is_dir();
        let is_srt_file = entry.file_type().is_ok_and(|file_type| file_type.is_file())
            && split_srt_name_bytes(name.as_bytes()).is_some();
        match name.into_string() {
            Ok(name) if is_folder => folders.push(name),
            Ok(name) if is_srt_file => files.push(name),
            Err(name) if is_srt_file => raw_files.push(name),
            _ => {}
        }
    }
    folders.sort_by_key(|name| name.to_lowercase());
    files.sort_by_key(|name| name.to_lowercase());
    raw_files.sort();
    let is_truncated = folders.len() + files.len() + raw_files.len() > BROWSE_MAXIMUM_ENTRIES;
    folders.truncate(BROWSE_MAXIMUM_ENTRIES);
    files.truncate(BROWSE_MAXIMUM_ENTRIES.saturating_sub(folders.len()));
    raw_files.truncate(BROWSE_MAXIMUM_ENTRIES.saturating_sub(folders.len() + files.len()));
    let parent = real
        .parent()
        .filter(|parent| area.real_location(parent).is_ok())
        .map(Path::to_path_buf);
    Ok(Listing {
        path: real,
        parent,
        folders,
        files,
        raw_files,
        is_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::fs::symlink;

    #[test]
    fn listing_shows_folders_and_srt_files_only() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Show");
        fs::create_dir_all(folder.join("season 2")).unwrap();
        fs::create_dir_all(folder.join(".hidden")).unwrap();
        for name in ["b.srt", "A.SRT", "notes.txt", ".c.srt"] {
            fs::write(folder.join(name), "1\n").unwrap();
        }
        symlink(folder.join("b.srt"), folder.join("link.srt")).unwrap();
        let legacy_file = OsStr::from_bytes(b"Fat\xe3.srt");
        fs::write(folder.join(legacy_file), "1\n").unwrap();
        fs::create_dir_all(folder.join(OsStr::from_bytes(b"Sezon \xba"))).unwrap();
        let area = AllowedArea::new([root.path().to_path_buf()]);
        let listing = list_folder(&folder, &area).unwrap();
        assert_eq!(listing.folders, ["season 2"]);
        assert_eq!(listing.files, ["A.SRT", "b.srt"]);
        assert_eq!(listing.raw_files, [legacy_file]);
        assert_eq!(listing.parent, Some(fs::canonicalize(root.path()).unwrap()));
        let top = list_folder(root.path(), &area).unwrap();
        assert_eq!(top.parent, None);
    }

    #[test]
    fn browse_cannot_leave_allowed_folder() {
        let root = tempfile::tempdir().unwrap();
        let allowed = root.path().join("data");
        fs::create_dir_all(&allowed).unwrap();
        symlink(root.path(), allowed.join("escape")).unwrap();
        let area = AllowedArea::new([allowed.clone()]);
        assert_eq!(
            list_folder(&allowed.join("escape"), &area),
            Err(SkipReason::OutsideAllowedArea)
        );
        assert_eq!(
            list_folder(Path::new("/etc"), &area),
            Err(SkipReason::OutsideAllowedArea)
        );
    }
}
