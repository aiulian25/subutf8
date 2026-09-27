use std::time::Duration;

use subutf8_core::constants::MAXIMUM_FILE_BYTES;

/// TARGET table: the Docker image sets `SUBUTF8_MODE=container`.
pub const MODE_VARIABLE: &str = "SUBUTF8_MODE";
pub const CONTAINER_MODE: &str = "container";
/// ACCESS-05: comma-separated host names Docker also answers to, such as a reverse proxy's.
pub const ALLOWED_HOSTS_VARIABLE: &str = "SUBUTF8_ALLOWED_HOSTS";
pub const ALLOWED_HOSTS_SEPARATOR: char = ',';
/// TARGET-02: the folders Docker browses, as in the owner's other container apps; more can
/// be listed in `SUBUTF8_BROWSE_ROOTS`, separated by colons.
pub const DEFAULT_BROWSE_ROOTS: [&str; 2] = ["/mnt", "/media"];
pub const BROWSE_ROOTS_VARIABLE: &str = "SUBUTF8_BROWSE_ROOTS";
pub const BROWSE_ROOTS_SEPARATOR: char = ':';
/// TARGET-04: Docker's persistent output folder, when it is mounted.
pub const OUTPUT_ROOT: &str = "/output";

/// SET-02: where the saved settings and the history live. Docker keeps them in `/data`, the
/// desktop in its settings folder; `SUBUTF8_DATA_DIR` moves them on either.
pub const DATA_FOLDER_VARIABLE: &str = "SUBUTF8_DATA_DIR";
pub const CONTAINER_DATA_FOLDER: &str = "/data";
pub const DATA_FOLDER_NAME: &str = "subutf8";
pub const DEFAULTS_FILE_NAME: &str = "settings.json";
pub const HISTORY_FILE_NAME: &str = "history.jsonl";

/// HIST-01: the oldest records go once there are more; the file is rewritten only after
/// `HISTORY_COMPACT_SLACK` more, not after every conversion.
pub const MAXIMUM_HISTORY_RECORDS: usize = 20_000;
pub const HISTORY_COMPACT_SLACK: usize = 2_000;
pub const MAXIMUM_SEARCH_RESULTS: usize = 100;
pub const MAXIMUM_QUERY_CHARACTERS: usize = 200;
/// HIST-01: local time, to the second, with the offset from UTC.
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%:z";

/// WATCH-01.
pub const WATCH_INTERVAL_VARIABLE: &str = "SUBUTF8_WATCH_INTERVAL";
pub const DEFAULT_WATCH_INTERVAL: Duration = Duration::from_secs(60);
pub const MINIMUM_WATCH_INTERVAL: Duration = Duration::from_secs(2);
pub const MAXIMUM_WATCH_FOLDERS: usize = 20;
pub const WATCH_LOG_ENTRIES: usize = 50;

