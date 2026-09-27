use std::env;
use std::fs::{self, File, Permissions};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ring::digest::{Context, SHA256};
use serde::Deserialize;
use subutf8_core::output_naming::dropped_file_name;
use tempfile::Builder;
use ureq::http::{Response, Uri, header};
use ureq::{Agent, Body};

use crate::access::hexadecimal;
use crate::constants::{
    ALLOWED_DOWNLOAD_DOMAINS, APPIMAGE_ASSET, APPIMAGE_VARIABLE, CHECKSUM_BINARY_MARK,
    CHECKSUMS_ASSET, DEB_ASSET, DEB_INSTALL, DOWNLOAD_BUFFER_BYTES, DOWNLOAD_CONNECT_TIMEOUT,
    DOWNLOAD_TIMEOUT, DPKG_PROGRAM, DPKG_STATUS_FLAG, EXECUTABLE_PERMISSIONS,
    EXECUTE_PERMISSION_BITS, GITHUB_ACCEPT, HTTPS_SCHEME, INSTALLED_PROGRAM, LATEST_RELEASE_URL,
    MAXIMUM_CHECKSUMS_BYTES, MAXIMUM_PACKAGE_BYTES, MAXIMUM_REDIRECTS, MAXIMUM_RELEASE_BYTES,
    NOT_AUTHORIZED_EXIT_CODES, PACKAGE_FILE_PERMISSIONS, PARTIAL_DOWNLOAD_PREFIX,
    PARTIAL_DOWNLOAD_SUFFIX, PATH_VARIABLE, PRIVILEGE_PROGRAM, PROGRAM_NAME, REPLACEMENT_SUFFIX,
    RPM_ASSET, RPM_INSTALLERS, RPM_PROGRAM, RPM_QUERY_FLAG, SHA256_DIGEST_PREFIX,
    UPDATE_CHECK_TIMEOUT, USER_AGENT, VERSION_SEPARATOR, VERSION_TAG_PREFIX,
};
use crate::settings::Mode;

/// UPDATE-04: how the running copy was installed, which decides how it is updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Package {
    Docker,
    Deb,
    Rpm,
    AppImage(PathBuf),
    Other,
}

/// UPDATE-02: one file of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// GitHub's own checksum, such as `sha256:…`.
    pub digest: Option<String>,
}

/// UPDATE-02: the newest release on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// The release notes, only ever on GitHub (UPDATE-03).
    pub page: Option<String>,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateProblem {
    CheckFailed,
    NoReleaseFile,
    DownloadFailed,
    NotGithub,
    ChecksumMismatch,
    NoChecksum,
    NoPrivilegeProgram,
    NotAuthorized,
    InstallFailed,
    ReplaceFailed,
}

/// A verified download, checked again right before it is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Idle,
    Downloading { received: u64, total: u64 },
    Installing,
    Installed,
    Failed(UpdateProblem),
}

/// UPDATE-01 to UPDATE-05: what the page shows about updates.
#[derive(Debug)]
pub struct UpdateState {
    pub package: Package,
    pub last_check: Option<Instant>,
    pub check_failed: bool,
    pub newer: Option<Release>,
    pub downloaded: Option<Downloaded>,
    pub step: Step,
}

impl UpdateState {
    pub fn new(package: Package) -> Self {
        Self {
            package,
            last_check: None,
            check_failed: false,
            newer: None,
            downloaded: None,
            step: Step::Idle,
        }
    }

    /// A download, an install or a restart is under way or waiting.
    pub fn is_busy(&self) -> bool {
        matches!(
            self.step,
            Step::Downloading { .. } | Step::Installing | Step::Installed
        )
    }

    pub fn record_check(&mut self, latest: Result<Release, UpdateProblem>) {
        self.last_check = Some(Instant::now());
        match latest {
            Ok(release) => {
                self.check_failed = false;
                self.newer =
                    is_newer(&release.version, env!("CARGO_PKG_VERSION")).then_some(release);
            }
            Err(_) => self.check_failed = true,
        }
    }

    /// UPDATE-04: the release file this copy installs, if it can update itself.
    pub fn release_file(&self) -> Option<&Asset> {
        pick_asset(self.newer.as_ref()?, &self.package)
    }
}

