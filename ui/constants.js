// Names shared with the server: crates/subutf8-app/src/constants.rs and views.rs.
export const TOKEN_HEADER = "x-subutf8-token";
export const TOKEN_STORAGE_KEY = "subutf8-token";
export const TOKEN_PATTERN = /[#&]token=([^&]+)/;
export const CONTENT_TYPE_HEADER = "content-type";
export const JSON_CONTENT_TYPE = "application/json";
export const BINARY_CONTENT_TYPE = "application/octet-stream";
export const UPLOAD_NAME_PARAMETER = "name";

export const ROUTES = {
  state: "/api/state",
  encodings: "/api/encodings",
  browse: "/api/browse",
  add: "/api/add",
  upload: "/api/upload",
  files: "/api/files",
  clear: "/api/clear",
  settings: "/api/settings",
  convert: "/api/convert",
  cancel: "/api/cancel",
  quit: "/api/quit",
  defaults: "/api/defaults",
  restoreDefaults: "/api/defaults/restore",
  history: "/api/history",
  watchLog: "/api/watch-log",
  updateCheck: "/api/update/check",
  updateDownload: "/api/update/download",
  updateInstall: "/api/update/install",
  updateRestart: "/api/update/restart",
};

export const FILE_ACTIONS = {
  preview: "preview",
  encoding: "encoding",
  remove: "remove",
};

export const HTTP_METHODS = { get: "GET", post: "POST" };
export const PAYLOAD_TOO_LARGE_STATUS = 413;
export const UNAUTHORIZED_STATUS = 401;
export const NETWORK_FAILURE_STATUS = 0;

export const MODES = { desktop: "desktop", container: "container" };
export const BROWSE_MODES = { files: "files", folder: "folder" };
export const PATH_SEPARATOR = "/";
export const SHORT_PATH_PARTS = 2;

// Reasons the page itself acts on; the full list is REASON_MESSAGES below.
export const REASONS = {
  tooLarge: "too-large",
  tooMuchDropped: "too-much-dropped",
  listFull: "list-full",
  internal: "internal",
  dataFolderReadOnly: "data-folder-read-only",
  noPrivilegeProgram: "no-privilege-program",
  updateCheckOff: "update-check-off",
};

export const DESTINATIONS = { besideOriginals: "beside-originals", outputFolder: "output-folder" };

// UI-13: "system" follows the desktop's light or dark setting.
export const THEMES = { system: "system", light: "light", dark: "dark" };
export const THEME_OPTIONS = [
  [THEMES.system, "Match the system"],
  [THEMES.light, "Light"],
  [THEMES.dark, "Dark"],
];

// UPDATE-01 to UPDATE-05; crates/subutf8-app/src/views.rs names the same ones.
export const PACKAGES = {
  docker: "docker",
  deb: "deb",
  rpm: "rpm",
  appimage: "appimage",
  other: "other",
};
export const UPDATE_STEPS = {
  idle: "idle",
  downloading: "downloading",
  installing: "installing",
  installed: "installed",
  failed: "failed",
};
export const PERCENT = 100;

export const STATUSES = {
  ready: "ready",
  needsReview: "needs-review",
  converted: "converted",
  skipped: "skipped",
  failed: "failed",
};

// UI-02: every status is shown in words.
export const STATUS_LABELS = {
  [STATUSES.ready]: "Ready",
  [STATUSES.needsReview]: "Needs review",
  [STATUSES.converted]: "Converted",
  [STATUSES.skipped]: "Skipped",
  [STATUSES.failed]: "Failed",
};

// UI-07: what happened and, where there is something to do, what to do next.
// {offset} is a byte offset and {system} the system's own words.
export const REASON_MESSAGES = {
  "too-little-evidence":
    "Too little accented text to detect the encoding. Check the preview, then choose the encoding.",
  "guess-does-not-decode": "The encoding could not be detected. Choose it by hand.",
  "language-hint-disagrees":
    "The subtitle language points to another encoding. Check the preview, then choose the encoding.",
  "looks-like-utf16":
    "Looks like UTF-16 without a byte-order mark. Check the preview, then confirm the encoding.",
  "damaged-or-mixed-utf8":
    "Mostly UTF-8 but with invalid bytes, so it is damaged or mixed. Choose an encoding only if the preview reads correctly.",
  empty: "The file is empty.",
  "not-text": "This is not a text file.",
  "damaged-utf8": "Damaged UTF-8 at byte {offset}.",
  "damaged-utf16": "Damaged UTF-16 at byte {offset}.",
  "does-not-decode":
    "This encoding does not fit the file (byte {offset}). Choose another encoding.",
  "round-trip-differs":
    "Converting back does not give the original bytes (byte {offset}). Choose another encoding.",
  unreadable: "The file could not be read: {system}.",
  "too-large": "The file is larger than 16 MiB.",
  "not-regular-file": "Not a regular file.",
  "symbolic-link": "Symbolic links are not followed.",
  "not-srt": "Not an .srt file.",
  "outside-allowed-area": "Outside the folders SubUTF8 may use.",
  "not-a-folder": "Not a folder.",
  "name-not-utf8": "The name contains characters SubUTF8 cannot handle.",
  "already-exists":
    "The output file already exists. Choose Rename or Overwrite to convert it anyway.",
  "name-taken-in-list": "A file higher in the list gets the same output name.",
  "listed-file": "The output would replace a file in the list, so it was left alone.",
  "read-only-destination": "The existing output file is read-only.",
  "folder-not-writable": "Folder is read-only: choose an output folder.",
  "no-free-name": "No free output name was found.",
  "name-too-long": "The output name would be too long.",
  "verification-failed": "The written file did not verify, so no file was written.",
  "write-failed": "Writing failed: {system}. No file was written.",
  cancelled: "Cancelled before this file.",
  "unknown-file": "The file is no longer in the list.",
  "conversion-running": "Wait for the conversion to finish.",
  "too-much-dropped": "Too many dropped files are held in memory. Convert or remove files first.",
  "list-full": "The list is full. Convert or remove files first.",
  "not-offered": "That encoding is not offered.",
  "not-applicable": "The encoding of this file cannot be changed.",
  "invalid-language": "Use a language tag such as ro, en or pt-BR, or leave it empty.",
  "unusable-dropped-name": "The file name cannot be used.",
  "output-folder-unavailable": "The output folder is not available. Choose another one.",
  "not-available": "Not available here.",
  internal: "Something went wrong. Try again.",
  "converted-copy": "SubUTF8's converted copy of {name}, so it is left alone.",
  "waiting-in-list": "Added to the list, where it waits for you to check it.",
  "settings-damaged":
    "The saved settings could not be read, so the defaults are shown. Save replaces them.",
  "settings-unreadable": "The saved settings could not be read: {system}.",
  "settings-not-saved": "Settings could not be saved: {system}.",
  "data-folder-read-only": "Settings and the list of converted files cannot be kept.",
  "history-not-saved": "The list of converted files could not be saved: {system}.",
  "too-many-watch-folders": "Watch at most 20 folders.",
  "watch-folder-holds-output-folder":
    "{name} holds the output folder, so SubUTF8 would convert its own outputs. Choose another folder, or another output folder.",
  "update-check-off": "Update checks are turned off (SUBUTF8_UPDATE_CHECK=0).",
  "update-check-failed": "GitHub could not be reached to check for updates.",
  "no-update": "There is no newer version.",
  "update-busy": "An update is already under way.",
  "update-not-downloaded": "Download the update first.",
  "update-not-installed": "Install the update first.",
  "no-release-file": "This release has no file for this kind of install. Get it from GitHub.",
  "download-failed": "The download failed. Try again.",
  "not-github": "The download led away from GitHub, so it was stopped.",
  "checksum-mismatch": "The download did not match its checksum, so it was not kept.",
  "no-checksum": "The release lists no checksum, so the download was not trusted.",
  "no-privilege-program": "This system has no pkexec to install with.",
  "not-authorized": "The update was not installed: the password was not given.",
  "install-failed": "The package manager could not install the update.",
  "replace-failed": "The AppImage could not be replaced.",
};

export const WARNING_MESSAGES = {
  "round-trip-differs":
    "Some characters have two byte forms in this encoding (first at byte {offset}); the text is unaffected.",
  "contains-replacement-characters": "The original already contained replacement characters (�).",
  "no-cues": "No subtitle timings were found.",
  "missing-cue-number": "Line {line}: a subtitle without a number.",
  "cue-number-out-of-order": "Line {line}: a subtitle number out of order.",
  "malformed-timing": "Line {line}: an unusual timing line.",
};

export const TEXT = {
  noAccess: "This page cannot reach SubUTF8. Start SubUTF8 again from the app menu.",
  stopped: "SubUTF8 has stopped. You can close this window.",
  noMounts:
    "No folders are mounted. Mount your subtitle folders under /media or /mnt, as docker-compose.yml shows, then start the container again.",
  selectFile: "Select a file to see its text.",
  nothingToPreview: "Nothing to preview.",
  clipped: "(cut at 400 characters)",
  writtenTo: "Written to {path}",
  encodingLabel: "Encoding",
  useEncoding: "Use this encoding",
  uploading: "Adding dropped files: {current} of {total}",
  converting: "Converting {finished} of {total}",
  notSrtDropped: "Not .srt files, so skipped: {count}.",
  dropUnreadable: "The dropped files could not be read. Try again, or use Browse.",
  addSkipped: "{count} could not be added. First: {path}: {reason}",
  limitReached: "The list is full, so some files were not added.",
  nothingAdded: "Nothing new was added.",
  summaryParts: {
    ready: "{count} ready",
    converted: "{count} converted",
    review: "{count} need review",
    skipped: "{count} skipped",
    failed: "{count} failed",
  },
  summarySeparator: ", ",
  noEncoding: "—",
  noOutput: "—",
  removeLabel: "Remove {name}",
  removeSymbol: "×",
  browseFilesTitle: "Add files",
  browseFolderTitle: "Choose output folder",
  addSelected: "Add selected",
  addSelectedCount: "Add {count} selected",
  selectedCount: "{count} selected, in any folder",
  nothingSelected: "Nothing selected",
  crumbRoot: "/",
  crumbSeparator: "›",
  folderLabel: "{name}, folder",
  dropHintBrowser: "Dropped files are written to the output folder.",
  dropHintWindow: "Dropped files and folders are converted where they are.",
  encodingForSelected: "Encoding for the {count} selected files",
  encodingRefused: "{count} selected files kept their encoding. First: {name}: {reason}",
  useThisFolder: "Use this folder",
  emptyFolder: "No folders or .srt files here.",
  truncatedFolder: "Only the first 5,000 entries are shown.",
  homeShortcut: "Home",
  pathEllipsis: "…/",
};

export const SETTINGS_TEXT = {
  restored: "The defaults are back.",
  dockerDataHint: "Mount a folder at /data, as docker-compose.yml shows.",
  watchExplanation:
    "New subtitles in these folders are converted with the defaults above, once they have stopped changing. Existing files are never replaced.",
  watchEmpty: "No folders are watched.",
  watchInterval: "Looks every {seconds} seconds.",
  watchUnavailable: "Not available now: {folders}.",
  watchOutputUnavailable: "The output folder is not available, so watched files wait.",
  watchNoActivity: "Nothing yet.",
  removeWatchFolder: "Stop watching {path}",
  listSeparator: ", ",
};

export const UPDATE_TEXT = {
  available: "SubUTF8 {latest} is available. You have {current}.",
  upToDate: "SubUTF8 {current} is up to date.",
  notChecked: "You have SubUTF8 {current}.",
  checkFailed: "GitHub could not be reached. You have SubUTF8 {current}.",
  checkNow: "Check for updates",
  download: "Download update",
  downloading: "Downloading: {percent}%",
  downloaded: "Downloaded and checked: {path}",
  install: "Install update",
  replaceAppImage: "Replace the AppImage",
  installing: "Installing. Confirm with your password in the system's dialog.",
  replacing: "Replacing the AppImage.",
  installed: "The update is installed.",
  restart: "Restart SubUTF8",
  docker: "Update the container on the server with:",
  dockerCommand: "docker compose pull && docker compose up -d",
  getFromGithub: "Download it from the release page.",
  releaseNotes: "What's new",
  installYourselfDeb: "Install it yourself with: sudo apt install {path}",
  installYourselfRpm: "Install it yourself with: sudo dnf install {path}",
  banner: "SubUTF8 {latest} is available.",
  bannerOpen: "Update…",
  bannerOpenDocker: "How to update",
  bannerDismiss: "Hide this message",
  bannerDismissSymbol: "×",
};

export const SEARCH_TEXT = {
  settingsGroup: "Settings",
  filesGroup: "In the list",
  historyGroup: "Converted before",
  noResults: "Nothing found.",
  searching: "Searching…",
  watched: "watched folder",
  historyDetail: "{time} · {encoding} · {output}",
  fileDetail: "{status} · {folder}",
};

// HIST-02: words the page turns into dates before searching, by how many days ago they are.
export const DATE_WORDS = { today: 0, yesterday: 1 };
export const WORD_SEPARATOR = /\s+/;
export const DATE_PART_LENGTH = 2;
export const YEAR_LENGTH = 4;
export const MONTH_OFFSET = 1;
export const DATE_SEPARATOR = "-";
// "2026-09-27T14:03:11+03:00" is shown as "2026-09-27 14:03".
export const SHOWN_TIME_LENGTH = 16;
export const TIME_SEPARATOR = "T";
export const SHOWN_TIME_SEPARATOR = " ";
export const SEARCH_DELAY_MILLISECONDS = 200;
export const SEARCH_KEY = "k";

// What search finds among the settings: the label shown, extra words that find it, and the
// control in the Settings dialog it opens. Watch folders are Docker's alone.
export const SETTING_ENTRIES = [
  {
    label: "Subtitle language",
    words: "language tag ro en detection name",
    target: "default-language",
  },
  {
    label: "Write to",
    words: "destination beside originals output folder where",
    target: "default-destination",
  },
  {
    label: "Output folder",
    words: "output folder where save",
    target: "default-output-folder-button",
  },
  {
    label: "A folder for each day",
    words: "day date daily folder organise organize",
    target: "default-by-day",
  },
  {
    label: "If the output exists",
    words: "collision skip rename overwrite exists replace",
    target: "default-collision",
  },
  { label: "Theme", words: "theme light dark appearance colour color", target: "default-theme" },
  {
    label: "Watch folders",
    words: "watch automatic auto convert incoming folder",
    target: "watch-add-button",
    containerOnly: true,
  },
  {
    label: "Check for updates",
    words: "update version upgrade release new",
    target: "default-update-check",
  },
  { label: "Restore defaults", words: "reset restore defaults", target: "settings-restore" },
];

export const DESTINATION_OPTIONS = [
  [DESTINATIONS.besideOriginals, "Beside originals"],
  [DESTINATIONS.outputFolder, "Output folder"],
];

export const COLLISION_OPTIONS = [
  ["skip", "Skip"],
  ["rename", "Rename"],
  ["overwrite", "Overwrite"],
];

// ENC-13: Romanian first, then common subtitle languages.
export const LANGUAGE_SUGGESTIONS = [
  "ro",
  "en",
  "fr",
  "de",
  "es",
  "it",
  "pt",
  "pt-BR",
  "nl",
  "hu",
  "pl",
  "cs",
  "sk",
  "sl",
  "hr",
  "sr",
  "bs",
  "bg",
  "mk",
  "ru",
  "uk",
  "el",
  "tr",
  "ar",
  "he",
  "fa",
  "ja",
  "ko",
  "zh",
  "zh-TW",
  "th",
  "vi",
];

export const SRT_EXTENSION = ".srt";

// The desktop window opens the system's file picker for these requests, and hands over what
// was chosen or dropped onto it through the page's PICKED_CALLBACK; crates/subutf8-window uses
// the same names.
export const NATIVE_PICKS = {
  files: "files",
  folders: "folders",
  outputFolder: "output-folder",
  dropped: "dropped",
};
export const PICKED_CALLBACK = "subutf8Picked";

export const KEYS = {
  enter: "Enter",
  delete: "Delete",
  backspace: "Backspace",
  arrowUp: "ArrowUp",
  arrowDown: "ArrowDown",
  escape: "Escape",
  location: "l",
};
export const HIDDEN_NAME_PREFIX = ".";
export const IDLE_POLL_MILLISECONDS = 30000;
export const BUSY_POLL_MILLISECONDS = 300;
export const PLACEHOLDER_PATTERN = /\{(\w+)\}/g;
