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
  languages: "/api/languages",
  browse: "/api/browse",
  add: "/api/add",
  upload: "/api/upload",
  files: "/api/files",
  clear: "/api/clear",
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
  candidates: "candidates",
  encoding: "encoding",
  language: "language",
  repair: "repair",
  output: "output",
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
// NAME-11: a Browse row for a file whose name is not UTF-8 is known by its path's bytes.
export const BROWSE_RAW_KEY_PREFIX = "hex:";

// Reasons the page itself acts on; the full list is REASON_MESSAGES below.
export const REASONS = {
  tooLarge: "too-large",
  tooMuchDropped: "too-much-dropped",
  listFull: "list-full",
  internal: "internal",
  dataFolderReadOnly: "data-folder-read-only",
  noPrivilegeProgram: "no-privilege-program",
  updateCheckOff: "update-check-off",
  folderAgrees: "folder-agrees",
  folderNotWritable: "folder-not-writable",
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
// {line} is a line number, {system} the system's own words, {name} a file or folder,
// {count} and {encoding} what a folder agrees on (ENC-20), and {encoding} and {misreadAs} what
// garbled text was and how it was read (ENC-22).
export const REASON_MESSAGES = {
  "too-little-evidence":
    "Too little accented text to detect the encoding. Check the preview, then choose the encoding.",
  "folder-agrees":
    "Too little accented text on its own; {count} other files in this folder are {encoding}. Use it if the preview reads correctly.",
  "guess-does-not-decode": "The encoding could not be detected. Choose it by hand.",
  "language-hint-disagrees":
    "The subtitle language points to another encoding. Check the preview, then choose the encoding.",
  "looks-like-utf16":
    "Looks like UTF-16 without a byte-order mark. Check the preview, then confirm the encoding.",
  "damaged-or-mixed-utf8":
    "Part UTF-8, part another encoding. Choose the other encoding; lines that are already UTF-8 are kept.",
  "looks-garbled":
    "Looks garbled: {encoding} text that was read as {misreadAs} and saved as UTF-8. Compare the two lines, then choose.",
  "control-characters-in-utf8":
    "This UTF-8 file contains control characters that subtitles never use (line {line}), so it cannot be converted.",
  empty: "The file is empty.",
  "not-text": "This is not a text file.",
  "damaged-utf8": "Damaged UTF-8 on line {line}.",
  "damaged-utf16": "Damaged UTF-16 on line {line}.",
  "does-not-decode": "This encoding does not fit line {line}. Choose another encoding.",
  "round-trip-differs":
    "Converting back does not give the original bytes on line {line}. Choose another encoding.",
  unreadable: "The file could not be read: {system}.",
  "too-large": "The file is larger than 16 MiB.",
  "not-regular-file": "Not a regular file.",
  "symbolic-link": "Symbolic links are not followed.",
  "not-srt": "Not an .srt file.",
  "outside-allowed-area": "Outside the folders SubUTF8 may use.",
  "not-a-folder": "Not a folder.",
  "invalid-path": "That path cannot be used.",
  "name-not-decodable":
    "The file name is in an encoding that does not match the file's, so it was left alone.",
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
  "not-converted": "The file has not been converted yet.",
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
    "Some characters have two byte forms in this encoding (first on line {line}); the text is unaffected.",
  "contains-replacement-characters": "The original already contained replacement characters (�).",
  "no-cues": "No subtitle timings were found.",
  "missing-cue-number": "Line {line}{more}: a subtitle without a number.",
  "cue-number-out-of-order": "Line {line}{more}: a subtitle number out of order.",
  "malformed-timing": "Line {line}{more}: an unusual timing line.",
  "timing-uses-dot":
    "Line {line}{more}: timings use '.' before the milliseconds; most players accept it.",
};

export const TEXT = {
  noAccess: "This page cannot reach SubUTF8. Start SubUTF8 again from the app menu.",
  stopped: "SubUTF8 has stopped. You can close this window.",
  noMounts:
    "No folders are mounted. Mount your subtitle folders under /media or /mnt, as docker-compose.yml shows, then start the container again.",
  selectFile: "Select a file to see its text.",
  nothingToPreview: "Nothing to preview.",
  clipped: "(cut at 400 characters)",
  brokenLine: "Line {line}, as far as it can be read:",
  moreLines: " and {count} more",
  suggestionsTitle: "Suggestions — pick the one that reads correctly:",
  suggestionLabel: "{encoding} — {sample}",
  writtenTo: "Written to {path}",
  encodingLabel: "Encoding",
  useEncoding: "Use this encoding",
  keepUtf8Lines: "Keep lines that are already UTF-8",
  useForAllAgreeing: "Use {encoding} for the {count} files like this",
  repairAsIs: "As it is: {sample}",
  repairRepaired: "Repaired: {sample}",
  repair: "Repair",
  keepAsIs: "Keep as it is",
  repairSelected: "Repair the {count} selected files",
  keepSelected: "Keep the {count} selected files as they are",
  repairRefused: "{count} selected files were left as they were. First: {name}: {reason}",
  uploading: "Adding dropped files: {current} of {total}",
  converting: "Converting {finished} of {total}",
  notSrtDropped: "Not .srt files, so skipped: {count}.",
  dropUnreadable: "The dropped files could not be read. Try again, or use Browse.",
  addSkipped: "{count} could not be added. First: {path}: {reason}",
  limitReached: "The list is full, so some files were not added.",
  download: "Download",
  helpsDetection: "helps detection",
  nameOnly: "name only",
  downloadAll: "Download all converted ({count})",
  writeSkippedToOutputFolder: "Write the {count} skipped files to {folder}",
  writeSkippedFileToOutputFolder: "Write the skipped file to {folder}",
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
  languageRefused: "{count} selected files kept their language. First: {name}: {reason}",
  fileLanguage: "Subtitle language for this file",
  fileLanguageForSelected: "Subtitle language for the {count} selected files",
  languageFromSettings: "{language}, as in Settings",
  noLanguage: "none",
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
    "New subtitles in these folders are converted with the settings above, once they have stopped changing. Existing files are never replaced.",
  watchEmpty: "No folders are watched.",
  watchInterval: "Looks every {seconds} seconds.",
  watchUnavailable: "Not available now: {folders}.",
  watchOutputUnavailable: "The output folder is not available, so watched files wait.",
  watchNoActivity: "Nothing yet.",
  removeWatchFolder: "Stop watching {path}",
  listSeparator: ", ",
};

// The desktop window opens only SubUTF8's own GitHub pages, in the system's browser.
export const RELEASES_PAGE = "https://github.com/aiulian25/subutf8/releases";

// UPDATE-01 to UPDATE-05: the Settings card, as CineSort shows it.
export const UPDATE_TEXT = {
  upToDate: "✓ Up to date",
  couldNotCheck: "Couldn't check",
  notChecked: "Not checked yet",
  newChip: "New",
  version: "SubUTF8 v{current}",
  automaticCheck: "automatic check once per day",
  automaticCheckOff: "automatic check off",
  checkNow: "Check for updates",
  checking: "Checking…",
  checkedLatest: "Checked just now — you're on the latest version.",
  checkFailed: "Couldn't reach GitHub — check your connection and try again.",
  releases: "Releases on GitHub",
  available: "SubUTF8 v{latest} is available",
  currentVersion: "You're on v{current} · ",
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
  readyTitle: "SubUTF8 v{latest} is ready to install",
  readyChecked: "Downloaded, and its SHA-256 matches the GitHub release.",
  readyPackage:
    "Installing asks for your password in the system's own dialog; SubUTF8 itself never runs as root.",
  readyAppImage: "The AppImage is replaced in place, so no password is needed.",
  untouched: "Your files, history and settings are untouched.",
  later: "Later",
  installingTitle: "Installing SubUTF8 v{latest}…",
  hide: "Hide",
  installedTitle: "SubUTF8 v{latest} is installed",
  restartBody: "Restart SubUTF8 to start using it.",
  restartLater: "Restart later",
  manualTitle: "Install SubUTF8 v{latest} yourself",
  close: "Close",
  banner: "SubUTF8 v{latest} is available.",
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
    target: "update-card",
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

// ENC-23: Romanian subtitles, `ro` with or without a region, can take the comma letters that
// windows-1250 and ISO-8859-2 lack.
export const ROMANIAN_LANGUAGE = "ro";
export const LANGUAGE_SUBTAG_SEPARATOR = "-";
export const ROMANIAN_LETTERS = { asDecoded: "as-decoded", comma: "comma" };
export const ROMANIAN_LETTER_OPTIONS = [
  [ROMANIAN_LETTERS.asDecoded, "ş ţ (as decoded)"],
  [ROMANIAN_LETTERS.comma, "ș ț (correct)"],
];

// UI-19: common subtitle languages that only name outputs; the ones that help detection
// (ENC-13) come from the app.
export const NAME_ONLY_LANGUAGES = ["en", "fr", "de", "es", "it", "pt", "pt-BR", "nl"];

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
// UI-17: how long a downloaded file's temporary address lives; browsers start saving it within
// this time, and some do only after the click that asked for it has finished.
export const DOWNLOAD_ADDRESS_LIFETIME_MILLISECONDS = 60000;
export const PLACEHOLDER_PATTERN = /\{(\w+)\}/g;
