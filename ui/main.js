import { ApiError, api, connect } from "./api.js";
import {
  BROWSE_MODES,
  BROWSE_RAW_KEY_PREFIX,
  BUSY_POLL_MILLISECONDS,
  DESTINATIONS,
  HIDDEN_NAME_PREFIX,
  IDLE_POLL_MILLISECONDS,
  KEYS,
  MODES,
  NATIVE_PICKS,
  NETWORK_FAILURE_STATUS,
  PAYLOAD_TOO_LARGE_STATUS,
  REASONS,
  SEARCH_KEY,
  SRT_EXTENSION,
  PICKED_CALLBACK,
  STATUSES,
  TEXT,
  UNAUTHORIZED_STATUS,
  UPDATE_STEPS,
} from "./constants.js";
import {
  applyTheme,
  dialogElements,
  renderUpdateBanner,
  setUpDialogs,
  setUpLanguageList,
} from "./dialogs.js";
import { openSearch, setUpSearch } from "./search.js";
import { Selection } from "./selection.js";
import {
  isSettingsOpen,
  openSettings,
  refreshSettings,
  refreshUpdatePrompt,
  setUpSettingsDialog,
} from "./settings.js";
import {
  disableControls,
  elements,
  fileName,
  fill,
  focusBrowseEntry,
  focusFileRow,
  joinPath,
  problemText,
  renderBrowse,
  renderBrowseSelection,
  renderEmptyPreview,
  renderFiles,
  renderMode,
  renderPreview,
  renderProgress,
  renderWriteSkipped,
  saveFile,
  setDragging,
  showBrowsePath,
  showListMessage,
  showNotice,
} from "./view.js";

const model = {
  app: null,
  encodings: [],
  selection: new Selection(),
  previewKey: null,
  preview: null,
  candidatesKey: null,
  candidates: [],
  // What the last encoding or language choice in the preview could not do.
  choiceMessage: "",
  pollTimer: null,
  isStopped: false,
  isBannerDismissed: false,
  // What to do with the folder the desktop window's picker returns next.
  pendingFolderPick: null,
  browse: newBrowse(BROWSE_MODES.files, null),
};

function newBrowse(mode, onFolderChosen) {
  return {
    mode,
    onFolderChosen,
    listing: null,
    folders: new Set(),
    selection: new Selection(),
    filter: "",
    message: "",
    navigation: 0,
    isTypingPath: false,
  };
}

// The desktop window lets the page open the system's own file picker.
function hasNativePicker() {
  return typeof window.ipc?.postMessage === "function";
}

// UI-17: the desktop window cannot save downloads; a browser, beside the window or instead of
// it, can.
function canDownload() {
  return !hasNativePicker();
}

function isBusy() {
  return model.app?.conversion != null;
}

// UPDATE-03: a download or an install is followed closely, like a conversion.
function isUpdating() {
  return [UPDATE_STEPS.downloading, UPDATE_STEPS.installing].includes(model.app?.update.step);
}

function fileIds() {
  return model.app?.files.map((file) => file.id) ?? [];
}

function focusedFile() {
  return model.app?.files.find((file) => file.id === model.selection.focus) ?? null;
}

// A choice applies to every selected file it fits, or else to the file shown.
function choiceTargets(fits) {
  const selected = (model.app?.files ?? []).filter(
    (file) => model.selection.has(file.id) && fits(file),
  );
  const focused = focusedFile();
  if (selected.length > 0 || !focused || !fits(focused)) {
    return selected;
  }
  return [focused];
}

// UI-16: a language fits every file.
function languageTargets() {
  return choiceTargets(() => true);
}

function encodingTargets() {
  return choiceTargets((file) => file.canChooseEncoding);
}

// ENC-22.
function repairTargets() {
  return choiceTargets((file) => file.canRepair);
}

function errorMessage(error) {
  if (error instanceof ApiError && error.problem) {
    return problemText(error.problem);
  }
  if (error instanceof ApiError && error.status === PAYLOAD_TOO_LARGE_STATUS) {
    return problemText({ reason: REASONS.tooLarge });
  }
  return problemText({ reason: REASONS.internal });
}

