import {
  BROWSE_MODES,
  BROWSE_RAW_KEY_PREFIX,
  DESTINATIONS,
  DOWNLOAD_ADDRESS_LIFETIME_MILLISECONDS,
  MODES,
  PATH_SEPARATOR,
  PLACEHOLDER_PATTERN,
  SHORT_PATH_PARTS,
  REASONS,
  REASON_MESSAGES,
  STATUSES,
  STATUS_LABELS,
  TEXT,
  WARNING_MESSAGES,
} from "./constants.js";

// UI-03: every file name and subtitle line goes through textContent, never through markup.

export const elements = {
  notice: document.getElementById("notice"),
  searchButton: document.getElementById("search-button"),
  settingsButton: document.getElementById("settings-button"),
  quitButton: document.getElementById("quit-button"),
  addFilesButton: document.getElementById("add-files-button"),
  addFolderButton: document.getElementById("add-folder-button"),
  dropZone: document.getElementById("drop-zone"),
  dropHint: document.getElementById("drop-hint"),
  browseButton: document.getElementById("browse-button"),
  clearButton: document.getElementById("clear-button"),
  listMessage: document.getElementById("list-message"),
  writeSkippedButton: document.getElementById("write-skipped-button"),
  fileRows: document.getElementById("file-rows"),
  emptyList: document.getElementById("empty-list"),
  previewBody: document.getElementById("preview-body"),
  progress: document.getElementById("progress"),
  summary: document.getElementById("summary"),
  cancelButton: document.getElementById("cancel-button"),
  downloadAllButton: document.getElementById("download-all-button"),
  convertButton: document.getElementById("convert-button"),
  browseDialog: document.getElementById("browse-dialog"),
  browseTitle: document.getElementById("browse-title"),
  browseUp: document.getElementById("browse-up"),
  browsePath: document.getElementById("browse-path"),
  browseCrumbs: document.getElementById("browse-crumbs"),
  browseShortcuts: document.getElementById("browse-shortcuts"),
  browseFilter: document.getElementById("browse-filter"),
  browseTrayText: document.getElementById("browse-tray-text"),
  browseClearSelection: document.getElementById("browse-clear-selection"),
  browseEntries: document.getElementById("browse-entries"),
  browseMessage: document.getElementById("browse-message"),
  subfoldersLabel: document.getElementById("subfolders-label"),
  subfoldersCheckbox: document.getElementById("subfolders-checkbox"),
  browseCancel: document.getElementById("browse-cancel"),
  browseAddFolder: document.getElementById("browse-add-folder"),
  browseConfirm: document.getElementById("browse-confirm"),
};

export function fill(template, values) {
  return template.replace(PLACEHOLDER_PATTERN, (_, key) => String(values[key] ?? ""));
}

export function problemText(problem) {
  if (!problem) {
    return "";
  }
  const template = REASON_MESSAGES[problem.reason] ?? REASON_MESSAGES[REASONS.internal];
  return fill(template, {
    line: problem.line,
    system: problem.systemReason,
    name: problem.relatedName,
    count: problem.count,
    encoding: problem.encoding,
    misreadAs: problem.misreadAs,
  });
}

// ENC-19: one line per kind of warning, counting the others of its kind.
function warningText(warning) {
  const more = warning.count > 1 ? fill(TEXT.moreLines, { count: warning.count - 1 }) : "";
  return fill(WARNING_MESSAGES[warning.warning] ?? "", { line: warning.line, more });
}

// Long paths are shown by their last parts; the full path is in the tooltip.
export function shortPath(path) {
  const parts = path.split(PATH_SEPARATOR).filter(Boolean);
  if (parts.length <= SHORT_PATH_PARTS) {
    return path;
  }
  return TEXT.pathEllipsis + parts.slice(-SHORT_PATH_PARTS).join(PATH_SEPARATOR);
}

function pathLabel(className, path, shown = shortPath(path)) {
  const label = create("div", className, shown);
  label.title = path;
  return label;
}

export function fileName(path) {
  return path.split(PATH_SEPARATOR).pop();
}

