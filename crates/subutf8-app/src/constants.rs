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
pub const BROWSE_ROUTE: &str = "/browse";
pub const ADD_ROUTE: &str = "/add";
pub const OPEN_ROUTE: &str = "/open";
pub const UPLOAD_ROUTE: &str = "/upload";
pub const PREVIEW_ROUTE: &str = "/files/{id}/preview";
pub const ENCODING_ROUTE: &str = "/files/{id}/encoding";
pub const REMOVE_ROUTE: &str = "/files/{id}/remove";
pub const CLEAR_ROUTE: &str = "/clear";
pub const SETTINGS_ROUTE: &str = "/settings";
pub const CONVERT_ROUTE: &str = "/convert";
pub const CANCEL_ROUTE: &str = "/cancel";
pub const QUIT_ROUTE: &str = "/quit";

/// ACCESS-03: the interface reads the token from this part of its address.
pub const TOKEN_FRAGMENT: &str = "#token=";
pub const LOOPBACK_HOST: &str = "127.0.0.1";

pub const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";
pub const CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";
pub const JAVASCRIPT_CONTENT_TYPE: &str = "text/javascript; charset=utf-8";
pub const SVG_CONTENT_TYPE: &str = "image/svg+xml";

/// The interface files, built into the program so every build serves the same ones.
pub const ASSETS: [(&str, &str, &[u8]); 8] = [
    (
        "/",
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
pub const START_FAILED_MESSAGE: &str = "SubUTF8 could not start:";
pub const NO_RANDOMNESS_MESSAGE: &str =
    "The system could not supply random numbers for the access token.";