// The app stops on Quit, when idle or when its window closes; an old tab then says so.
function handleFailure(error) {
  const isGone = error instanceof ApiError && error.status === NETWORK_FAILURE_STATUS;
  const isRefused = error instanceof ApiError && error.status === UNAUTHORIZED_STATUS;
  if (!isGone && !isRefused) {
    return false;
  }
  model.isStopped = true;
  clearTimeout(model.pollTimer);
  disableControls();
  showNotice(isGone ? TEXT.stopped : TEXT.noAccess);
  return true;
}

async function refresh() {
  clearTimeout(model.pollTimer);
  if (model.isStopped) {
    return;
  }
  try {
    model.app = await api.state();
    render();
    await refreshPreview();
  } catch (error) {
    if (handleFailure(error)) {
      return;
    }
  }
  // ACCESS-09: an open tab checks in regularly; a conversion is followed closely.
  const delay = isBusy() || isUpdating() ? BUSY_POLL_MILLISECONDS : IDLE_POLL_MILLISECONDS;
  model.pollTimer = setTimeout(refresh, delay);
}

function render() {
  const { app, selection } = model;
  const ids = fileIds();
  selection.keep(ids);
  if (selection.focus === null && ids.length > 0) {
    selection.selectOnly(ids[0]);
  }
  renderFiles(app.files, selection, isBusy());
  renderMode(app.mode, app.windowOpen, hasNativePicker());
  renderProgress(app.files, app.conversion, canDownload());
  renderWriteSkipped(app.files, app.defaults, isBusy());
  renderUpdateBanner(app.update, model.isBannerDismissed);
  if (app.mode === MODES.container && app.roots.length === 0) {
    showNotice(TEXT.noMounts);
  }
  // UI-13: while Settings is open, it shows the theme being chosen.
  if (!isSettingsOpen()) {
    applyTheme(app.defaults.theme);
  }
  refreshSettings();
  refreshUpdatePrompt();
}

async function refreshPreview() {
  const file = focusedFile();
  if (!file) {
    model.previewKey = null;
    renderEmptyPreview(TEXT.selectFile);
    return;
  }
  // ENC-23: the letters shown follow the file's language and the saved choice.
  const { language, romanianCommaLetters } = model.app.defaults;
  const key = JSON.stringify([
    file.id,
    file.status,
    file.reading,
    file.output,
    file.language,
    language,
    romanianCommaLetters,
  ]);
  if (key !== model.previewKey) {
    const preview = await api.preview(file.id);
    model.previewKey = key;
    model.preview = preview;
  }
  await refreshCandidates(file);
  // A slow answer about a file that changed or lost the focus meanwhile is not shown, and a
  // language still being typed is not thrown away.
  if (focusedFile() !== file || isTypingLanguage(file)) {
    return;
  }
  renderPreview(file, model.preview, {
    encodings: model.encodings,
    message: model.choiceMessage,
    targets: encodingTargets().length,
    languageTargets: languageTargets().length,
    savedLanguage: model.app.defaults.language,
    candidates: model.candidates,
    agreeingCount: agreeingFiles(file).length,
    repairTargets: repairTargets().length,
    canDownload: canDownload(),
  });
}

// UI-16: the language box holds text that differs from the file's language, and has the focus.
function isTypingLanguage(file) {
  const input = document.getElementById("file-language-input");
  const typed = input?.value.trim().toLowerCase() ?? "";
  return document.activeElement === input && typed !== (file.language ?? "").toLowerCase();
}

// ENC-20: the files whose folders agree on the same encoding as this file's, this one included.
function agreeingFiles(file) {
  if (file.problem?.reason !== REASONS.folderAgrees) {
    return [];
  }
  const { encoding } = file.problem;
  return model.app.files.filter(
    (other) =>
      other.problem?.reason === REASONS.folderAgrees && other.problem.encoding === encoding,
  );
}

// UI-15: suggestions for a file that needs review, asked for again only when it changes.
async function refreshCandidates(file) {
  if (!file.canChooseEncoding || file.status !== STATUSES.needsReview) {
    model.candidatesKey = null;
    model.candidates = [];
    return;
  }
  const key = JSON.stringify([file.id, file.encoding, model.app.defaults.language]);
  if (key === model.candidatesKey) {
    return;
  }
  const { candidates } = await api.candidates(file.id);
  model.candidatesKey = key;
  model.candidates = candidates;
}