/// UPDATE-04: from `$APPIMAGE`, then the package that owns `/usr/bin/subutf8`.
pub fn detect_package(mode: Mode) -> Package {
    if mode == Mode::Container {
        return Package::Docker;
    }
    if let Some(appimage) = env::var_os(APPIMAGE_VARIABLE).filter(|value| !value.is_empty()) {
        return Package::AppImage(PathBuf::from(appimage));
    }
    let is_installed =
        env::current_exe().is_ok_and(|program| program == Path::new(INSTALLED_PROGRAM));
    if !is_installed {
        return Package::Other;
    }
    if succeeds(DPKG_PROGRAM, &[DPKG_STATUS_FLAG, PROGRAM_NAME]) {
        return Package::Deb;
    }
    if succeeds(RPM_PROGRAM, &[RPM_QUERY_FLAG, PROGRAM_NAME]) {
        return Package::Rpm;
    }
    Package::Other
}

fn succeeds(program: &str, arguments: &[&str]) -> bool {
    find_program(program).is_some_and(|program| {
        Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

fn find_program(name: &str) -> Option<PathBuf> {
    let folders = env::var_os(PATH_VARIABLE)?;
    env::split_paths(&folders)
        .map(|folder| folder.join(name))
        .find(|candidate| {
            fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & EXECUTE_PERMISSION_BITS != 0
            })
        })
}

/// Every request goes out over HTTPS only and follows no redirect by itself (UPDATE-03).
fn agent(total: Duration) -> Agent {
    Agent::config_builder()
        .https_only(true)
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .timeout_connect(Some(DOWNLOAD_CONNECT_TIMEOUT.min(total)))
        .timeout_global(Some(total))
        .build()
        .into()
}

/// UPDATE-02: asks GitHub for the newest release, waiting a few seconds at most.
pub fn fetch_latest() -> Result<Release, UpdateProblem> {
    let mut response = agent(UPDATE_CHECK_TIMEOUT)
        .get(LATEST_RELEASE_URL)
        .header(header::ACCEPT, GITHUB_ACCEPT)
        .header(header::USER_AGENT, USER_AGENT)
        .call()
        .map_err(|_| UpdateProblem::CheckFailed)?;
    if !response.status().is_success() {
        return Err(UpdateProblem::CheckFailed);
    }
    let text = response
        .body_mut()
        .with_config()
        .limit(MAXIMUM_RELEASE_BYTES)
        .read_to_string()
        .map_err(|_| UpdateProblem::CheckFailed)?;
    parse_release(&text)
}

fn parse_release(text: &str) -> Result<Release, UpdateProblem> {
    let release: GithubRelease =
        serde_json::from_str(text).map_err(|_| UpdateProblem::CheckFailed)?;
    let page = check_github_address(&release.html_url)
        .ok()
        .map(|()| release.html_url);
    Ok(Release {
        version: release
            .tag_name
            .trim_start_matches(VERSION_TAG_PREFIX)
            .to_owned(),
        page,
        assets: release
            .assets
            .into_iter()
            .map(|asset| Asset {
                name: asset.name,
                url: asset.browser_download_url,
                size: asset.size,
                digest: asset.digest,
            })
            .collect(),
    })
}

/// UPDATE-02: numbers compared in order, so 1.10.0 is newer than 1.9.0.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (version_numbers(candidate), version_numbers(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn version_numbers(version: &str) -> Option<Vec<u64>> {
    version
        .trim_start_matches(VERSION_TAG_PREFIX)
        .split(VERSION_SEPARATOR)
        .map(|part| part.parse().ok())
        .collect()
}

/// UPDATE-04: the file for this kind of install, named as the release script names it.
pub fn pick_asset<'release>(
    release: &'release Release,
    package: &Package,
) -> Option<&'release Asset> {
    let (prefix, suffix) = match package {
        Package::Deb => DEB_ASSET,
        Package::Rpm => RPM_ASSET,
        Package::AppImage(_) => APPIMAGE_ASSET,
        Package::Docker | Package::Other => return None,
    };
    release.assets.iter().find(|asset| {
        let is_plain_name = dropped_file_name(&asset.name).is_ok_and(|name| name == asset.name);
        is_plain_name && asset.name.starts_with(prefix) && asset.name.ends_with(suffix)
    })
}