/// UPDATE-01: `SUBUTF8_UPDATE_CHECK=0` turns every update check off.
pub const UPDATE_CHECK_VARIABLE: &str = "SUBUTF8_UPDATE_CHECK";
pub const UPDATE_CHECK_OFF: &str = "0";
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/aiulian25/subutf8/releases/latest";
pub const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const MINIMUM_MANUAL_CHECK_INTERVAL: Duration = Duration::from_secs(30);
pub const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(3);
pub const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const USER_AGENT: &str = concat!("subutf8/", env!("CARGO_PKG_VERSION"));
pub const GITHUB_ACCEPT: &str = "application/vnd.github+json";
/// UPDATE-03: downloads use HTTPS, and every address, redirects included, is on GitHub.
pub const HTTPS_SCHEME: &str = "https";
pub const ALLOWED_DOWNLOAD_DOMAINS: [&str; 2] = ["github.com", "githubusercontent.com"];
pub const MAXIMUM_REDIRECTS: usize = 5;
pub const MAXIMUM_RELEASE_BYTES: u64 = 1024 * 1024;
pub const MAXIMUM_CHECKSUMS_BYTES: u64 = 64 * 1024;
pub const MAXIMUM_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const DOWNLOAD_BUFFER_BYTES: usize = 64 * 1024;
pub const VERSION_TAG_PREFIX: char = 'v';
pub const VERSION_SEPARATOR: char = '.';
pub const CHECKSUMS_ASSET: &str = "SHA256SUMS";
pub const SHA256_DIGEST_PREFIX: &str = "sha256:";
/// `sha256sum --binary` writes this before the file name.
pub const CHECKSUM_BINARY_MARK: char = '*';
pub const EXECUTE_PERMISSION_BITS: u32 = 0o111;
/// The release files, as `scripts/build-release.sh` names them: a prefix and a suffix each.
pub const DEB_ASSET: (&str, &str) = ("subutf8_", "_amd64.deb");
pub const RPM_ASSET: (&str, &str) = ("subutf8-", ".x86_64.rpm");
pub const APPIMAGE_ASSET: (&str, &str) = ("SubUTF8-", "-x86_64.AppImage");
/// UPDATE-04: how the running copy was installed, and how it is updated.
pub const APPIMAGE_VARIABLE: &str = "APPIMAGE";
pub const INSTALLED_PROGRAM: &str = "/usr/bin/subutf8";
pub const PATH_VARIABLE: &str = "PATH";
pub const PRIVILEGE_PROGRAM: &str = "pkexec";
/// pkexec's answer when the person cancels or cannot authenticate.
pub const NOT_AUTHORIZED_EXIT_CODES: [i32; 2] = [126, 127];
pub const DPKG_PROGRAM: &str = "dpkg";
pub const DPKG_STATUS_FLAG: &str = "-s";
pub const RPM_PROGRAM: &str = "rpm";
pub const RPM_QUERY_FLAG: &str = "-q";
pub const DEB_INSTALL: (&str, &[&str]) = ("apt-get", &["install", "-y"]);
/// Tried in order; the first one present installs the `.rpm`.
pub const RPM_INSTALLERS: [(&str, &[&str]); 3] = [
    ("dnf", &["install", "-y"]),
    (
        "zypper",
        &["--non-interactive", "install", "--allow-unsigned-rpm"],
    ),
    ("rpm", &["-U"]),
];
pub const PACKAGE_FILE_PERMISSIONS: u32 = 0o644;
pub const EXECUTABLE_PERMISSIONS: u32 = 0o755;
pub const PARTIAL_DOWNLOAD_PREFIX: &str = ".subutf8-";
pub const PARTIAL_DOWNLOAD_SUFFIX: &str = ".part";
pub const REPLACEMENT_SUFFIX: &str = ".new";

/// UI-13: `ui/index.html` names the theme with this attribute; the server fills in the saved
/// one, so the page never flashes the wrong colours.
pub const THEME_PLACEHOLDER: &str = "data-theme=\"system\"";
pub const THEME_ATTRIBUTE: &str = "data-theme";

/// ENC-19: a warning shown once stands for this many of its kind before others join it.
pub const SINGLE_WARNING_COUNT: usize = 1;

/// ENC-20: files of a folder detected with certainty, all with one encoding, before a short
/// file of that folder is offered it.
pub const FOLDER_AGREEMENT_MINIMUM_FILES: usize = 2;

/// ACCESS-01.
pub const CONTAINER_PORT: u16 = 61880;
pub const DESKTOP_ADDRESS: [u8; 4] = [127, 0, 0, 1];
pub const CONTAINER_ADDRESS: [u8; 4] = [0, 0, 0, 0];
/// ACCESS-01: port 0 asks the system for a free port.
pub const ANY_FREE_PORT: u16 = 0;

/// ACCESS-02: 256 random bits, written as hexadecimal.
pub const TOKEN_BYTES: usize = 32;
/// ACCESS-04: every data request carries this header, holding the token on the desktop.
pub const TOKEN_HEADER: &str = "x-subutf8-token";
/// ACCESS-05.
pub const ALWAYS_ALLOWED_HOSTS: [&str; 2] = ["localhost", "127.0.0.1"];

/// TARGET table.
pub const HOME_VARIABLE: &str = "HOME";
pub const CONFIG_HOME_VARIABLE: &str = "XDG_CONFIG_HOME";
pub const CONFIG_FOLDER: &str = ".config";
pub const USER_FOLDERS_FILE: &str = "user-dirs.dirs";
pub const DOWNLOAD_FOLDER_KEY: &str = "XDG_DOWNLOAD_DIR=";
pub const HOME_PLACEHOLDER: &str = "$HOME";
pub const DOWNLOAD_FOLDER_NAME: &str = "Downloads";
pub const ROOT_FOLDER: &str = "/";