async function perform(action) {
  try {
    await action();
  } catch (error) {
    if (!handleFailure(error)) {
      showListMessage(errorMessage(error));
    }
  }
  await refresh();
}

function reportAdded(result) {
  if (!result) {
    return;
  }
  const [first] = result.skipped;
  const messages = [];
  if (first) {
    messages.push(
      fill(TEXT.addSkipped, {
        count: result.skipped.length,
        path: first.path,
        reason: problemText(first.problem),
      }),
    );
  }
  if (result.limitReached) {
    messages.push(TEXT.limitReached);
  }
  if (result.added === 0 && messages.length === 0) {
    messages.push(TEXT.nothingAdded);
  }
  showListMessage(messages.join(" "));
}

async function addPaths(paths, includeSubfolders) {
  await perform(async () => reportAdded(await api.add(paths, includeSubfolders)));
}

// Dropped into a browser, files arrive as contents only (SAFE-15): folders are walked in the
// page, hidden entries are skipped (SAFE-14), and only .srt files are sent (NAME-01).
function droppedEntries(transfer) {
  return [...transfer.items].map((item) => item.webkitGetAsEntry?.()).filter(Boolean);
}

function readEntries(reader) {
  return new Promise((resolve, reject) => reader.readEntries(resolve, reject));
}

async function collectFiles(entry, files) {
  if (entry.name.startsWith(HIDDEN_NAME_PREFIX)) {
    return;
  }
  if (entry.isFile) {
    files.push(await new Promise((resolve, reject) => entry.file(resolve, reject)));
    return;
  }
  if (!entry.isDirectory) {
    return;
  }
  const reader = entry.createReader();
  for (let batch = await readEntries(reader); batch.length > 0; batch = await readEntries(reader)) {
    for (const child of batch) {
      await collectFiles(child, files);
    }
  }
}

async function uploadFiles(files) {
  const srtFiles = files.filter((file) => file.name.toLowerCase().endsWith(SRT_EXTENSION));
  const messages = [];
  const skippedCount = files.length - srtFiles.length;
  if (skippedCount > 0) {
    messages.push(fill(TEXT.notSrtDropped, { count: skippedCount }));
  }
  for (const [index, file] of srtFiles.entries()) {
    showListMessage(fill(TEXT.uploading, { current: index + 1, total: srtFiles.length }));
    try {
      await api.upload(file);
    } catch (error) {
      if (handleFailure(error)) {
        return;
      }
      messages.push(`${file.name}: ${errorMessage(error)}`);
      const isFull = [REASONS.tooMuchDropped, REASONS.listFull].includes(error.problem?.reason);
      if (isFull) {
        break;
      }
    }
  }
  showListMessage(messages.join(" "));
  await refresh();
}

// Everything the drop carries must be read before the first pause, while the browser still
// allows access to it.
async function handleDrop(event) {
  event.preventDefault();
  setDragging(false);
  if (isBusy() || model.isStopped) {
    return;
  }
  const entries = droppedEntries(event.dataTransfer);
  const plainFiles = [...event.dataTransfer.files];
  if (entries.length === 0 && plainFiles.length === 0) {
    showListMessage(TEXT.dropUnreadable);
    return;
  }
  if (entries.length === 0) {
    await uploadFiles(plainFiles);
    return;
  }
  const files = [];
  for (const entry of entries) {
    await collectFiles(entry, files);
  }
  await uploadFiles(files);
}

// A choice made in the preview applies to each target file in turn; refusals are reported
// together, with the first one's reason.
async function applyToFiles(targets, call, refusedTemplate) {
  const refusals = [];
  for (const file of targets) {
    try {
      await call(file);
    } catch (error) {
      if (handleFailure(error)) {
        return;
      }
      refusals.push({ file, error });
    }
  }
  model.choiceMessage = refusalMessage(targets, refusals, refusedTemplate);
  model.previewKey = null;
  await refresh();
}