// UI-17: saves a file the page fetched, under the given name, from an address that lives only
// in this page.
export function saveFile(blob, name) {
  const address = URL.createObjectURL(blob);
  const link = create("a");
  link.href = address;
  link.download = name;
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(address), DOWNLOAD_ADDRESS_LIFETIME_MILLISECONDS);
}

export function joinPath(folder, name) {
  return folder.endsWith(PATH_SEPARATOR) ? folder + name : folder + PATH_SEPARATOR + name;
}

export function create(tag, className, text) {
  const element = document.createElement(tag);
  if (className) {
    element.className = className;
  }
  if (text !== undefined) {
    element.textContent = text;
  }
  return element;
}

export function fillSelect(select, options) {
  select.replaceChildren(
    ...options.map(([value, label]) => {
      const option = create("option", "", label);
      option.value = value;
      return option;
    }),
  );
}

export function showNotice(message) {
  elements.notice.textContent = message;
  elements.notice.hidden = !message;
}

// Once the app has stopped, nothing on the page can work any more.
export function disableControls() {
  for (const control of document.querySelectorAll("button, input, select")) {
    control.disabled = true;
  }
}

export function showListMessage(message) {
  elements.listMessage.textContent = message;
  elements.listMessage.hidden = !message;
}

// UI-11: in the app's own window, closing the window quits, so Quit shows only in a browser.
// Files dropped onto the window keep their folders; a browser only hands over their contents.
// The window opens the system's file picker; a browser uses the app's own Browse view.
export function renderMode(mode, windowOpen, hasNativePicker) {
  elements.quitButton.hidden = mode !== MODES.desktop || windowOpen;
  elements.dropHint.textContent = windowOpen ? TEXT.dropHintWindow : TEXT.dropHintBrowser;
  elements.addFilesButton.hidden = !hasNativePicker;
  elements.addFolderButton.hidden = !hasNativePicker;
  elements.browseButton.hidden = hasNativePicker;
}

export function statusBadge(status) {
  return create("span", `status status-${status}`, STATUS_LABELS[status] ?? status);
}

function fileRow(file, isSelected, isFocused, isBusy) {
  const row = create("tr", "file-row");
  row.dataset.id = String(file.id);
  row.tabIndex = isFocused ? 0 : -1;
  row.setAttribute("aria-selected", String(isSelected));

  const nameCell = create("td");
  const name = create("div", "file-name", file.name);
  // UI-16: a file with its own subtitle language shows it.
  if (file.language) {
    name.append(create("span", "language-badge", file.language));
  }
  nameCell.append(name);
  if (file.folder) {
    nameCell.append(pathLabel("file-folder", file.folder));
  }

  const encodingCell = create("td", "file-encoding", file.reading ?? TEXT.noEncoding);

  const statusCell = create("td");
  statusCell.append(statusBadge(file.status));
  if (file.problem) {
    statusCell.append(create("div", "file-reason", problemText(file.problem)));
  }

  const outputCell = create("td");
  outputCell.append(
    file.output
      ? pathLabel("file-output", file.output, fileName(file.output))
      : create("div", "file-output", TEXT.noOutput),
  );

  const removeCell = create("td");
  const removeButton = create("button", "quiet-button remove-button", TEXT.removeSymbol);
  removeButton.type = "button";
  removeButton.tabIndex = -1;
  removeButton.dataset.remove = String(file.id);
  removeButton.disabled = isBusy;
  removeButton.setAttribute("aria-label", fill(TEXT.removeLabel, { name: file.name }));
  removeCell.append(removeButton);

  row.append(nameCell, encodingCell, statusCell, outputCell, removeCell);
  return row;
}

const renderedRows = new Map();

