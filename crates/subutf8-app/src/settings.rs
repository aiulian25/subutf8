use std::env;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use subutf8_core::input_scan::AllowedArea;

use crate::access::{NoRandomness, allowed_hosts, generate_token};
use crate::constants::{
    ALLOWED_HOSTS_SEPARATOR, ALLOWED_HOSTS_VARIABLE, ANY_FREE_PORT, BROWSE_ROOTS_SEPARATOR,
    BROWSE_ROOTS_VARIABLE, CONFIG_FOLDER, CONFIG_HOME_VARIABLE, CONTAINER_ADDRESS,
    CONTAINER_DATA_FOLDER, CONTAINER_MODE, CONTAINER_PORT, DATA_FOLDER_NAME, DATA_FOLDER_VARIABLE,
    DEFAULT_BROWSE_ROOTS, DEFAULT_WATCH_INTERVAL, DESKTOP_ADDRESS, DOWNLOAD_FOLDER_KEY,
    DOWNLOAD_FOLDER_NAME, HOME_PLACEHOLDER, HOME_VARIABLE, MINIMUM_WATCH_INTERVAL, MODE_VARIABLE,
    OUTPUT_ROOT, ROOT_FOLDER, UPDATE_CHECK_OFF, UPDATE_CHECK_VARIABLE, USER_FOLDERS_FILE,
    WATCH_INTERVAL_VARIABLE,
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
    /// TARGET-04: Docker has `/output` mounted, so outputs go there, by day, until changed.
    pub has_output_mount: bool,
    /// SET-02.
    pub data_folder: PathBuf,
    /// WATCH-01.
    pub watch_interval: Duration,
    /// UPDATE-01.
    pub update_check_allowed: bool,
    /// ACCESS-08: files given on the command line, as the file manager's "Open with" does.
    pub files_to_open: Vec<PathBuf>,
}

impl Settings {
    /// Reads `SUBUTF8_MODE`, in Docker `SUBUTF8_BROWSE_ROOTS` and `SUBUTF8_ALLOWED_HOSTS`, and
    /// on both `SUBUTF8_DATA_DIR`, `SUBUTF8_WATCH_INTERVAL` and `SUBUTF8_UPDATE_CHECK`.
    pub fn from_environment(files_to_open: Vec<PathBuf>) -> Result<Self, NoRandomness> {
        let is_container = env::var(MODE_VARIABLE).is_ok_and(|mode| mode == CONTAINER_MODE);
        let mut settings = if is_container {
            let extra_hosts = env::var(ALLOWED_HOSTS_VARIABLE).unwrap_or_default();
            let extra_roots = env::var(BROWSE_ROOTS_VARIABLE).unwrap_or_default();
            container_settings(&extra_hosts, &extra_roots, Path::new(OUTPUT_ROOT))
        } else {
            desktop_settings(files_to_open, generate_token()?)
        };
        if let Some(folder) = env::var_os(DATA_FOLDER_VARIABLE).filter(|folder| !folder.is_empty())
        {
            settings.data_folder = PathBuf::from(folder);
        }
        settings.watch_interval = watch_interval(env::var(WATCH_INTERVAL_VARIABLE).ok().as_deref());
        settings.update_check_allowed =
            env::var(UPDATE_CHECK_VARIABLE).map_or(true, |value| value.trim() != UPDATE_CHECK_OFF);
        Ok(settings)
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
        has_output_mount: false,
        data_folder: config_folder(&home).join(DATA_FOLDER_NAME),
        watch_interval: DEFAULT_WATCH_INTERVAL,
        update_check_allowed: true,
        browse_start: home,
        files_to_open,
    }
}