// ENC-21: files that can keep their UTF-8 lines do so when the box is ticked.
async function chooseEncoding(encoding, targets = encodingTargets()) {
  const keep = document.getElementById("keep-utf8-lines")?.checked ?? false;
  const choose = (file) => api.chooseEncoding(file.id, encoding, keep && file.canKeepUtf8Lines);
  await applyToFiles(targets, choose, TEXT.encodingRefused);
}

// UI-16: an empty language makes the files follow Settings again.
async function chooseLanguage(text) {
  const language = text.trim() || null;
  const choose = (file) => api.setLanguage(file.id, language);
  await applyToFiles(languageTargets(), choose, TEXT.languageRefused);
}

// ENC-22: Repair, or Keep as it is.
async function chooseRepair(repair) {
  const choose = (file) => api.repair(file.id, repair);
  await applyToFiles(repairTargets(), choose, TEXT.repairRefused);
}

function refusalMessage(targets, refusals, template) {
  const [first] = refusals;
  if (!first) {
    return "";
  }
  if (targets.length === 1) {
    return errorMessage(first.error);
  }
  return fill(template, {
    count: refusals.length,
    name: first.file.name,
    reason: errorMessage(first.error),
  });
}

// Search opens a file by selecting it in the list.
function selectFile(id) {
  if (!fileIds().includes(id)) {
    return;
  }
  model.selection.selectOnly(id);
  showSelection();
}

// The main list: the preview follows the focused file, and Delete removes the selected ones.
function showSelection() {
  renderFiles(model.app.files, model.selection, isBusy());
  focusFileRow(model.selection.focus);
  model.choiceMessage = "";
  refreshPreview().catch(handleFailure);
}

async function removeFiles(ids) {
  if (ids.length === 0 || isBusy()) {
    return;
  }
  const firstIndex = fileIds().indexOf(ids[0]);
  await perform(async () => {
    for (const id of ids) {
      await api.remove(id);
    }
  });
  const remaining = fileIds();
  const next = remaining[Math.min(firstIndex, remaining.length - 1)];
  if (next !== undefined) {
    model.selection.selectOnly(next);
    showSelection();
  }
}

function bindFileTable() {
  elements.fileRows.addEventListener("click", (event) => {
    const removeId = event.target.closest("[data-remove]")?.dataset.remove;
    if (removeId) {
      removeFiles([Number(removeId)]);
      return;
    }
    const row = event.target.closest(".file-row");
    if (row) {
      model.selection.click(Number(row.dataset.id), fileIds(), event);
      showSelection();
    }
  });
  elements.fileRows.addEventListener("keydown", (event) => {
    if (event.key === KEYS.delete) {
      event.preventDefault();
      removeFiles(model.selection.inOrder(fileIds()));
      return;
    }
    if (model.selection.press(fileIds(), event)) {
      event.preventDefault();
      showSelection();
    }
  });
}

function bindPreview() {
  elements.previewBody.addEventListener("change", (event) => {
    const isEncodingChoice = ["encoding-select", "keep-utf8-lines"].includes(event.target.id);
    if (isEncodingChoice) {
      chooseEncoding(document.getElementById("encoding-select").value);
      return;
    }
    if (event.target.id === "file-language-input") {
      chooseLanguage(event.target.value);
    }
  });
  elements.previewBody.addEventListener("click", (event) => {
    if (event.target.id === "encoding-confirm") {
      chooseEncoding(document.getElementById("encoding-select").value);
      return;
    }
    if (event.target.id === "use-for-all-agreeing") {
      const file = focusedFile();
      chooseEncoding(file.problem.encoding, agreeingFiles(file));
      return;
    }
    if (event.target.id === "repair-yes" || event.target.id === "repair-no") {
      chooseRepair(event.target.id === "repair-yes");
      return;
    }
    if (event.target.id === "download-file") {
      const file = focusedFile();
      perform(() => downloadFile(file));
      return;
    }
    const suggestion = event.target.closest("[data-candidate]")?.dataset.candidate;
    if (suggestion) {
      chooseEncoding(suggestion);
    }
  });
}

// The Browse view selects like a file manager (UI-10).
function browseShortcuts() {
  const { app } = model;
  if (app.mode === MODES.container) {
    return app.roots.map((root) => ({ label: root, path: root }));
  }
  return [{ label: TEXT.homeShortcut, path: app.browseStart }];
}