// Rows are rebuilt only when their file or selection changed, so a long list stays fast
// while the progress updates several times a second.
export function renderFiles(files, selection, isBusy) {
  const hadFocus = elements.fileRows.contains(document.activeElement);
  const rows = files.map((file) => {
    const isSelected = selection.has(file.id);
    const isFocused = selection.focus === file.id;
    const signature = JSON.stringify([file, isSelected, isFocused, isBusy]);
    const cached = renderedRows.get(file.id);
    if (cached?.signature === signature) {
      return cached.row;
    }
    const row = fileRow(file, isSelected, isFocused, isBusy);
    renderedRows.set(file.id, { row, signature });
    return row;
  });
  const listedIds = new Set(files.map((file) => file.id));
  for (const id of renderedRows.keys()) {
    if (!listedIds.has(id)) {
      renderedRows.delete(id);
    }
  }
  const current = [...elements.fileRows.children];
  const isSame =
    current.length === rows.length && rows.every((row, index) => row === current[index]);
  if (!isSame) {
    elements.fileRows.replaceChildren(...rows);
  }
  elements.emptyList.hidden = files.length > 0;
  if (hadFocus) {
    focusFileRow(selection.focus);
  }
}

export function focusFileRow(id) {
  const row = renderedRows.get(id)?.row;
  row?.focus();
  row?.scrollIntoView({ block: "nearest" });
}

function countBy(files) {
  const counts = new Map();
  for (const file of files) {
    counts.set(file.status, (counts.get(file.status) ?? 0) + 1);
  }
  return counts;
}

// UI-05 to UI-07: Convert includes Ready files and those skipped or failed before.
// UI-17: `canDownload` is false in the desktop window, which cannot save downloads.
export function renderProgress(files, conversion, canDownload) {
  const counts = countBy(files);
  const pendingCount = files.filter((file) => file.isPending).length;
  const isBusy = conversion !== null;
  const convertedCount = counts.get(STATUSES.converted) ?? 0;
  elements.progress.hidden = !isBusy;
  elements.cancelButton.hidden = !isBusy;
  elements.downloadAllButton.hidden = isBusy || !canDownload || convertedCount === 0;
  elements.downloadAllButton.textContent = fill(TEXT.downloadAll, { count: convertedCount });
  elements.convertButton.disabled = isBusy || pendingCount === 0;
  elements.clearButton.disabled = isBusy || files.length === 0;
  if (isBusy) {
    elements.progress.max = Math.max(conversion.total, 1);
    elements.progress.value = conversion.finished;
    elements.summary.textContent = fill(TEXT.converting, conversion);
    return;
  }
  const parts = [
    [STATUSES.ready, TEXT.summaryParts.ready],
    [STATUSES.converted, TEXT.summaryParts.converted],
    [STATUSES.needsReview, TEXT.summaryParts.review],
    [STATUSES.skipped, TEXT.summaryParts.skipped],
    [STATUSES.failed, TEXT.summaryParts.failed],
  ]
    .filter(([status]) => counts.has(status))
    .map(([status, template]) => fill(template, { count: counts.get(status) }));
  elements.summary.textContent = parts.join(TEXT.summarySeparator);
}

// UI-18: browsed files skipped because their folder is read-only can go to the output folder;
// dropped files go there already, so they do not count.
export function renderWriteSkipped(files, defaults, isBusy) {
  const count = files.filter(
    (file) => file.folder && file.problem?.reason === REASONS.folderNotWritable,
  ).length;
  const button = elements.writeSkippedButton;
  const writesBeside = defaults.destination === DESTINATIONS.besideOriginals;
  button.hidden = isBusy || count === 0 || !writesBeside;
  if (button.hidden) {
    return;
  }
  const template =
    count > 1 ? TEXT.writeSkippedToOutputFolder : TEXT.writeSkippedFileToOutputFolder;
  button.textContent = fill(template, { count, folder: shortPath(defaults.outputFolder) });
  button.title = defaults.outputFolder;
}

export function renderEmptyPreview(message) {
  elements.previewBody.replaceChildren(create("p", "muted", message));
}

