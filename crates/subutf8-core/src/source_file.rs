use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::constants::MAXIMUM_FILE_BYTES;

/// Why a file was not read (ENC-01, ENC-02, SAFE-11 and SAFE-12).
#[derive(Debug)]
pub enum ReadError {
    /// ENC-01, with the system's reason.
    Unreadable(io::Error),
    /// ENC-02.
    TooLarge { size: u64 },
    /// SAFE-11.
    NotRegularFile,
    /// SAFE-12.
    SymbolicLink,
}

/// Reads a subtitle file whole, refusing symbolic links, special files and files over LIMIT-01.
pub fn read_source(path: &Path) -> Result<Vec<u8>, ReadError> {
    let link_metadata = fs::symlink_metadata(path).map_err(ReadError::Unreadable)?;
    if link_metadata.file_type().is_symlink() {
        return Err(ReadError::SymbolicLink);
    }
    if !link_metadata.is_file() {
        return Err(ReadError::NotRegularFile);
    }
    let file = open_without_following_links(path)?;
    let metadata = file.metadata().map_err(ReadError::Unreadable)?;
    if !metadata.is_file() {
        return Err(ReadError::NotRegularFile);
    }
    if metadata.len() > MAXIMUM_FILE_BYTES {
        return Err(ReadError::TooLarge {
            size: metadata.len(),
        });
    }
    read_up_to_limit(file)
}

// Another program could swap the file between the checks above and the open, so the
// open itself refuses links, never blocks on a pipe, and its result is checked again.
fn open_without_following_links(path: &Path) -> Result<File, ReadError> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                return ReadError::SymbolicLink;
            }
            ReadError::Unreadable(error)
        })
}

fn read_up_to_limit(file: File) -> Result<Vec<u8>, ReadError> {
    let mut bytes = Vec::new();
    file.take(MAXIMUM_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(ReadError::Unreadable)?;
    let size = bytes.len() as u64;
    if size > MAXIMUM_FILE_BYTES {
        return Err(ReadError::TooLarge { size });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::source_path;
    use std::os::unix::fs::symlink;
    use std::process::Command;

    #[test]
    fn regular_file_is_read_whole() {
        let path = source_path("windows-1250-romanian");
        assert_eq!(read_source(&path).unwrap(), fs::read(&path).unwrap());
    }

    /// ENC-01.
    #[test]
    fn missing_file_is_unreadable() {
        let folder = tempfile::tempdir().unwrap();
        let result = read_source(&folder.path().join("missing.srt"));
        assert!(matches!(
            result,
            Err(ReadError::Unreadable(error)) if error.kind() == io::ErrorKind::NotFound
        ));
    }

    /// ENC-02 and LIMIT-01.
    #[test]
    fn oversized_file_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let at_limit = folder.path().join("at-limit.srt");
        let over_limit = folder.path().join("over-limit.srt");
        File::create(&at_limit)
            .unwrap()
            .set_len(MAXIMUM_FILE_BYTES)
            .unwrap();
        File::create(&over_limit)
            .unwrap()
            .set_len(MAXIMUM_FILE_BYTES + 1)
            .unwrap();
        assert_eq!(
            read_source(&at_limit).unwrap().len() as u64,
            MAXIMUM_FILE_BYTES
        );
        assert!(matches!(
            read_source(&over_limit),
            Err(ReadError::TooLarge { size }) if size == MAXIMUM_FILE_BYTES + 1
        ));
    }

    /// SAFE-11. A pipe must be refused without blocking.
    #[test]
    fn special_files_are_not_read() {
        let folder = tempfile::tempdir().unwrap();
        let pipe = folder.path().join("pipe.srt");
        let created = Command::new("mkfifo").arg(&pipe).status().unwrap();
        assert!(created.success());
        for path in [pipe.as_path(), folder.path(), Path::new("/dev/null")] {
            assert!(
                matches!(read_source(path), Err(ReadError::NotRegularFile)),
                "{}",
                path.display()
            );
        }
    }

    /// SAFE-12.
    #[test]
    fn symbolic_link_is_not_read() {
        let folder = tempfile::tempdir().unwrap();
        let link = folder.path().join("link.srt");
        symlink(source_path("utf8-romanian"), &link).unwrap();
        assert!(matches!(read_source(&link), Err(ReadError::SymbolicLink)));
    }
}