// The entries shown, in order: folders, then files, matching the filter.
// The rows' keys, in the order they are shown: folders, files, then files whose names are not
// UTF-8 (NAME-11).
function browsePaths() {
  const { listing, mode, filter } = model.browse;
  if (!listing) {
    return [];
  }
  const shown = (name) => name.toLowerCase().includes(filter.toLowerCase());
  const isFileMode = mode === BROWSE_MODES.files;
  const names = isFileMode ? [...listing.folders, ...listing.files] : listing.folders;
  const paths = names.filter(shown).map((name) => joinPath(listing.path, name));
  if (!isFileMode) {
    return paths;
  }
  const rawKeys = listing.rawFiles
    .filter(({ display }) => shown(display))
    .map(({ hex }) => BROWSE_RAW_KEY_PREFIX + hex);
  return [...paths, ...rawKeys];
}

// NAME-11: a file whose name is not UTF-8 is added by its path's bytes.
function requestedPath(key) {
  if (!key.startsWith(BROWSE_RAW_KEY_PREFIX)) {
    return key;
  }
  return { hex: key.slice(BROWSE_RAW_KEY_PREFIX.length) };
}

function renderBrowseDialog() {
  const { listing, mode, selection, message, filter } = model.browse;
  renderBrowse(listing, mode, selection, browseShortcuts(), message, filter, model.app.roots);
}

function showBrowseSelection() {
  const { mode, selection } = model.browse;
  renderBrowseSelection(mode, selection);
  focusBrowseEntry(selection.focus);
}

// Only the latest navigation counts, so a slow answer never replaces a newer folder.
async function browseTo(path) {
  const { browse } = model;
  browse.navigation += 1;
  const navigation = browse.navigation;
  let listing;
  try {
    listing = await api.browse(path);
  } catch (error) {
    if (handleFailure(error)) {
      elements.browseDialog.close();
      return;
    }
    if (navigation === browse.navigation) {
      browse.message = errorMessage(error);
      renderBrowseDialog();
    }
    return;
  }
  if (navigation !== browse.navigation) {
    return;
  }
  showListing(listing);
}

function showListing(listing) {
  const { browse } = model;
  browse.listing = listing;
  browse.folders = new Set(
    browse.listing.folders.map((name) => joinPath(browse.listing.path, name)),
  );
  browse.filter = "";
  // UI-10: files picked in several folders are added together; one output folder is one pick.
  if (browse.mode === BROWSE_MODES.folder) {
    browse.selection.clear();
  }
  browse.selection.placeFocus(browsePaths()[0] ?? null);
  browse.message = "";
  renderBrowseDialog();
  // A listing that arrives while a folder is being typed leaves the typing alone.
  if (browse.isTypingPath) {
    return;
  }
  focusBrowseEntry(browse.selection.focus);
  showBrowsePath(listing.path);
}

async function openBrowse(mode, startPath, onFolderChosen = null) {
  model.browse = newBrowse(mode, onFolderChosen);
  renderBrowseDialog();
  elements.browseDialog.showModal();
  await browseTo(startPath);
}

function goUp() {
  const parent = model.browse.listing?.parent;
  if (parent) {
    browseTo(parent);
  }
}

async function confirmBrowse() {
  const { mode, listing, selection } = model.browse;
  if (!listing) {
    return;
  }
  const selected = [...selection.selected];
  if (mode === BROWSE_MODES.folder) {
    elements.browseDialog.close();
    await model.browse.onFolderChosen(selected.length === 1 ? selected[0] : listing.path);
    return;
  }
  if (selected.length === 0) {
    return;
  }
  elements.browseDialog.close();
  await addPaths(selected.map(requestedPath), elements.subfoldersCheckbox.checked);
}

async function addCurrentFolder() {
  const { listing } = model.browse;
  if (!listing) {
    return;
  }
  elements.browseDialog.close();
  await addPaths([listing.path], elements.subfoldersCheckbox.checked);
}

// Double-click or Enter opens a folder; on a file it adds the selection, as in GTK's picker.
function activate(path) {
  const { folders, selection, mode } = model.browse;
  if (folders.has(path)) {
    // The folder was clicked to open it, not to add it.
    selection.selected.delete(path);
    browseTo(path);
    return;
  }
  if (mode === BROWSE_MODES.files) {
    confirmBrowse();
  }
}