// UI-04: numbers, timings and text of up to five cues, decoded with the current encoding.
// `choice` is what choosing an encoding, a language or a repair offers: the encodings, the
// message about the last choice, how many selected files each applies to, the saved language,
// the suggestions (UI-15), how many files share this one's folder agreement (ENC-20), and
// whether a converted file can be downloaded (UI-17).
export function renderPreview(file, preview, choice) {
  const parts = [create("p", "preview-title", file.name)];

  const statusLine = create("p");
  statusLine.append(statusBadge(file.status));
  parts.push(statusLine);
  if (file.problem) {
    parts.push(create("p", `problem problem-${file.status}`, problemText(file.problem)));
  }
  if (file.output) {
    parts.push(create("p", "path", fill(TEXT.writtenTo, { path: file.output })));
  }
  if (file.status === STATUSES.converted && choice.canDownload) {
    const download = create("button", "primary-button download-button", TEXT.download);
    download.type = "button";
    download.id = "download-file";
    parts.push(download);
  }
  if (file.warnings.length > 0) {
    const list = create("ul", "warnings");
    list.append(...file.warnings.map((warning) => create("li", "", warningText(warning))));
    parts.push(list);
  }

  parts.push(encodingControl(file, choice.encodings, choice.targets));
  if (file.canRepair && preview.repair) {
    parts.push(...repairChoice(file, preview.repair, choice.repairTargets));
  }
  if (file.isPending || file.status === STATUSES.needsReview) {
    parts.push(languageControl(file, choice));
  }
  if (choice.message) {
    parts.push(create("p", "problem", choice.message));
  }
  if (choice.agreeingCount > 1) {
    parts.push(useForAllAgreeingButton(file, choice.agreeingCount));
  }
  if (choice.candidates.length > 0) {
    const { candidates } = choice;
    parts.push(create("p", "muted", TEXT.suggestionsTitle), suggestionList(file, candidates));
  }

  if (preview.problem) {
    parts.push(create("p", "problem", problemText(preview.problem)));
  }
  // UI-14.
  if (preview.brokenLine) {
    parts.push(create("p", "muted", fill(TEXT.brokenLine, { line: preview.brokenLine.line })));
    const block = create("div", "cue broken-line");
    block.append(create("pre", "cue-text", preview.brokenLine.text));
    parts.push(block);
  }
  for (const cue of preview.cues) {
    parts.push(cueBlock(cue));
  }
  if (preview.cues.length === 0 && !preview.problem) {
    parts.push(create("p", "muted", TEXT.nothingToPreview));
  }
  elements.previewBody.replaceChildren(...parts);
}

// UI-16: the file's own subtitle language; left empty, the file follows Settings.
function languageControl(file, choice) {
  const label = create("label", "preview-encoding");
  const count = choice.languageTargets;
  const text = count > 1 ? fill(TEXT.fileLanguageForSelected, { count }) : TEXT.fileLanguage;
  const input = create("input", "file-language");
  input.id = "file-language-input";
  input.setAttribute("list", "language-list");
  input.autocomplete = "off";
  input.spellcheck = false;
  input.value = file.language ?? "";
  const savedLanguage = choice.savedLanguage ?? TEXT.noLanguage;
  input.placeholder = fill(TEXT.languageFromSettings, { language: savedLanguage });
  label.append(create("span", "muted", text), input);
  return label;
}

// ENC-20: one click gives every file its folder agrees on the same encoding.
function useForAllAgreeingButton(file, count) {
  const encoding = file.problem.encoding;
  const button = create(
    "button",
    "primary-button",
    fill(TEXT.useForAllAgreeing, { encoding, count }),
  );
  button.type = "button";
  button.id = "use-for-all-agreeing";
  return button;
}

// ENC-22: the first accented line as it is and repaired, then the choice, which applies to
// every selected garbled file. The choice made is shown pressed.
function repairChoice(file, sample, count) {
  const asIs = create("div", "cue garbled-line");
  asIs.append(create("pre", "cue-text", fill(TEXT.repairAsIs, { sample: sample.asIs })));
  const repaired = create("div", "cue");
  repaired.append(
    create("pre", "cue-text", fill(TEXT.repairRepaired, { sample: sample.repaired })),
  );
  const isChosen = file.status !== STATUSES.needsReview;
  const buttons = create("div", "repair-choice");
  buttons.append(
    pressableButton({
      id: "repair-yes",
      className: "primary-button",
      text: count > 1 ? fill(TEXT.repairSelected, { count }) : TEXT.repair,
      isPressed: isChosen && file.isRepaired,
    }),
    pressableButton({
      id: "repair-no",
      className: "",
      text: count > 1 ? fill(TEXT.keepSelected, { count }) : TEXT.keepAsIs,
      isPressed: isChosen && !file.isRepaired,
    }),
  );
  return [asIs, repaired, buttons];
}