/// ACCESS-08.
pub const RUNTIME_FOLDER_VARIABLE: &str = "XDG_RUNTIME_DIR";
pub const RUNTIME_FOLDER_NAME: &str = "subutf8";
pub const INSTANCE_FILE_NAME: &str = "instance.json";
pub const PRIVATE_FOLDER_PERMISSIONS: u32 = 0o700;
pub const PRIVATE_FILE_PERMISSIONS: u32 = 0o600;
pub const HAND_OVER_TIMEOUT: Duration = Duration::from_secs(3);
/// Any 2xx answer from the running app means it took the files.
pub const SUCCESS_STATUS_PREFIX: &str = "HTTP/1.1 2";

/// ACCESS-09.
pub const IDLE_LIMIT: Duration = Duration::from_secs(60 * 60);
pub const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// The desktop builds show the interface in their own window when this program is present
/// next to `subutf8`, and in the default browser otherwise.
pub const WINDOW_PROGRAM: &str = "subutf8-window";
/// The window reads its address from the environment; `subutf8-window` uses the same name.
pub const WINDOW_URL_VARIABLE: &str = "SUBUTF8_WINDOW_URL";
pub const BROWSER_OPENER: &str = "xdg-open";
/// `subutf8-window` reads these lines from its standard input.
pub const RAISE_WINDOW_LINE: &str = "raise\n";
pub const WEBKIT_RENDERER_VARIABLE: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
pub const WEBKIT_RENDERER_SETTING: &str = "1";
/// The window's file picker is the desktop's own dialog, through its portal, where there is
/// one; GTK falls back to its built-in dialog otherwise.
pub const PORTAL_VARIABLE: &str = "GTK_USE_PORTAL";
pub const PORTAL_SETTING: &str = "1";

/// LIMIT-01, for one dropped file.
pub const MAXIMUM_UPLOAD_BYTES: usize = MAXIMUM_FILE_BYTES as usize;
/// LIMIT-02.
pub const MAXIMUM_DROPPED_BYTES: usize = 256 * 1024 * 1024;
/// Keeps one huge folder from producing a huge response in the Browse view.
pub const BROWSE_MAXIMUM_ENTRIES: usize = 5_000;

/// ACCESS-06.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
    style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; \
    object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
pub const NO_STORE: &str = "no-store";
pub const NO_SNIFF: &str = "nosniff";
pub const NO_REFERRER: &str = "no-referrer";
pub const DENY_FRAMING: &str = "DENY";

/// Routes; `ui/constants.js` names the same ones.
pub const API_PREFIX: &str = "/api";
pub const STATE_ROUTE: &str = "/state";
pub const ENCODINGS_ROUTE: &str = "/encodings";
pub const LANGUAGES_ROUTE: &str = "/languages";
pub const BROWSE_ROUTE: &str = "/browse";
pub const ADD_ROUTE: &str = "/add";
pub const OPEN_ROUTE: &str = "/open";
pub const UPLOAD_ROUTE: &str = "/upload";
pub const PREVIEW_ROUTE: &str = "/files/{id}/preview";
pub const CANDIDATES_ROUTE: &str = "/files/{id}/candidates";
pub const ENCODING_ROUTE: &str = "/files/{id}/encoding";
pub const LANGUAGE_ROUTE: &str = "/files/{id}/language";
pub const REPAIR_ROUTE: &str = "/files/{id}/repair";
pub const OUTPUT_ROUTE: &str = "/files/{id}/output";

/// UI-17: a converted file is sent as a SubRip attachment, named as RFC 8187 allows any name.
pub const SUBRIP_CONTENT_TYPE: &str = "application/x-subrip; charset=utf-8";
pub const ATTACHMENT_DISPOSITION_PREFIX: &str = "attachment; filename*=UTF-8''";
/// RFC 3986: besides letters and digits, the characters a name keeps without percent-encoding.
pub const UNRESERVED_NAME_PUNCTUATION: &[u8] = b"-._~";