// In folder mode only one folder can be picked, so modifier keys are ignored.
function selectionEvent(event) {
  if (model.browse.mode === BROWSE_MODES.files) {
    return event;
  }
  return { key: event.key, ctrlKey: false, metaKey: false, shiftKey: false };
}

function bindBrowseList() {
  const list = elements.browseEntries;
  list.addEventListener("click", (event) => {
    const path = event.target.closest("[data-key]")?.dataset.key;
    if (path) {
      model.browse.selection.click(path, browsePaths(), selectionEvent(event));
      showBrowseSelection();
    }
  });
  list.addEventListener("dblclick", (event) => {
    const path = event.target.closest("[data-key]")?.dataset.key;
    if (path) {
      activate(path);
    }
  });
  list.addEventListener("keydown", (event) => {
    const { selection } = model.browse;
    if (event.key === KEYS.enter && selection.focus !== null) {
      event.preventDefault();
      activate(selection.focus);
      return;
    }
    if (event.key === KEYS.backspace) {
      event.preventDefault();
      goUp();
      return;
    }
    if (selection.press(browsePaths(), selectionEvent(event))) {
      event.preventDefault();
      showBrowseSelection();
    }
  });
}

// The desktop window hands over what its file picker chose and what was dropped onto it, by
// their places on disk, so they are added like browsed files. Folders bring their sub-folders.
async function handlePicked(pick, paths) {
  setDragging(false);
  const onFolderChosen = model.pendingFolderPick;
  if (pick === NATIVE_PICKS.outputFolder) {
    model.pendingFolderPick = null;
  }
  if (paths.length === 0) {
    return;
  }
  // NAME-11: files come as strings, or by their bytes when their names are not UTF-8; an
  // output folder must have a UTF-8 name.
  const [folder] = paths;
  if (pick === NATIVE_PICKS.outputFolder && typeof folder === "string") {
    await onFolderChosen?.(folder);
    return;
  }
  if (pick === NATIVE_PICKS.outputFolder) {
    return;
  }
  await addPaths(paths, pick !== NATIVE_PICKS.files);
}

function bindNativePicker() {
  window[PICKED_CALLBACK] = handlePicked;
  elements.addFilesButton.addEventListener("click", () =>
    window.ipc.postMessage(NATIVE_PICKS.files),
  );
  elements.addFolderButton.addEventListener("click", () =>
    window.ipc.postMessage(NATIVE_PICKS.folders),
  );
}

// One folder, from the system's picker in the desktop window or the Browse view elsewhere.
function chooseFolder(startPath, onFolderChosen) {
  if (hasNativePicker()) {
    model.pendingFolderPick = onFolderChosen;
    window.ipc.postMessage(NATIVE_PICKS.outputFolder);
    return;
  }
  openBrowse(BROWSE_MODES.folder, startPath, onFolderChosen);
}

function bindBrowseDialog() {
  elements.browseButton.addEventListener("click", () =>
    openBrowse(BROWSE_MODES.files, model.app?.browseStart),
  );
  elements.browseCrumbs.addEventListener("click", (event) => {
    const path = event.target.closest("[data-path]")?.dataset.path;
    if (path) {
      browseTo(path);
    }
  });
  elements.browseFilter.addEventListener("input", () => {
    model.browse.filter = elements.browseFilter.value;
    model.browse.selection.placeFocus(browsePaths()[0] ?? null);
    renderBrowseDialog();
  });
  elements.browseClearSelection.addEventListener("click", () => {
    const { selection } = model.browse;
    selection.clear();
    selection.placeFocus(browsePaths()[0] ?? null);
    showBrowseSelection();
  });
  elements.browseUp.addEventListener("click", goUp);
  elements.browsePath.addEventListener("input", () => {
    model.browse.isTypingPath = true;
  });
  elements.browsePath.addEventListener("keydown", (event) => {
    if (event.key === KEYS.enter) {
      event.preventDefault();
      model.browse.isTypingPath = false;
      browseTo(elements.browsePath.value.trim());
    }
  });
  elements.browseShortcuts.addEventListener("click", (event) => {
    const path = event.target.closest("[data-path]")?.dataset.path;
    if (path) {
      browseTo(path);
    }
  });
  elements.browseDialog.addEventListener("keydown", (event) => {
    const wantsLocation = (event.ctrlKey || event.metaKey) && event.key === KEYS.location;
    if (wantsLocation) {
      event.preventDefault();
      elements.browsePath.select();
      return;
    }
    if (event.altKey && event.key === KEYS.arrowUp) {
      event.preventDefault();
      goUp();
    }
  });
  bindBrowseList();
  elements.browseCancel.addEventListener("click", () => elements.browseDialog.close());
  elements.browseConfirm.addEventListener("click", confirmBrowse);
  elements.browseAddFolder.addEventListener("click", addCurrentFolder);
}