function pressableButton({ id, className, text, isPressed }) {
  const button = create("button", className, text);
  button.type = "button";
  button.id = id;
  button.setAttribute("aria-pressed", String(isPressed));
  return button;
}

// UI-15: each suggestion shows the file's first accented line in that encoding.
function suggestionList(file, candidates) {
  const list = create("ul", "suggestions");
  for (const { encoding, sample } of candidates) {
    const button = create(
      "button",
      "quiet-button",
      fill(TEXT.suggestionLabel, { encoding, sample }),
    );
    button.type = "button";
    button.dataset.candidate = encoding;
    button.setAttribute("aria-pressed", String(encoding === file.encoding));
    const item = create("li");
    item.append(button);
    list.append(item);
  }
  return list;
}

function encodingControl(file, encodings, encodingTargets) {
  const container = create("div", "preview-encoding");
  if (!file.canChooseEncoding) {
    container.append(
      create("span", "muted", TEXT.encodingLabel),
      create("span", "", file.reading ?? TEXT.noEncoding),
    );
    return container;
  }
  const labelText =
    encodingTargets > 1
      ? fill(TEXT.encodingForSelected, { count: encodingTargets })
      : TEXT.encodingLabel;
  const label = create("label", "preview-encoding");
  label.append(create("span", "muted", labelText));
  const select = create("select");
  select.id = "encoding-select";
  for (const name of encodings) {
    const option = create("option", "", name);
    option.value = name;
    select.append(option);
  }
  select.value = file.encoding ?? "";
  label.append(select);
  container.append(label);
  // ENC-21: a damaged or mixed file keeps its lines that are already UTF-8.
  if (file.canKeepUtf8Lines) {
    const keep = create("label", "checkbox-label");
    const checkbox = create("input");
    checkbox.type = "checkbox";
    checkbox.id = "keep-utf8-lines";
    checkbox.checked = file.keepsUtf8Lines;
    keep.append(checkbox, TEXT.keepUtf8Lines);
    container.append(keep);
  }
  if (file.status === STATUSES.needsReview) {
    const confirm = create("button", "primary-button", TEXT.useEncoding);
    confirm.type = "button";
    confirm.id = "encoding-confirm";
    container.append(confirm);
  }
  return container;
}

function cueBlock(cue) {
  const block = create("div", "cue");
  const meta = [cue.number, cue.timing].filter(Boolean).join("  ");
  if (meta) {
    block.append(create("div", "cue-meta", meta));
  }
  block.append(create("pre", "cue-text", cue.text));
  if (cue.isClipped) {
    block.append(create("div", "cue-clipped", TEXT.clipped));
  }
  return block;
}

export function setDragging(isDragging) {
  elements.dropZone.classList.toggle("is-dragging", isDragging);
}

// The Browse view. In "files" mode it adds files and folders; in "folder" mode it picks the
// output folder. Entries are keyed by their full path.
const browseRows = new Map();

function isInside(path, roots) {
  return roots.some((root) => path === root || path.startsWith(joinPath(root, "")));
}

// Breadcrumbs, as in a file manager; folders outside the allowed area are shown but inactive.
function renderCrumbs(path, roots) {
  const parts = path.split(PATH_SEPARATOR).filter(Boolean);
  const crumbs = [{ label: TEXT.crumbRoot, path: PATH_SEPARATOR }];
  for (const part of parts) {
    crumbs.push({ label: part, path: joinPath(crumbs.at(-1).path, part) });
  }
  const items = crumbs.flatMap(({ label, path: crumbPath }, index) => {
    const button = create("button", "quiet-button", label);
    button.type = "button";
    button.dataset.path = crumbPath;
    button.disabled = !isInside(crumbPath, roots);
    const separator = create("span", "browse-crumb-separator", TEXT.crumbSeparator);
    return index === 0 ? [button] : [separator, button];
  });
  elements.browseCrumbs.replaceChildren(...items);
}