/// NAME-11: a path that is not UTF-8 travels as its bytes in hexadecimal.
pub const HEX_RADIX: u32 = 16;
pub const HEX_DIGITS_PER_BYTE: usize = 2;
pub const INVALID_HEX_MESSAGE: &str = "a path's hexadecimal does not read";
pub const REMOVE_ROUTE: &str = "/files/{id}/remove";
pub const CLEAR_ROUTE: &str = "/clear";
pub const CONVERT_ROUTE: &str = "/convert";
pub const CANCEL_ROUTE: &str = "/cancel";
pub const QUIT_ROUTE: &str = "/quit";
pub const DEFAULTS_ROUTE: &str = "/defaults";
pub const RESTORE_DEFAULTS_ROUTE: &str = "/defaults/restore";
pub const HISTORY_ROUTE: &str = "/history";
pub const WATCH_LOG_ROUTE: &str = "/watch-log";
pub const UPDATE_CHECK_ROUTE: &str = "/update/check";
pub const UPDATE_DOWNLOAD_ROUTE: &str = "/update/download";
pub const UPDATE_INSTALL_ROUTE: &str = "/update/install";
pub const UPDATE_RESTART_ROUTE: &str = "/update/restart";

/// ACCESS-03: the interface reads the token from this part of its address.
pub const TOKEN_FRAGMENT: &str = "#token=";
pub const LOOPBACK_HOST: &str = "127.0.0.1";

pub const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";
pub const CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";
pub const JAVASCRIPT_CONTENT_TYPE: &str = "text/javascript; charset=utf-8";
pub const SVG_CONTENT_TYPE: &str = "image/svg+xml";

/// The page itself; the other interface files are loaded from it.
pub const INDEX_PATH: &str = "/";

/// The interface files, built into the program so every build serves the same ones.
pub const ASSETS: [(&str, &str, &[u8]); 11] = [
    (
        INDEX_PATH,
        HTML_CONTENT_TYPE,
        include_bytes!("../../../ui/index.html"),
    ),
    (
        "/selection.js",
        JAVASCRIPT_CONTENT_TYPE,
        include_bytes!("../../../ui/selection.js"),
    ),
    (
        "/styles.css",
        CSS_CONTENT_TYPE,
        include_bytes!("../../../ui/styles.css"),
    ),
    (
        "/constants.js",
        JAVASCRIPT_CONTENT_TYPE,
        include_bytes!("../../../ui/constants.js"),
    ),
    (
        "/api.js",
        JAVASCRIPT_CONTENT_TYPE,
        include_bytes!("../../../ui/api.js"),
    ),
    (
        "/view.js",
        JAVASCRIPT_CONTENT_TYPE,
        include_bytes!("../../../ui/view.js"),
    ),
    (
        "/main.js",
        JAVASCRIPT_CONTENT_TYPE,
        include_bytes!("../../../ui/main.js"),
    ),
    (
        "/settings.js",
        JAVASCRIPT_CONTENT_TYPE,
        include_bytes!("../../../ui/settings.js"),
    ),
    (
        "/search.js",
        JAVASCRIPT_CONTENT_TYPE,
        include_bytes!("../../../ui/search.js"),
    ),
    (
        "/dialogs.js",
        JAVASCRIPT_CONTENT_TYPE,
        include_bytes!("../../../ui/dialogs.js"),
    ),
    (
        "/icon.svg",
        SVG_CONTENT_TYPE,
        include_bytes!("../../../packaging/icon/subutf8.svg"),
    ),
];

pub const VERSION_FLAG: &str = "--version";
pub const PROGRAM_NAME: &str = "subutf8";
pub const RUNNING_MESSAGE: &str = "SubUTF8 is running. Open this address in a browser:";
pub const CONTAINER_RUNNING_MESSAGE: &str = "SubUTF8 is running. Open http://localhost:61880 on this \
    server, or http://<this server's address>:61880 from another machine.";
pub const CONTAINER_FOLDERS_MESSAGE: &str = "Folders it can browse:";
pub const NO_FOLDERS_MESSAGE: &str = "No folders are mounted. Mount your subtitle folders under \
    /media or /mnt, as docker-compose.yml shows, and start the container again.";
pub const DATA_FOLDER_READ_ONLY_MESSAGE: &str = "Settings and the list of converted files \
    cannot be saved. Mount a folder at /data, as docker-compose.yml shows.";
pub const OUTPUT_FOLDER_READ_ONLY_MESSAGE: &str = "The folder mounted at /output cannot be \
    written to. Give it to the user and group in PUID and PGID, for example with \
    sudo chown 1000:1000 on the server's folder.";
pub const START_FAILED_MESSAGE: &str = "SubUTF8 could not start:";
pub const NO_RANDOMNESS_MESSAGE: &str =
    "The system could not supply random numbers for the access token.";
