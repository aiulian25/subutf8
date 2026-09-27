use std::fs::{self, File, Metadata, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tempfile::{Builder, NamedTempFile};

use crate::constants::{
    NEW_FILE_PERMISSIONS, READ_WRITE_PERMISSION_BITS, TEMPORARY_FILE_PREFIX, TEMPORARY_FILE_SUFFIX,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Never replaces an existing file (SAFE-06).
    CreateNew,
    /// Replaces an existing file in one step, but never a link or a read-only file (SAFE-04).
    Replace,
}

#[derive(Debug)]
pub enum WriteError {
    /// The destination exists and was left untouched.
    AlreadyExists,
    DestinationIsSymbolicLink,
    DestinationIsReadOnly,
    /// ENC-16: the file read back differs from the text meant to be written.
    VerificationFailed,
    Io(io::Error),
}

/// SAFE-09: the mode for a new output. It copies the read and write bits of the original,
/// and never execute or special bits.
pub fn output_permissions(original: Option<&Metadata>) -> u32 {
    original.map_or(NEW_FILE_PERMISSIONS, |metadata| {
        metadata.permissions().mode() & READ_WRITE_PERMISSION_BITS
    })
}

/// SAFE-06, SAFE-07, SAFE-10 and ENC-16: writes `text` through a private temporary file in
/// the destination folder, reads it back, and only then publishes it. On any failure the
/// temporary file is removed and the destination is untouched.
pub fn write_verified(
    destination: &Path,
    text: &str,
    permissions: u32,
    write_mode: WriteMode,
) -> Result<(), WriteError> {
    let folder = destination
        .parent()
        .ok_or_else(|| WriteError::Io(io::ErrorKind::InvalidInput.into()))?;
    if write_mode == WriteMode::Replace {
        check_replaceable(destination)?;
    }
    let temporary = write_temporary(folder, text.as_bytes())?;
    verify(temporary.path(), text)?;
    // SAFE-09: file systems without permissions refuse this, which is harmless.
    fs::set_permissions(temporary.path(), Permissions::from_mode(permissions)).ok();
    publish(temporary, destination, write_mode)?;
    sync_folder(folder);
    Ok(())
}

fn check_replaceable(destination: &Path) -> Result<(), WriteError> {
    let Ok(metadata) = fs::symlink_metadata(destination) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        return Err(WriteError::DestinationIsSymbolicLink);
    }
    if metadata.permissions().readonly() {
        return Err(WriteError::DestinationIsReadOnly);
    }
    Ok(())
}

/// The temporary file is readable only by the user until it is published.
fn write_temporary(folder: &Path, bytes: &[u8]) -> Result<NamedTempFile, WriteError> {
    let mut temporary = Builder::new()
        .prefix(TEMPORARY_FILE_PREFIX)
        .suffix(TEMPORARY_FILE_SUFFIX)
        .tempfile_in(folder)
        .map_err(WriteError::Io)?;
    temporary.write_all(bytes).map_err(WriteError::Io)?;
    temporary.as_file().sync_all().map_err(WriteError::Io)?;
    Ok(temporary)
}

/// ENC-16. The text is a Rust string, so equal bytes are also valid UTF-8 of that text.
fn verify(path: &Path, text: &str) -> Result<(), WriteError> {
    let written = fs::read(path).map_err(WriteError::Io)?;
    if written != text.as_bytes() {
        return Err(WriteError::VerificationFailed);
    }
    Ok(())
}

fn publish(
    temporary: NamedTempFile,
    destination: &Path,
    write_mode: WriteMode,
) -> Result<(), WriteError> {
    if write_mode == WriteMode::Replace {
        return temporary
            .persist(destination)
            .map(drop)
            .map_err(|error| WriteError::Io(error.error));
    }
    match temporary.persist_noclobber(destination) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
            Err(WriteError::AlreadyExists)
        }
        Err(error) if cannot_publish_without_replacing(&error.error) => {
            publish_after_checking_name(error.file, destination)
        }
        Err(error) => Err(WriteError::Io(error.error)),
    }
}

/// SAFE-10: some network and FUSE drives support neither an exclusive rename nor hard links.
fn cannot_publish_without_replacing(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EINVAL | libc::ENOTSUP | libc::EPERM | libc::ENOSYS)
    )
}

/// SAFE-10: checks the name immediately before renaming, so only a file created by another
/// program in that instant could be replaced.
fn publish_after_checking_name(
    temporary: NamedTempFile,
    destination: &Path,
) -> Result<(), WriteError> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(WriteError::AlreadyExists);
    }
    temporary
        .persist(destination)
        .map(drop)
        .map_err(|error| WriteError::Io(error.error))
}