export function renderBrowse(listing, mode, selection, shortcuts, message, filter, roots) {
  const isFileMode = mode === BROWSE_MODES.files;
  elements.browseTitle.textContent = isFileMode ? TEXT.browseFilesTitle : TEXT.browseFolderTitle;
  elements.browseAddFolder.hidden = !isFileMode;
  elements.subfoldersLabel.hidden = !isFileMode;
  elements.browseEntries.setAttribute("aria-multiselectable", String(isFileMode));
  showBrowsePath(listing?.path ?? "");
  elements.browseEntries.dataset.path = listing?.path ?? "";
  elements.browseUp.disabled = !listing?.parent;
  elements.browseAddFolder.disabled = !listing;
  if (document.activeElement !== elements.browseFilter) {
    elements.browseFilter.value = filter;
  }
  renderCrumbs(listing?.path ?? PATH_SEPARATOR, roots);

  elements.browseShortcuts.replaceChildren(
    ...shortcuts.map(({ label, path }) => {
      const button = create("button", "quiet-button", label);
      button.type = "button";
      button.dataset.path = path;
      return button;
    }),
  );

  const shown = (name) => name.toLowerCase().includes(filter.toLowerCase());
  const folders = (listing?.folders ?? []).filter(shown);
  const files = isFileMode ? (listing?.files ?? []).filter(shown) : [];
  // NAME-11: names that are not UTF-8 show as far as they read, and are added by their bytes.
  const rawFiles = isFileMode
    ? (listing?.rawFiles ?? []).filter(({ display }) => shown(display))
    : [];
  browseRows.clear();
  elements.browseEntries.replaceChildren(
    ...folders.map((name) => browseEntry(name, joinPath(listing.path, name), true)),
    ...files.map((name) => browseEntry(name, joinPath(listing.path, name), false)),
    ...rawFiles.map(({ display, hex }) => browseEntry(display, BROWSE_RAW_KEY_PREFIX + hex, false)),
  );

  const isEmpty = listing && folders.length + files.length + rawFiles.length === 0;
  const notes = [
    message,
    isEmpty ? TEXT.emptyFolder : "",
    listing?.isTruncated ? TEXT.truncatedFolder : "",
  ];
  elements.browseMessage.textContent = notes.filter(Boolean).join(" ");
  renderBrowseSelection(mode, selection);
}

function browseEntry(name, path, isFolder) {
  const item = create("li", isFolder ? "browse-entry browse-folder" : "browse-entry browse-file");
  item.setAttribute("role", "option");
  item.dataset.key = path;
  item.append(create("span", "browse-name", name));
  if (isFolder) {
    item.setAttribute("aria-label", fill(TEXT.folderLabel, { name }));
  }
  browseRows.set(path, item);
  return item;
}

// Selection changes only touch the rows' state, so large folders stay quick.
export function renderBrowseSelection(mode, selection) {
  const isFileMode = mode === BROWSE_MODES.files;
  const focusKey = selection.focus ?? browseRows.keys().next().value;
  for (const [path, row] of browseRows) {
    row.setAttribute("aria-selected", String(selection.has(path)));
    row.tabIndex = path === focusKey ? 0 : -1;
  }
  const count = selection.size;
  elements.browseConfirm.textContent = isFileMode
    ? fill(count > 0 ? TEXT.addSelectedCount : TEXT.addSelected, { count })
    : TEXT.useThisFolder;
  elements.browseConfirm.disabled = isFileMode && count === 0;
  elements.browseTrayText.textContent =
    count > 0 ? fill(TEXT.selectedCount, { count }) : TEXT.nothingSelected;
  elements.browseClearSelection.hidden = !isFileMode || count === 0;
}

// A folder being typed is never overwritten by a listing that arrives meanwhile.
export function showBrowsePath(path) {
  if (document.activeElement !== elements.browsePath) {
    elements.browsePath.value = path;
  }
}

export function focusBrowseEntry(path) {
  const row = browseRows.get(path) ?? browseRows.values().next().value;
  row?.focus();
  row?.scrollIntoView({ block: "nearest" });
}