/// UPDATE-03: HTTPS, and a GitHub host.
fn check_github_address(url: &str) -> Result<(), UpdateProblem> {
    let address: Uri = url.parse().map_err(|_| UpdateProblem::NotGithub)?;
    let host = address.host().unwrap_or_default().to_ascii_lowercase();
    let is_github = ALLOWED_DOWNLOAD_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|subdomain| subdomain.ends_with('.'))
    });
    if address.scheme_str() == Some(HTTPS_SCHEME) && is_github {
        return Ok(());
    }
    Err(UpdateProblem::NotGithub)
}

/// UPDATE-03: every address on the way, redirects included, must be on GitHub.
fn get_on_github(agent: &Agent, url: &str) -> Result<Response<Body>, UpdateProblem> {
    let mut url = url.to_owned();
    for _ in 0..=MAXIMUM_REDIRECTS {
        check_github_address(&url)?;
        let response = agent
            .get(&url)
            .header(header::USER_AGENT, USER_AGENT)
            .call()
            .map_err(|_| UpdateProblem::DownloadFailed)?;
        let status = response.status();
        if !status.is_redirection() {
            return status
                .is_success()
                .then_some(response)
                .ok_or(UpdateProblem::DownloadFailed);
        }
        url = response
            .headers()
            .get(header::LOCATION)
            .and_then(|location| location.to_str().ok())
            .ok_or(UpdateProblem::DownloadFailed)?
            .to_owned();
    }
    Err(UpdateProblem::DownloadFailed)
}

/// UPDATE-03: downloads into `folder` through a private temporary file, which becomes the
/// release file only when its size and SHA-256 match.
pub fn download(
    release: &Release,
    asset: &Asset,
    folder: &Path,
    mut on_progress: impl FnMut(u64),
) -> Result<Downloaded, UpdateProblem> {
    if asset.size > MAXIMUM_PACKAGE_BYTES {
        return Err(UpdateProblem::DownloadFailed);
    }
    let agent = agent(DOWNLOAD_TIMEOUT);
    let expected = expected_sha256(asset, checksums(&agent, release).as_deref())?;
    let response = get_on_github(&agent, &asset.url)?;
    // The reader refuses a body that reaches its limit, so the limit leaves one byte over the
    // expected size; the size is then checked exactly.
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(asset.size.saturating_add(1))
        .reader();
    let mut part = Builder::new()
        .prefix(PARTIAL_DOWNLOAD_PREFIX)
        .suffix(PARTIAL_DOWNLOAD_SUFFIX)
        .tempfile_in(folder)
        .map_err(|_| UpdateProblem::DownloadFailed)?;
    let (size, sha256) = copy_and_hash(&mut reader, part.as_file_mut(), &mut on_progress)
        .map_err(|_| UpdateProblem::DownloadFailed)?;
    if size != asset.size || sha256 != expected {
        return Err(UpdateProblem::ChecksumMismatch);
    }
    let is_appimage = asset.name.ends_with(APPIMAGE_ASSET.1);
    let permissions = if is_appimage {
        EXECUTABLE_PERMISSIONS
    } else {
        PACKAGE_FILE_PERMISSIONS
    };
    let path = folder.join(&asset.name);
    fs::set_permissions(part.path(), Permissions::from_mode(permissions))
        .map_err(|_| UpdateProblem::DownloadFailed)?;
    part.persist(&path)
        .map_err(|_| UpdateProblem::DownloadFailed)?;
    Ok(Downloaded { path, size, sha256 })
}

fn checksums(agent: &Agent, release: &Release) -> Option<String> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == CHECKSUMS_ASSET)?;
    let mut response = get_on_github(agent, &asset.url).ok()?;
    response
        .body_mut()
        .with_config()
        .limit(MAXIMUM_CHECKSUMS_BYTES)
        .read_to_string()
        .ok()
}

