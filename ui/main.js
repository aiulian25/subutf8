import { ApiError, api, connect } from "./api.js";
import {
  BROWSE_MODES,
  BUSY_POLL_MILLISECONDS,
  HIDDEN_NAME_PREFIX,
  IDLE_POLL_MILLISECONDS,
  KEYS,
  MODES,
  NATIVE_PICKS,
  NETWORK_FAILURE_STATUS,
  PAYLOAD_TOO_LARGE_STATUS,
  REASONS,
  SRT_EXTENSION,
  PICKED_CALLBACK,
  TEXT,
  UNAUTHORIZED_STATUS,
} from "./constants.js";
import { Selection } from "./selection.js";
import {
  disableControls,
  elements,
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
  renderSettings,
  setDragging,
  setUpSettings,
  showBrowsePath,
  showListMessage,
  showNotice,
  showSettingsMessage,
} from "./view.js";

const model = {
  app: null,
  encodings: [],
  selection: new Selection(),
  previewKey: null,
  preview: null,
  encodingMessage: "",
  pollTimer: null,
  isStopped: false,
  browse: newBrowse(BROWSE_MODES.files),
};

function newBrowse(mode) {
  return {
    mode,
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

function isBusy() {
  return model.app?.conversion != null;
}

function fileIds() {
  return model.app?.files.map((file) => file.id) ?? [];
}

function focusedFile() {
  return model.app?.files.find((file) => file.id === model.selection.focus) ?? null;
}

// A chosen encoding applies to every selected file that takes one, or else to the file shown.
function encodingTargets() {
  const selected = (model.app?.files ?? []).filter(
    (file) => model.selection.has(file.id) && file.canChooseEncoding,
  );
  const focused = focusedFile();
  if (selected.length > 0 || !focused?.canChooseEncoding) {
    return selected;
  }
  return [focused];
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
  const delay = isBusy() ? BUSY_POLL_MILLISECONDS : IDLE_POLL_MILLISECONDS;
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
  renderSettings(app.settings, isBusy());
  renderMode(app.mode, app.windowOpen, hasNativePicker());
  renderProgress(app.files, app.conversion);
  if (app.mode === MODES.container && app.roots.length === 0) {
    showNotice(TEXT.noMounts);
  }
}

async function refreshPreview() {
  const file = focusedFile();
  if (!file) {
    model.previewKey = null;
    renderEmptyPreview(TEXT.selectFile);
    return;
  }
  const key = JSON.stringify([file.id, file.status, file.encoding, file.output]);
  if (key !== model.previewKey) {
    model.previewKey = key;
    model.preview = await api.preview(file.id);
  }
  const targets = encodingTargets().length;
  renderPreview(file, model.preview, model.encodings, model.encodingMessage, targets);
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

function currentSettings() {
  const { settings } = model.app;
  return {
    language: elements.languageInput.value.trim() || null,
    destination: elements.destinationSelect.value,
    outputFolder: settings.outputFolder,
    collisionPolicy: elements.collisionSelect.value,
  };
}

async function saveSettings(changes) {
  try {
    await api.saveSettings({ ...currentSettings(), ...changes });
    showSettingsMessage("");
  } catch (error) {
    if (handleFailure(error)) {
      return;
    }
    showSettingsMessage(errorMessage(error));
  }
  await refresh();
}

async function chooseEncoding(encoding) {
  const targets = encodingTargets();
  const refusals = [];
  for (const file of targets) {
    try {
      await api.chooseEncoding(file.id, encoding);
    } catch (error) {
      if (handleFailure(error)) {
        return;
      }
      refusals.push({ file, error });
    }
  }
  model.encodingMessage = encodingMessage(targets, refusals);
  model.previewKey = null;
  await refresh();
}

function encodingMessage(targets, refusals) {
  const [first] = refusals;
  if (!first) {
    return "";
  }
  if (targets.length === 1) {
    return errorMessage(first.error);
  }
  return fill(TEXT.encodingRefused, {
    count: refusals.length,
    name: first.file.name,
    reason: errorMessage(first.error),
  });
}

// The main list: the preview follows the focused file, and Delete removes the selected ones.
function showSelection() {
  renderFiles(model.app.files, model.selection, isBusy());
  focusFileRow(model.selection.focus);
  model.encodingMessage = "";
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
    if (event.target.id === "encoding-select") {
      chooseEncoding(event.target.value);
    }
  });
  elements.previewBody.addEventListener("click", (event) => {
    if (event.target.id === "encoding-confirm") {
      chooseEncoding(document.getElementById("encoding-select").value);
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
function browsePaths() {
  const { listing, mode, filter } = model.browse;
  if (!listing) {
    return [];
  }
  const names =
    mode === BROWSE_MODES.files ? [...listing.folders, ...listing.files] : listing.folders;
  return names
    .filter((name) => name.toLowerCase().includes(filter.toLowerCase()))
    .map((name) => joinPath(listing.path, name));
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

async function openBrowse(mode, startPath) {
  model.browse = newBrowse(mode);
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
    await saveSettings({ outputFolder: selected.length === 1 ? selected[0] : listing.path });
    return;
  }
  if (selected.length === 0) {
    return;
  }
  elements.browseDialog.close();
  await addPaths(selected, elements.subfoldersCheckbox.checked);
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
  if (paths.length === 0) {
    return;
  }
  if (pick === NATIVE_PICKS.outputFolder) {
    await saveSettings({ outputFolder: paths[0] });
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

function chooseOutputFolder() {
  if (hasNativePicker()) {
    window.ipc.postMessage(NATIVE_PICKS.outputFolder);
    return;
  }
  openBrowse(BROWSE_MODES.folder, model.app?.settings.outputFolder);
}

function bindBrowseDialog() {
  elements.browseButton.addEventListener("click", () =>
    openBrowse(BROWSE_MODES.files, model.app?.browseStart),
  );
  elements.outputFolderButton.addEventListener("click", chooseOutputFolder);
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

function bindControls() {
  elements.clearButton.addEventListener("click", () => perform(() => api.clear()));
  elements.convertButton.addEventListener("click", () => perform(() => api.convert()));
  elements.cancelButton.addEventListener("click", () => perform(() => api.cancel()));
  elements.quitButton.addEventListener("click", async () => {
    await api.quit().catch(() => null);
    model.isStopped = true;
    clearTimeout(model.pollTimer);
    showNotice(TEXT.stopped);
    disableControls();
  });
  elements.languageInput.addEventListener("change", () => saveSettings({}));
  elements.destinationSelect.addEventListener("change", () => saveSettings({}));
  elements.collisionSelect.addEventListener("change", () => saveSettings({}));
}

async function start() {
  setUpSettings();
  connect();
  if (hasNativePicker()) {
    bindNativePicker();
  }
  bindFileTable();
  bindPreview();
  bindBrowseDialog();
  bindDragAndDrop();
  bindControls();
  try {
    model.encodings = await api.encodings();
  } catch (error) {
    if (handleFailure(error)) {
      return;
    }
  }
  await refresh();
}

start();