/// TARGET-02 and TARGET-04: folders mounted under `/mnt`, `/media` or at `/output`, and any
/// listed in `SUBUTF8_BROWSE_ROOTS`, are the only ones the container reads and writes.
fn container_settings(extra_hosts: &str, extra_roots: &str, output_root: &Path) -> Settings {
    let extra_hosts: Vec<String> = extra_hosts
        .split(ALLOWED_HOSTS_SEPARATOR)
        .map(str::to_owned)
        .collect();
    let roots = DEFAULT_BROWSE_ROOTS
        .iter()
        .copied()
        .map(PathBuf::from)
        .chain([output_root.to_path_buf()])
        .chain(
            extra_roots
                .split(BROWSE_ROOTS_SEPARATOR)
                .map(str::trim)
                .filter(|root| !root.is_empty())
                .map(PathBuf::from),
        );
    let allowed_area = AllowedArea::new(roots);
    let first_root = allowed_area
        .roots()
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(ROOT_FOLDER));
    let output_mount = fs::canonicalize(output_root)
        .ok()
        .filter(|output| allowed_area.roots().contains(output));
    Settings {
        mode: Mode::Container,
        token: None,
        allowed_hosts: allowed_hosts(&extra_hosts),
        listen_address: SocketAddr::from((Ipv4Addr::from(CONTAINER_ADDRESS), CONTAINER_PORT)),
        allowed_area,
        browse_start: first_root.clone(),
        has_output_mount: output_mount.is_some(),
        default_output_folder: output_mount.unwrap_or(first_root),
        data_folder: PathBuf::from(CONTAINER_DATA_FOLDER),
        watch_interval: DEFAULT_WATCH_INTERVAL,
        update_check_allowed: true,
        files_to_open: Vec::new(),
    }
}

/// WATCH-01: whole seconds, never below the minimum.
fn watch_interval(seconds: Option<&str>) -> Duration {
    seconds
        .and_then(|seconds| seconds.trim().parse().ok())
        .map_or(DEFAULT_WATCH_INTERVAL, Duration::from_secs)
        .max(MINIMUM_WATCH_INTERVAL)
}

fn config_folder(home: &Path) -> PathBuf {
    env::var_os(CONFIG_HOME_VARIABLE).map_or_else(|| home.join(CONFIG_FOLDER), PathBuf::from)
}

/// The desktop's Downloads folder from `user-dirs.dirs`, then `~/Downloads`, then home.
fn downloads_folder(home: &Path) -> PathBuf {
    let configured = fs::read_to_string(config_folder(home).join(USER_FOLDERS_FILE))
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
        assert!(settings.data_folder.ends_with(DATA_FOLDER_NAME));
    }

    /// ACCESS-01, ACCESS-02, ACCESS-05 and TARGET-02.
    #[test]
    fn container_listens_on_its_fixed_port_and_uses_mounted_folders() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let extra_roots = format!("{}:{}: ", first.path().display(), second.path().display());
        let settings =
            container_settings("Subtitles.lan", &extra_roots, &first.path().join("missing"));
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
        assert!(!settings.has_output_mount);
        assert_eq!(settings.data_folder, Path::new(CONTAINER_DATA_FOLDER));
    }

    /// TARGET-04.
    #[test]
    fn mounted_output_folder_is_the_default() {
        let output = tempfile::tempdir().unwrap();
        let settings = container_settings("", "", output.path());
        let output = fs::canonicalize(output.path()).unwrap();
        assert!(settings.has_output_mount);
        assert_eq!(settings.default_output_folder, output);
        assert!(settings.allowed_area.roots().contains(&output));
    }

    /// WATCH-01.
    #[test]
    fn watch_interval_has_a_default_and_a_minimum() {
        assert_eq!(watch_interval(None), DEFAULT_WATCH_INTERVAL);
        assert_eq!(watch_interval(Some("soon")), DEFAULT_WATCH_INTERVAL);
        assert_eq!(watch_interval(Some("0")), MINIMUM_WATCH_INTERVAL);
        assert_eq!(watch_interval(Some(" 90 ")), Duration::from_secs(90));
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