/// UPDATE-03: GitHub's checksum and the release's `SHA256SUMS`; where both exist they must
/// agree, and one of them must exist.
fn expected_sha256(asset: &Asset, checksums: Option<&str>) -> Result<String, UpdateProblem> {
    let from_github = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix(SHA256_DIGEST_PREFIX))
        .map(str::to_ascii_lowercase);
    let from_list = checksums.and_then(|text| {
        text.lines().find_map(|line| {
            let (hash, name) = line.split_once(char::is_whitespace)?;
            let name = name.trim().trim_start_matches(CHECKSUM_BINARY_MARK);
            (name == asset.name).then(|| hash.to_ascii_lowercase())
        })
    });
    match (from_github, from_list) {
        (Some(github), Some(listed)) if github != listed => Err(UpdateProblem::ChecksumMismatch),
        (Some(hash), _) | (None, Some(hash)) => Ok(hash),
        (None, None) => Err(UpdateProblem::NoChecksum),
    }
}

fn copy_and_hash(
    reader: &mut impl Read,
    writer: &mut impl Write,
    on_progress: &mut impl FnMut(u64),
) -> io::Result<(u64, String)> {
    let mut context = Context::new(&SHA256);
    let mut buffer = vec![0; DOWNLOAD_BUFFER_BYTES];
    let mut size = 0;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        context.update(&buffer[..count]);
        writer.write_all(&buffer[..count])?;
        size += count as u64;
        on_progress(size);
    }
    writer.flush()?;
    Ok((size, hexadecimal(context.finish().as_ref())))
}

/// UPDATE-04: the same file that was verified, checked again so nothing swapped in since is
/// installed.
fn verify(path: &Path, downloaded: &Downloaded) -> Result<(), UpdateProblem> {
    let mut file = File::open(path).map_err(|_| UpdateProblem::ChecksumMismatch)?;
    let (size, sha256) = copy_and_hash(&mut file, &mut io::sink(), &mut |_| {})
        .map_err(|_| UpdateProblem::ChecksumMismatch)?;
    if size != downloaded.size || sha256 != downloaded.sha256 {
        return Err(UpdateProblem::ChecksumMismatch);
    }
    Ok(())
}

/// UPDATE-04: the package manager installs the file through pkexec, which asks for the
/// password in the system's own dialog; SubUTF8 itself never runs as root. An AppImage
/// replaces its own file.
pub fn install(downloaded: &Downloaded, package: &Package) -> Result<(), UpdateProblem> {
    verify(&downloaded.path, downloaded)?;
    match package {
        Package::AppImage(target) => replace_appimage(downloaded, target),
        Package::Deb => run_privileged(DEB_INSTALL, &downloaded.path),
        Package::Rpm => {
            let installer = RPM_INSTALLERS
                .into_iter()
                .find(|(program, _)| find_program(program).is_some())
                .ok_or(UpdateProblem::InstallFailed)?;
            run_privileged(installer, &downloaded.path)
        }
        Package::Docker | Package::Other => Err(UpdateProblem::NoReleaseFile),
    }
}

fn run_privileged((program, arguments): (&str, &[&str]), file: &Path) -> Result<(), UpdateProblem> {
    let privilege = find_program(PRIVILEGE_PROGRAM).ok_or(UpdateProblem::NoPrivilegeProgram)?;
    let program = find_program(program).ok_or(UpdateProblem::InstallFailed)?;
    let status = Command::new(privilege)
        .arg(program)
        .args(arguments)
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| UpdateProblem::InstallFailed)?;
    match status.code() {
        Some(0) => Ok(()),
        Some(code) if NOT_AUTHORIZED_EXIT_CODES.contains(&code) => {
            Err(UpdateProblem::NotAuthorized)
        }
        _ => Err(UpdateProblem::InstallFailed),
    }
}

/// UPDATE-04: copied beside the running AppImage, checked, then renamed over it in one step.
fn replace_appimage(downloaded: &Downloaded, target: &Path) -> Result<(), UpdateProblem> {
    let mut name = target
        .file_name()
        .ok_or(UpdateProblem::ReplaceFailed)?
        .to_os_string();
    name.push(REPLACEMENT_SUFFIX);
    let replacement = target.with_file_name(name);
    let replaced = fs::copy(&downloaded.path, &replacement)
        .map_err(|_| UpdateProblem::ReplaceFailed)
        .and_then(|_| verify(&replacement, downloaded))
        .and_then(|()| {
            fs::set_permissions(&replacement, Permissions::from_mode(EXECUTABLE_PERMISSIONS))
                .and_then(|()| fs::rename(&replacement, target))
                .map_err(|_| UpdateProblem::ReplaceFailed)
        });
    if replaced.is_err() {
        let _ = fs::remove_file(&replacement);
    }
    replaced
}

