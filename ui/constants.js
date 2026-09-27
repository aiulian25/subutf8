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
};

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

export const DESTINATION_OPTIONS = [
  ["beside-originals", "Beside originals"],
  ["output-folder", "Output folder"],
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
  location: "l",
};
export const HIDDEN_NAME_PREFIX = ".";
export const IDLE_POLL_MILLISECONDS = 30000;
export const BUSY_POLL_MILLISECONDS = 300;
export const PLACEHOLDER_PATTERN = /\{(\w+)\}/g;
