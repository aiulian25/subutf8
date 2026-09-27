use std::env;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use subutf8_core::input_scan::AllowedArea;

use crate::access::{NoRandomness, allowed_hosts, generate_token};
use crate::constants::{
    ALLOWED_HOSTS_SEPARATOR, ALLOWED_HOSTS_VARIABLE, ANY_FREE_PORT, BROWSE_ROOTS_SEPARATOR,
    BROWSE_ROOTS_VARIABLE, CONFIG_FOLDER, CONFIG_HOME_VARIABLE, CONTAINER_ADDRESS, CONTAINER_MODE,
    CONTAINER_PORT, DEFAULT_BROWSE_ROOTS, DESKTOP_ADDRESS, DOWNLOAD_FOLDER_KEY,
    DOWNLOAD_FOLDER_NAME, HOME_PLACEHOLDER, HOME_VARIABLE, MODE_VARIABLE, ROOT_FOLDER,
    USER_FOLDERS_FILE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Desktop,
    Container,
}

/// Everything that differs between the desktop builds and Docker (TARGET table).
#[derive(Debug, Clone)]
pub struct Settings {
    pub mode: Mode,
    /// ACCESS-02: the desktop's access token. Docker has none: like other home-server apps it
    /// serves whoever can reach its port, and other websites are kept out by ACCESS-04 and
    /// ACCESS-05.
    pub token: Option<String>,
    pub allowed_hosts: Vec<String>,
    pub listen_address: SocketAddr,
    pub allowed_area: AllowedArea,
    pub browse_start: PathBuf,
    pub default_output_folder: PathBuf,
    /// ACCESS-08: files given on the command line, as the file manager's "Open with" does.
    pub files_to_open: Vec<PathBuf>,
}

impl Settings {
    /// Reads `SUBUTF8_MODE`, and in Docker `SUBUTF8_BROWSE_ROOTS` and `SUBUTF8_ALLOWED_HOSTS`.
    pub fn from_environment(files_to_open: Vec<PathBuf>) -> Result<Self, NoRandomness> {
        let is_container = env::var(MODE_VARIABLE).is_ok_and(|mode| mode == CONTAINER_MODE);
        if is_container {
            let extra_hosts = env::var(ALLOWED_HOSTS_VARIABLE).unwrap_or_default();
            let extra_roots = env::var(BROWSE_ROOTS_VARIABLE).unwrap_or_default();
            return Ok(container_settings(&extra_hosts, &extra_roots));
        }
        Ok(desktop_settings(files_to_open, generate_token()?))
    }
}

fn desktop_settings(files_to_open: Vec<PathBuf>, token: String) -> Settings {
    let home = env::var_os(HOME_VARIABLE).map_or_else(|| PathBuf::from(ROOT_FOLDER), PathBuf::from);
    Settings {
        mode: Mode::Desktop,
        token: Some(token),
        allowed_hosts: allowed_hosts(&[]),
        listen_address: SocketAddr::from((Ipv4Addr::from(DESKTOP_ADDRESS), ANY_FREE_PORT)),
        allowed_area: AllowedArea::new([PathBuf::from(ROOT_FOLDER)]),
        default_output_folder: downloads_folder(&home),
        browse_start: home,
        files_to_open,
    }
}

/// TARGET-02: folders mounted under `/mnt` or `/media`, and any listed in
/// `SUBUTF8_BROWSE_ROOTS`, are the only ones the container reads and writes.
fn container_settings(extra_hosts: &str, extra_roots: &str) -> Settings {
    let extra_hosts: Vec<String> = extra_hosts
        .split(ALLOWED_HOSTS_SEPARATOR)
        .map(str::to_owned)
        .collect();
    let roots = DEFAULT_BROWSE_ROOTS
        .iter()
        .copied()
        .chain(extra_roots.split(BROWSE_ROOTS_SEPARATOR))
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from);
    let allowed_area = AllowedArea::new(roots);
    let first_root = allowed_area
        .roots()
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(ROOT_FOLDER));
    Settings {
        mode: Mode::Container,
        token: None,
        allowed_hosts: allowed_hosts(&extra_hosts),
        listen_address: SocketAddr::from((Ipv4Addr::from(CONTAINER_ADDRESS), CONTAINER_PORT)),
        allowed_area,
        browse_start: first_root.clone(),
        default_output_folder: first_root,
        files_to_open: Vec::new(),
    }
}

/// The desktop's Downloads folder from `user-dirs.dirs`, then `~/Downloads`, then home.
fn downloads_folder(home: &Path) -> PathBuf {
    let config =
        env::var_os(CONFIG_HOME_VARIABLE).map_or_else(|| home.join(CONFIG_FOLDER), PathBuf::from);
    let configured = fs::read_to_string(config.join(USER_FOLDERS_FILE))
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                let value = line.trim().strip_prefix(DOWNLOAD_FOLDER_KEY)?;
                Some(PathBuf::from(
                    value
                        .trim_matches('"')
                        .replace(HOME_PLACEHOLDER, &home.to_string_lossy()),
                ))
            })
        });
    [configured, Some(home.join(DOWNLOAD_FOLDER_NAME))]
        .into_iter()
        .flatten()
        .find(|folder| folder.is_dir())
        .unwrap_or_else(|| home.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_listens_on_loopback_only_with_a_system_chosen_port() {
        let settings = desktop_settings(Vec::new(), String::from("token"));
        assert!(settings.listen_address.ip().is_loopback());
        assert_eq!(settings.listen_address.port(), ANY_FREE_PORT);
        assert_eq!(settings.token.as_deref(), Some("token"));
    }

    /// ACCESS-01, ACCESS-02, ACCESS-05 and TARGET-02.
    #[test]
    fn container_listens_on_its_fixed_port_and_uses_mounted_folders() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let extra_roots = format!("{}:{}: ", first.path().display(), second.path().display());
        let settings = container_settings("Subtitles.lan", &extra_roots);
        assert_eq!(settings.listen_address.port(), CONTAINER_PORT);
        assert!(settings.listen_address.ip().is_unspecified());
        assert_eq!(settings.token, None);
        assert!(
            settings
                .allowed_hosts
                .contains(&String::from("subtitles.lan"))
        );
        let roots = settings.allowed_area.roots();
        assert!(roots.contains(&fs::canonicalize(first.path()).unwrap()));
        assert!(roots.contains(&fs::canonicalize(second.path()).unwrap()));
        assert_eq!(settings.browse_start, roots[0]);
        assert_eq!(settings.default_output_folder, roots[0]);
    }

    #[test]
    fn downloads_folder_comes_from_the_desktop_settings() {
        let home = tempfile::tempdir().unwrap();
        let localized = home.path().join("Descărcări");
        fs::create_dir_all(&localized).unwrap();
        fs::create_dir_all(home.path().join(CONFIG_FOLDER)).unwrap();
        fs::write(
            home.path().join(CONFIG_FOLDER).join(USER_FOLDERS_FILE),
            "# comment\nXDG_DOWNLOAD_DIR=\"$HOME/Descărcări\"\n",
        )
        .unwrap();
        assert_eq!(downloads_folder(home.path()), localized);
        let bare_home = tempfile::tempdir().unwrap();
        assert_eq!(downloads_folder(bare_home.path()), bare_home.path());
    }
}
