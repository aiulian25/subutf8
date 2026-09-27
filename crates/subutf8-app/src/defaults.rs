use std::fs::{self, Permissions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use subutf8_core::language::SubtitleLanguage;
use subutf8_core::safe_write::{WriteError, WriteMode, write_verified};

use crate::constants::{DEFAULTS_FILE_NAME, PRIVATE_FILE_PERMISSIONS, PRIVATE_FOLDER_PERMISSIONS};
use crate::session::SessionSettings;
use crate::settings::Settings;
use crate::views::{CollisionName, DestinationName};

/// UI-13.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl Theme {
    /// The page's `data-theme` value.
    pub fn attribute_value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// SET-01: what a person saves in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Defaults {
    pub language: Option<String>,
    pub destination: DestinationName,
    pub output_folder: PathBuf,
    pub organise_by_day: bool,
    pub collision_policy: CollisionName,
    pub theme: Theme,
    pub watch_folders: Vec<PathBuf>,
    pub update_check: bool,
}

impl Defaults {
    /// SET-03: before anything is saved. Docker with `/output` mounted writes there, by day.
    pub fn factory(settings: &Settings) -> Self {
        let destination = if settings.has_output_mount {
            DestinationName::OutputFolder
        } else {
            DestinationName::BesideOriginals
        };
        Self {
            language: None,
            destination,
            output_folder: settings.default_output_folder.clone(),
            organise_by_day: settings.has_output_mount,
            collision_policy: CollisionName::Skip,
            theme: Theme::System,
            watch_folders: Vec::new(),
            update_check: true,
        }
    }

    /// UI-05: the bottom bar starts from these.
    pub fn session_settings(&self) -> SessionSettings {
        SessionSettings {
            language: self
                .language
                .as_deref()
                .and_then(|tag| SubtitleLanguage::parse(tag).ok()),
            destination: self.destination.into(),
            output_folder: self.output_folder.clone(),
            organise_by_day: self.organise_by_day,
            collision_policy: self.collision_policy.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreProblem {
    /// The file is not valid; the factory defaults apply until Save replaces it.
    Damaged,
    Unreadable(io::ErrorKind),
    NotSaved(io::ErrorKind),
}

/// SET-02: the saved defaults, in a file only the user can read.
#[derive(Debug)]
pub struct DefaultsStore {
    path: PathBuf,
    pub current: Defaults,
    pub factory: Defaults,
    pub problem: Option<StoreProblem>,
}

impl DefaultsStore {
    pub fn load(data_folder: &Path, factory: Defaults) -> Self {
        let path = data_folder.join(DEFAULTS_FILE_NAME);
        let (current, problem) = match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(saved) => (saved, None),
                Err(_) => (factory.clone(), Some(StoreProblem::Damaged)),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => (factory.clone(), None),
            Err(error) => (
                factory.clone(),
                Some(StoreProblem::Unreadable(error.kind())),
            ),
        };
        Self {
            path,
            current,
            factory,
            problem,
        }
    }

    /// Values are checked by the caller (SET-04).
    pub fn save(&mut self, defaults: Defaults) -> Result<(), StoreProblem> {
        let saved = serde_json::to_string_pretty(&defaults)
            .map_err(io::Error::other)
            .and_then(|text| write_private_file(&self.path, &text));
        if let Err(error) = saved {
            return Err(StoreProblem::NotSaved(error.kind()));
        }
        self.current = defaults;
        self.problem = None;
        Ok(())
    }

    /// SET-03: forgets what was saved, so later factory defaults apply too.
    pub fn restore(&mut self) -> Result<(), StoreProblem> {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(StoreProblem::NotSaved(error.kind())),
        }
        self.current = self.factory.clone();
        self.problem = None;
        Ok(())
    }
}

/// SET-02: SubUTF8's own folder is private when SubUTF8 creates it; files are replaced in one
/// step, readable only by the user, and never through a link.
pub fn write_private_file(path: &Path, text: &str) -> io::Result<()> {
    create_private_folder(path)?;
    write_verified(path, text, PRIVATE_FILE_PERMISSIONS, WriteMode::Replace).map_err(|error| {
        match error {
            WriteError::Io(error) => error,
            WriteError::VerificationFailed => io::ErrorKind::InvalidData.into(),
            WriteError::AlreadyExists
            | WriteError::DestinationIsSymbolicLink
            | WriteError::DestinationIsReadOnly => io::ErrorKind::PermissionDenied.into(),
        }
    })
}

pub fn create_private_folder(file: &Path) -> io::Result<()> {
    let folder = file.parent().ok_or(io::ErrorKind::InvalidInput)?;
    if folder.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(folder)?;
    fs::set_permissions(folder, Permissions::from_mode(PRIVATE_FOLDER_PERMISSIONS))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERMISSION_BITS: u32 = 0o777;

    fn example(folder: &Path) -> Defaults {
        Defaults {
            language: Some(String::from("ro")),
            destination: DestinationName::OutputFolder,
            output_folder: folder.to_path_buf(),
            organise_by_day: true,
            collision_policy: CollisionName::Rename,
            theme: Theme::Dark,
            watch_folders: vec![folder.join("incoming")],
            update_check: false,
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & PERMISSION_BITS
    }

    /// SET-02.
    #[test]
    fn saved_defaults_come_back_and_stay_private() {
        let home = tempfile::tempdir().unwrap();
        let data = home.path().join("config/subutf8");
        let factory = example(home.path());
        let mut store = DefaultsStore::load(&data, factory.clone());
        assert_eq!(store.current, factory);
        assert_eq!(store.problem, None);
        let mut changed = factory.clone();
        changed.theme = Theme::Light;
        changed.language = None;
        store.save(changed.clone()).unwrap();
        assert_eq!(mode(&data), PRIVATE_FOLDER_PERMISSIONS);
        assert_eq!(
            mode(&data.join(DEFAULTS_FILE_NAME)),
            PRIVATE_FILE_PERMISSIONS
        );
        assert_eq!(DefaultsStore::load(&data, factory).current, changed);
    }

    /// SET-02: a damaged file is reported and left for Save to replace.
    #[test]
    fn damaged_file_falls_back_to_the_factory_defaults() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join(DEFAULTS_FILE_NAME);
        fs::write(&file, "{\"theme\":\"purple\"}").unwrap();
        let factory = example(data.path());
        let store = DefaultsStore::load(data.path(), factory.clone());
        assert_eq!(store.current, factory);
        assert_eq!(store.problem, Some(StoreProblem::Damaged));
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"theme\":\"purple\"}");
    }

    /// SET-02: nothing is written through a link planted at the file's name.
    #[test]
    fn a_link_is_never_followed() {
        let data = tempfile::tempdir().unwrap();
        let target = data.path().join("elsewhere.json");
        fs::write(&target, "kept").unwrap();
        std::os::unix::fs::symlink(&target, data.path().join(DEFAULTS_FILE_NAME)).unwrap();
        let mut store = DefaultsStore::load(data.path(), example(data.path()));
        let saved = store.save(example(data.path()));
        assert_eq!(
            saved,
            Err(StoreProblem::NotSaved(io::ErrorKind::PermissionDenied))
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "kept");
    }
}