/// SAFE-06: makes the new name survive a power cut. Some file systems cannot flush a
/// folder, which is harmless once the file itself is flushed.
fn sync_folder(folder: &Path) {
    if let Ok(handle) = File::open(folder) {
        handle.sync_all().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const TEXT: &str = "1\r\n00:00:01,000 --> 00:00:02,000\r\nȘtii ce înseamnă?\r\n";
    const PERMISSION_BITS: u32 = 0o777;

    fn entries(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & PERMISSION_BITS
    }

    /// SAFE-06, SAFE-09 and ENC-16.
    #[test]
    fn writes_exact_text_with_the_originals_read_write_bits() {
        let folder = tempfile::tempdir().unwrap();
        let original = folder.path().join("Film.srt");
        fs::write(&original, "old").unwrap();
        fs::set_permissions(&original, Permissions::from_mode(0o755)).unwrap();
        let destination = folder.path().join("Film.ro.srt");
        let permissions = output_permissions(Some(&fs::metadata(&original).unwrap()));
        write_verified(&destination, TEXT, permissions, WriteMode::CreateNew).unwrap();
        assert_eq!(fs::read_to_string(&destination).unwrap(), TEXT);
        assert_eq!(mode(&destination), 0o644);
        assert_eq!(entries(folder.path()), ["Film.ro.srt", "Film.srt"]);
    }

    /// SAFE-06: the temporary file is private while it is being written.
    #[test]
    fn temporary_file_is_private() {
        let folder = tempfile::tempdir().unwrap();
        let temporary = write_temporary(folder.path(), TEXT.as_bytes()).unwrap();
        let name = temporary.path().file_name().unwrap().to_string_lossy();
        assert!(name.starts_with(TEMPORARY_FILE_PREFIX) && name.ends_with(TEMPORARY_FILE_SUFFIX));
        assert_eq!(mode(temporary.path()), 0o600);
    }

    /// SAFE-04: the default setting leaves an existing file untouched.
    #[test]
    fn collision_skip() {
        let folder = tempfile::tempdir().unwrap();
        let destination = folder.path().join("Film.ro.srt");
        fs::write(&destination, "existing").unwrap();
        assert!(matches!(
            write_verified(
                &destination,
                TEXT,
                NEW_FILE_PERMISSIONS,
                WriteMode::CreateNew
            ),
            Err(WriteError::AlreadyExists)
        ));
        assert_eq!(fs::read_to_string(&destination).unwrap(), "existing");
        assert_eq!(entries(folder.path()), ["Film.ro.srt"]);
    }

    /// SAFE-04.
    #[test]
    fn collision_overwrite() {
        let folder = tempfile::tempdir().unwrap();
        let destination = folder.path().join("Film.ro.srt");
        fs::write(&destination, "existing").unwrap();
        write_verified(&destination, TEXT, NEW_FILE_PERMISSIONS, WriteMode::Replace).unwrap();
        assert_eq!(fs::read_to_string(&destination).unwrap(), TEXT);
        assert_eq!(entries(folder.path()), ["Film.ro.srt"]);
    }

    /// SAFE-04 and SAFE-12.
    #[test]
    fn overwrite_never_replaces_a_link_or_a_read_only_file() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("target.srt");
        fs::write(&target, "target").unwrap();
        let link = folder.path().join("link.srt");
        symlink(&target, &link).unwrap();
        assert!(matches!(
            write_verified(&link, TEXT, NEW_FILE_PERMISSIONS, WriteMode::Replace),
            Err(WriteError::DestinationIsSymbolicLink)
        ));
        let read_only = folder.path().join("read-only.srt");
        fs::write(&read_only, "read-only").unwrap();
        fs::set_permissions(&read_only, Permissions::from_mode(0o444)).unwrap();
        assert!(matches!(
            write_verified(&read_only, TEXT, NEW_FILE_PERMISSIONS, WriteMode::Replace),
            Err(WriteError::DestinationIsReadOnly)
        ));
        assert_eq!(fs::read_to_string(&target).unwrap(), "target");
        assert_eq!(fs::read_to_string(&read_only).unwrap(), "read-only");
        assert_eq!(
            entries(folder.path()),
            ["link.srt", "read-only.srt", "target.srt"]
        );
    }

    /// SAFE-07: a folder that cannot be written leaves nothing behind.
    #[test]
    fn write_permission_failure_leaves_nothing() {
        let folder = tempfile::tempdir().unwrap();
        fs::set_permissions(folder.path(), Permissions::from_mode(0o555)).unwrap();
        let result = write_verified(
            &folder.path().join("Film.ro.srt"),
            TEXT,
            NEW_FILE_PERMISSIONS,
            WriteMode::CreateNew,
        );
        fs::set_permissions(folder.path(), Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            result,
            Err(WriteError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied
        ));
        assert!(entries(folder.path()).is_empty());
    }

    /// SAFE-10.
    #[test]
    fn fallback_is_only_for_file_systems_that_cannot_publish_exclusively() {
        for code in [libc::EINVAL, libc::ENOTSUP, libc::EPERM, libc::ENOSYS] {
            assert!(cannot_publish_without_replacing(
                &io::Error::from_raw_os_error(code)
            ));
        }
        for code in [libc::EACCES, libc::EEXIST, libc::ENOSPC] {
            assert!(!cannot_publish_without_replacing(
                &io::Error::from_raw_os_error(code)
            ));
        }
    }

    /// SAFE-10: the fallback still never replaces a name that exists.
    #[test]
    fn fallback_never_replaces_an_existing_name() {
        let folder = tempfile::tempdir().unwrap();
        let destination = folder.path().join("Film.ro.srt");
        fs::write(&destination, "existing").unwrap();
        let temporary = write_temporary(folder.path(), TEXT.as_bytes()).unwrap();
        assert!(matches!(
            publish_after_checking_name(temporary, &destination),
            Err(WriteError::AlreadyExists)
        ));
        assert_eq!(fs::read_to_string(&destination).unwrap(), "existing");
        assert_eq!(entries(folder.path()), ["Film.ro.srt"]);
        let free = folder.path().join("Film1.ro.srt");
        let temporary = write_temporary(folder.path(), TEXT.as_bytes()).unwrap();
        publish_after_checking_name(temporary, &free).unwrap();
        assert_eq!(fs::read_to_string(&free).unwrap(), TEXT);
    }
}