function bindDragAndDrop() {
  let depth = 0;
  document.addEventListener("dragenter", (event) => {
    event.preventDefault();
    depth += 1;
    setDragging(true);
  });
  document.addEventListener("dragleave", () => {
    depth = Math.max(depth - 1, 0);
    setDragging(depth > 0);
  });
  document.addEventListener("dragover", (event) => event.preventDefault());
  document.addEventListener("drop", (event) => {
    depth = 0;
    handleDrop(event);
  });
}

// UI-17: each file is saved under its output name.
async function downloadFile(file) {
  saveFile(await api.output(file.id), fileName(file.output));
}

// UI-17: one after another, in list order.
function downloadConverted() {
  const converted = model.app.files.filter((file) => file.status === STATUSES.converted);
  return perform(async () => {
    for (const file of converted) {
      await downloadFile(file);
    }
  });
}

// UI-18: the output folder becomes where files are written, as choosing it in Settings does
// (SET-01), and the skipped files are converted again.
function writeSkippedToOutputFolder() {
  const defaults = { ...model.app.defaults, destination: DESTINATIONS.outputFolder };
  return perform(async () => {
    await api.saveDefaults(defaults);
    await api.convert();
  });
}

function bindControls() {
  elements.clearButton.addEventListener("click", () => perform(() => api.clear()));
  elements.convertButton.addEventListener("click", () => perform(() => api.convert()));
  elements.writeSkippedButton.addEventListener("click", writeSkippedToOutputFolder);
  elements.downloadAllButton.addEventListener("click", downloadConverted);
  elements.cancelButton.addEventListener("click", () => perform(() => api.cancel()));
  elements.quitButton.addEventListener("click", async () => {
    await api.quit().catch(() => null);
    model.isStopped = true;
    clearTimeout(model.pollTimer);
    showNotice(TEXT.stopped);
    disableControls();
  });
}

// The Settings and Search dialogs, and the update banner, which opens Settings at Updates.
function bindDialogs() {
  const context = {
    getApp: () => model.app,
    refresh,
    handleFailure,
    errorMessage,
    chooseFolder,
    openSettings,
    selectFile,
  };
  setUpDialogs();
  setUpSettingsDialog(context);
  setUpSearch(context);
  elements.searchButton.addEventListener("click", openSearch);
  elements.settingsButton.addEventListener("click", () => openSettings());
  dialogElements.updateBannerOpen.addEventListener("click", () =>
    openSettings(dialogElements.updateCard.id),
  );
  dialogElements.updateBannerDismiss.addEventListener("click", () => {
    model.isBannerDismissed = true;
    renderUpdateBanner(model.app.update, true);
  });
  document.addEventListener("keydown", (event) => {
    const wantsSearch = (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === SEARCH_KEY;
    if (wantsSearch && !model.isStopped) {
      event.preventDefault();
      openSearch();
    }
  });
}

async function start() {
  connect();
  if (hasNativePicker()) {
    bindNativePicker();
  }
  bindFileTable();
  bindPreview();
  bindBrowseDialog();
  bindDragAndDrop();
  bindControls();
  bindDialogs();
  try {
    const [encodings, languages] = await Promise.all([api.encodings(), api.languages()]);
    model.encodings = encodings;
    setUpLanguageList(languages.hinted);
  } catch (error) {
    if (handleFailure(error)) {
      return;
    }
  }
  await refresh();
}

start();