/// UPDATE-05: starts the updated copy; the caller then stops this one.
pub fn start_new_copy(package: &Package) -> io::Result<()> {
    let program = match package {
        Package::AppImage(target) => target.clone(),
        Package::Deb | Package::Rpm => PathBuf::from(INSTALLED_PROGRAM),
        Package::Docker | Package::Other => return Err(io::ErrorKind::Unsupported.into()),
    };
    // Tokio waits for the child in the background, so none is left behind as a zombie.
    tokio::process::Command::new(program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE_TEXT: &str = "package";
    /// `printf package | sha256sum`
    const PACKAGE_SHA256: &str = "bc4a71180870f7945155fbb02f4b0a2e3faa2a62d6d31b7039013055ed19869a";
    const RELEASE: &str = r#"{
        "tag_name": "v1.2.0",
        "html_url": "https://github.com/aiulian25/subutf8/releases/tag/v1.2.0",
        "draft": false,
        "assets": [
            {"name": "SHA256SUMS", "browser_download_url": "https://github.com/a/SHA256SUMS", "size": 10},
            {"name": "subutf8_1.2.0_amd64.deb", "browser_download_url": "https://github.com/a/d", "size": 3,
             "digest": "sha256:ABC"},
            {"name": "subutf8-1.2.0-1.x86_64.rpm", "browser_download_url": "https://github.com/a/r", "size": 3},
            {"name": "SubUTF8-1.2.0-x86_64.AppImage", "browser_download_url": "https://github.com/a/i", "size": 3},
            {"name": "../SubUTF8-x86_64.AppImage", "browser_download_url": "https://github.com/a/x", "size": 3}
        ]
    }"#;

    /// UPDATE-02.
    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(is_newer("v2.0.0", "1.99.99"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("1.1.0-beta", "1.0.0"));
    }

    /// UPDATE-02 and UPDATE-04.
    #[test]
    fn each_install_picks_its_own_release_file() {
        let release = parse_release(RELEASE).unwrap();
        assert_eq!(release.version, "1.2.0");
        let picked =
            |package: Package| pick_asset(&release, &package).map(|asset| asset.name.clone());
        assert_eq!(picked(Package::Deb).unwrap(), "subutf8_1.2.0_amd64.deb");
        assert_eq!(picked(Package::Rpm).unwrap(), "subutf8-1.2.0-1.x86_64.rpm");
        assert_eq!(
            picked(Package::AppImage(PathBuf::from("/opt/SubUTF8.AppImage"))).unwrap(),
            "SubUTF8-1.2.0-x86_64.AppImage"
        );
        assert_eq!(picked(Package::Docker), None);
        assert_eq!(picked(Package::Other), None);
    }

    /// UPDATE-03.
    #[test]
    fn only_github_over_https_is_used() {
        for url in [
            "https://github.com/aiulian25/subutf8/releases/download/v1.2.0/x.deb",
            "https://objects.githubusercontent.com/github-production-release-asset/1",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1",
            "https://api.github.com/repos/aiulian25/subutf8/releases/latest",
        ] {
            assert_eq!(check_github_address(url), Ok(()), "{url}");
        }
        for url in [
            "http://github.com/x.deb",
            "https://evilgithub.com/x.deb",
            "https://github.com.evil.example/x.deb",
            "https://github.com@evil.example/x.deb",
            "javascript:alert(1)",
            "/relative/x.deb",
        ] {
            assert_eq!(
                check_github_address(url),
                Err(UpdateProblem::NotGithub),
                "{url}"
            );
        }
        let hostile = RELEASE.replace("https://github.com/aiulian25", "javascript:alert");
        assert_eq!(parse_release(&hostile).unwrap().page, None);
    }

    /// UPDATE-03.
    #[test]
    fn checksums_must_exist_and_agree() {
        let asset = |digest: Option<&str>| Asset {
            name: String::from("subutf8_1.2.0_amd64.deb"),
            url: String::new(),
            size: 3,
            digest: digest.map(str::to_owned),
        };
        let list = "abc  subutf8_1.2.0_amd64.deb\nfff *SubUTF8-1.2.0-x86_64.AppImage\n";
        assert_eq!(
            expected_sha256(&asset(Some("sha256:ABC")), Some(list)),
            Ok(String::from("abc"))
        );
        assert_eq!(
            expected_sha256(&asset(None), Some(list)),
            Ok(String::from("abc"))
        );
        assert_eq!(
            expected_sha256(&asset(Some("sha256:abc")), None),
            Ok(String::from("abc"))
        );
        assert_eq!(
            expected_sha256(&asset(Some("sha256:abd")), Some(list)),
            Err(UpdateProblem::ChecksumMismatch)
        );
        assert_eq!(
            expected_sha256(&asset(None), None),
            Err(UpdateProblem::NoChecksum)
        );
        assert_eq!(
            expected_sha256(&asset(Some("md5:abc")), Some("")),
            Err(UpdateProblem::NoChecksum)
        );
    }

    /// UPDATE-04: a file changed after its download is never installed.
    #[test]
    fn a_changed_download_fails_verification() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("subutf8_1.2.0_amd64.deb");
        fs::write(&path, PACKAGE_TEXT).unwrap();
        let (size, sha256) = copy_and_hash(
            &mut File::open(&path).unwrap(),
            &mut io::sink(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(size, PACKAGE_TEXT.len() as u64);
        assert_eq!(sha256, PACKAGE_SHA256);
        let downloaded = Downloaded {
            path: path.clone(),
            size,
            sha256,
        };
        assert_eq!(verify(&path, &downloaded), Ok(()));
        fs::write(&path, "pack4ge").unwrap();
        assert_eq!(
            verify(&path, &downloaded),
            Err(UpdateProblem::ChecksumMismatch)
        );
    }

    /// UPDATE-02 and UPDATE-03 with the release published on GitHub, redirects and both
    /// checksums included. It needs the network, so it runs only when asked:
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore = "downloads from GitHub"]
    fn the_published_release_downloads_and_verifies() {
        let release = fetch_latest().unwrap();
        let asset = pick_asset(&release, &Package::Deb).unwrap();
        let folder = tempfile::tempdir().unwrap();
        let mut received = 0;
        let downloaded =
            download(&release, asset, folder.path(), |bytes| received = bytes).unwrap();
        assert_eq!(received, asset.size);
        assert_eq!(downloaded.path, folder.path().join(&asset.name));
        assert_eq!(verify(&downloaded.path, &downloaded), Ok(()));
        let mode = fs::metadata(&downloaded.path).unwrap().permissions().mode();
        assert_eq!(mode & EXECUTABLE_PERMISSIONS, PACKAGE_FILE_PERMISSIONS);
        let leftovers = fs::read_dir(folder.path()).unwrap().count();
        assert_eq!(leftovers, 1, "only the release file is left");
    }

    /// UPDATE-04: the AppImage is replaced in one step, and a bad copy leaves it alone.
    #[test]
    fn appimage_replaces_itself() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("SubUTF8.AppImage");
        fs::write(&target, "old").unwrap();
        let new = folder.path().join("SubUTF8-1.2.0-x86_64.AppImage");
        fs::write(&new, "new").unwrap();
        let (size, sha256) =
            copy_and_hash(&mut File::open(&new).unwrap(), &mut io::sink(), &mut |_| {}).unwrap();
        let downloaded = Downloaded {
            path: new,
            size,
            sha256,
        };
        replace_appimage(&downloaded, &target).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        let mode = fs::metadata(&target).unwrap().permissions().mode() & EXECUTABLE_PERMISSIONS;
        assert_eq!(mode, EXECUTABLE_PERMISSIONS);
        let tampered = Downloaded {
            sha256: String::from("0"),
            ..downloaded
        };
        assert_eq!(
            replace_appimage(&tampered, &target),
            Err(UpdateProblem::ChecksumMismatch)
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert!(!folder.path().join("SubUTF8.AppImage.new").exists());
    }
}
