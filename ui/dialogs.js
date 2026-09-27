import {
  COLLISION_OPTIONS,
  DESTINATION_OPTIONS,
  LANGUAGE_SUBTAG_SEPARATOR,
  MODES,
  NAME_ONLY_LANGUAGES,
  PACKAGES,
  PERCENT,
  REASONS,
  RELEASES_PAGE,
  ROMANIAN_LANGUAGE,
  ROMANIAN_LETTER_OPTIONS,
  ROMANIAN_LETTERS,
  SEARCH_TEXT,
  SETTINGS_TEXT,
  SHOWN_TIME_LENGTH,
  SHOWN_TIME_SEPARATOR,
  TEXT,
  THEME_OPTIONS,
  TIME_SEPARATOR,
  UPDATE_STEPS,
  UPDATE_TEXT,
} from "./constants.js";
import { create, fileName, fill, fillSelect, problemText, shortPath, statusBadge } from "./view.js";

// The Settings and Search dialogs and the update banner. As on the main page (UI-03), names,
// paths and messages go through textContent, never through markup.

export const dialogElements = {
  updateBanner: document.getElementById("update-banner"),
  updateBannerText: document.getElementById("update-banner-text"),
  updateBannerOpen: document.getElementById("update-banner-open"),
  updateBannerDismiss: document.getElementById("update-banner-dismiss"),
  settingsDialog: document.getElementById("settings-dialog"),
  settingsNotice: document.getElementById("settings-notice"),
  defaultLanguage: document.getElementById("default-language"),
  languageHint: document.getElementById("language-hint"),
  languageList: document.getElementById("language-list"),
  romanianLettersSetting: document.getElementById("romanian-letters-setting"),
  defaultRomanianLetters: document.getElementById("default-romanian-letters"),
  defaultDestination: document.getElementById("default-destination"),
  defaultOutputFolder: document.getElementById("default-output-folder"),
  defaultOutputFolderButton: document.getElementById("default-output-folder-button"),
  defaultByDay: document.getElementById("default-by-day"),
  defaultCollision: document.getElementById("default-collision"),
  defaultTheme: document.getElementById("default-theme"),
  watchSection: document.getElementById("watch-section"),
  watchExplanation: document.getElementById("watch-explanation"),
  watchFolders: document.getElementById("watch-folders"),
  watchAddButton: document.getElementById("watch-add-button"),
  watchStatus: document.getElementById("watch-status"),
  watchLog: document.getElementById("watch-log"),
  updateCard: document.getElementById("update-card"),
  defaultUpdateCheck: document.getElementById("default-update-check"),
  updatePrompt: document.getElementById("update-prompt"),
  updatePromptTitle: document.getElementById("update-prompt-title"),
  updatePromptBody: document.getElementById("update-prompt-body"),
  updatePromptMessage: document.getElementById("update-prompt-message"),
  updatePromptLater: document.getElementById("update-prompt-later"),
  updatePromptAct: document.getElementById("update-prompt-act"),
  settingsDialogMessage: document.getElementById("settings-dialog-message"),
  settingsRestore: document.getElementById("settings-restore"),
  settingsCancel: document.getElementById("settings-cancel"),
  settingsSave: document.getElementById("settings-save"),
  searchDialog: document.getElementById("search-dialog"),
  searchInput: document.getElementById("search-input"),
  searchResults: document.getElementById("search-results"),
};

// Actions the update card offers; settings.js acts on them.
export const UPDATE_ACTIONS = {
  check: "check",
  download: "download",
  install: "install",
  restart: "restart",
};

export function setUpDialogs() {
  const {
    defaultDestination,
    defaultCollision,
    defaultTheme,
    defaultRomanianLetters,
    updateBannerDismiss,
  } = dialogElements;
  fillSelect(defaultDestination, DESTINATION_OPTIONS);
  fillSelect(defaultCollision, COLLISION_OPTIONS);
  fillSelect(defaultTheme, THEME_OPTIONS);
  fillSelect(defaultRomanianLetters, ROMANIAN_LETTER_OPTIONS);
  dialogElements.watchExplanation.textContent = SETTINGS_TEXT.watchExplanation;
  updateBannerDismiss.textContent = UPDATE_TEXT.bannerDismissSymbol;
  updateBannerDismiss.title = UPDATE_TEXT.bannerDismiss;
  updateBannerDismiss.setAttribute("aria-label", UPDATE_TEXT.bannerDismiss);
}

// UI-19: the languages the app hints detection with (ENC-13), in its lower-case form.
const hintedLanguages = new Set();

// UI-19: the suggestions for every subtitle language field: the languages that help
// detection, as the app lists them, then common ones that only name outputs.
export function setUpLanguageList(hinted) {
  for (const tag of hinted) {
    hintedLanguages.add(tag.toLowerCase());
  }
  const suggestions = [
    ...hinted.map((tag) => [tag, TEXT.helpsDetection]),
    ...NAME_ONLY_LANGUAGES.map((tag) => [tag, TEXT.nameOnly]),
  ];
  dialogElements.languageList.replaceChildren(
    ...suggestions.map(([tag, label]) => {
      const option = create("option");
      option.value = tag;
      option.label = label;
      return option;
    }),
  );
}

// UI-19 and ENC-13: as the app decides, a tag helps detection when it, or its language without
// the region, is one the app hints with.
function languageHint(tag) {
  if (!tag) {
    return "";
  }
  const [language] = tag.split(LANGUAGE_SUBTAG_SEPARATOR);
  const helps = [tag, language].some((part) => hintedLanguages.has(part.toLowerCase()));
  return helps ? TEXT.helpsDetection : TEXT.nameOnly;
}

// UI-13: "system" follows the desktop's setting through the stylesheet.
export function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
}

export function showTime(time) {
  return time.slice(0, SHOWN_TIME_LENGTH).replace(TIME_SEPARATOR, SHOWN_TIME_SEPARATOR);
}

// SET-01: the Settings dialog shows the values being edited, not yet saved.
export function renderDraft(draft, app) {
  const elements = dialogElements;
  if (document.activeElement !== elements.defaultLanguage) {
    elements.defaultLanguage.value = draft.language ?? "";
  }
  elements.languageHint.textContent = languageHint(draft.language);
  elements.romanianLettersSetting.hidden = !isRomanianLanguage(draft.language);
  elements.defaultRomanianLetters.value = draft.romanianCommaLetters
    ? ROMANIAN_LETTERS.comma
    : ROMANIAN_LETTERS.asDecoded;
  elements.defaultDestination.value = draft.destination;
  elements.defaultOutputFolder.textContent = shortPath(draft.outputFolder);
  elements.defaultOutputFolder.title = draft.outputFolder;
  elements.defaultByDay.checked = draft.organiseByDay;
  elements.defaultCollision.value = draft.collisionPolicy;
  elements.defaultTheme.value = draft.theme;
  elements.defaultUpdateCheck.checked = draft.updateCheck;
  elements.defaultUpdateCheck.disabled = !app.update.allowed;
  elements.watchSection.hidden = app.mode !== MODES.container;
  renderWatchFolders(draft.watchFolders);
  renderDataProblem(app);
}

// ENC-23: `ro`, with or without a region such as `ro-MD`, in any letter case.
function isRomanianLanguage(tag) {
  const [language] = (tag ?? "").split(LANGUAGE_SUBTAG_SEPARATOR);
  return language.toLowerCase() === ROMANIAN_LANGUAGE;
}

function renderWatchFolders(folders) {
  const items = folders.map((folder, index) => {
    const item = create("li", "watch-folder");
    const path = create("span", "path", folder);
    const remove = create("button", "quiet-button remove-button", TEXT.removeSymbol);
    remove.type = "button";
    remove.dataset.removeWatch = String(index);
    remove.setAttribute("aria-label", fill(SETTINGS_TEXT.removeWatchFolder, { path: folder }));
    item.append(path, remove);
    return item;
  });
  if (items.length === 0) {
    items.push(create("li", "muted", SETTINGS_TEXT.watchEmpty));
  }
  dialogElements.watchFolders.replaceChildren(...items);
}

// SET-02: why settings or the history are not kept; Docker says what to mount.
function renderDataProblem(app) {
  const problem = app.dataProblem;
  const needsMount = app.mode === MODES.container && problem?.reason === REASONS.dataFolderReadOnly;
  const text = [problemText(problem), needsMount ? SETTINGS_TEXT.dockerDataHint : ""]
    .filter(Boolean)
    .join(" ");
  dialogElements.settingsNotice.textContent = text;
  dialogElements.settingsNotice.hidden = !text;
}

export function renderSettingsMessage(message) {
  dialogElements.settingsDialogMessage.textContent = message;
  dialogElements.settingsDialogMessage.hidden = !message;
}

// WATCH-04: newest first.
export function renderWatchLog(log) {
  const notes = [fill(SETTINGS_TEXT.watchInterval, { seconds: log.intervalSeconds })];
  if (log.unavailableFolders.length > 0) {
    notes.push(
      fill(SETTINGS_TEXT.watchUnavailable, {
        folders: log.unavailableFolders.join(SETTINGS_TEXT.listSeparator),
      }),
    );
  }
  if (log.outputFolderUnavailable) {
    notes.push(SETTINGS_TEXT.watchOutputUnavailable);
  }
  dialogElements.watchStatus.textContent = notes.join(" ");
  const items = log.entries.map((entry) => {
    const item = create("li", "watch-entry");
    const heading = create("div", "watch-entry-heading");
    heading.append(
      create("span", "muted", showTime(entry.time)),
      create("span", "file-name", entry.name),
      statusBadge(entry.status),
    );
    item.append(heading);
    const detail = entry.problem ? problemText(entry.problem) : fileName(entry.output ?? "");
    if (detail) {
      item.append(create("div", "file-reason", detail));
    }
    return item;
  });
  if (items.length === 0) {
    items.push(create("li", "muted", SETTINGS_TEXT.watchNoActivity));
  }
  dialogElements.watchLog.replaceChildren(...items);
}

function actionButton(label, action, isPrimary) {
  const button = create("button", isPrimary ? "primary-button" : "", label);
  button.type = "button";
  button.dataset.updateAction = action;
  return button;
}

// A new tab in a browser; the desktop window hands SubUTF8's GitHub pages to the system's
// browser.
function releaseLink(address, label) {
  const link = create("a", "update-link", label);
  link.href = address;
  link.target = "_blank";
  link.rel = "noopener noreferrer";
  return link;
}

function statusChip(update) {
  if (update.checkFailed) {
    return create("span", "update-chip is-warning", UPDATE_TEXT.couldNotCheck);
  }
  if (update.checked) {
    return create("span", "update-chip is-ok", UPDATE_TEXT.upToDate);
  }
  return create("span", "update-chip", UPDATE_TEXT.notChecked);
}

function scheduleText(update, isAutomatic) {
  if (!update.allowed) {
    return problemText({ reason: REASONS.updateCheckOff });
  }
  return isAutomatic ? UPDATE_TEXT.automaticCheck : UPDATE_TEXT.automaticCheckOff;
}

// As in CineSort: the state, the version, the daily check, Check for updates and the releases.
function currentVersionRow(update, isAutomatic, check) {
  const row = create("div", "update-row");
  if (update.allowed) {
    row.append(statusChip(update));
  }
  row.append(
    create("span", "update-version", fill(UPDATE_TEXT.version, update)),
    create("span", "update-note update-spacer", scheduleText(update, isAutomatic)),
  );
  if (update.allowed) {
    const label = check.isChecking ? UPDATE_TEXT.checking : UPDATE_TEXT.checkNow;
    const button = actionButton(label, UPDATE_ACTIONS.check, false);
    button.disabled = check.isChecking;
    row.append(button);
  }
  row.append(releaseLink(RELEASES_PAGE, UPDATE_TEXT.releases));
  return row;
}

// UPDATE-01: what a check someone asked for found.
function checkNote(check) {
  if (!check.note) {
    return [];
  }
  return [create("p", check.isWarning ? "update-note is-warning" : "update-note", check.note)];
}

function newVersionHeading(update) {
  const heading = create("div", "update-row");
  heading.append(
    create("span", "update-chip is-new", UPDATE_TEXT.newChip),
    create("span", "update-version", fill(UPDATE_TEXT.available, update)),
  );
  const detail = create("p", "update-note", fill(UPDATE_TEXT.currentVersion, update));
  detail.append(releaseLink(update.releasePage ?? RELEASES_PAGE, UPDATE_TEXT.releaseNotes));
  return [heading, detail];
}

// UPDATE-04: what installing by hand takes when there is no pkexec.
function installYourself(update) {
  const template =
    update.package === PACKAGES.rpm
      ? UPDATE_TEXT.installYourselfRpm
      : UPDATE_TEXT.installYourselfDeb;
  return fill(template, { path: update.downloaded });
}

// The next step, as in CineSort: download, install, restart; Docker is told the command.
function nextStep(update) {
  const isAppImage = update.package === PACKAGES.appimage;
  switch (update.step) {
    case UPDATE_STEPS.downloading: {
      const bar = create("progress");
      bar.max = Math.max(update.total, 1);
      bar.value = update.received;
      const percent = Math.floor((update.received * PERCENT) / Math.max(update.total, 1));
      return [bar, create("p", "muted", fill(UPDATE_TEXT.downloading, { percent }))];
    }
    case UPDATE_STEPS.installing:
      return [create("p", "muted", isAppImage ? UPDATE_TEXT.replacing : UPDATE_TEXT.installing)];
    case UPDATE_STEPS.installed:
      return [
        create("p", "", UPDATE_TEXT.installed),
        actionButton(UPDATE_TEXT.restart, UPDATE_ACTIONS.restart, true),
      ];
    default:
      return idleStep(update, isAppImage);
  }
}

function idleStep(update, isAppImage) {
  if (update.downloaded) {
    const parts = [create("p", "path", fill(UPDATE_TEXT.downloaded, { path: update.downloaded }))];
    if (update.problem?.reason === REASONS.noPrivilegeProgram) {
      parts.push(create("p", "path", installYourself(update)));
      return parts;
    }
    const label = isAppImage ? UPDATE_TEXT.replaceAppImage : UPDATE_TEXT.install;
    parts.push(actionButton(label, UPDATE_ACTIONS.install, true));
    return parts;
  }
  if (!update.latest) {
    return [];
  }
  if (update.package === PACKAGES.docker) {
    return [
      create("p", "", UPDATE_TEXT.docker),
      create("code", "command", UPDATE_TEXT.dockerCommand),
    ];
  }
  if (update.canInstall) {
    return [actionButton(UPDATE_TEXT.download, UPDATE_ACTIONS.download, true)];
  }
  return [create("p", "", UPDATE_TEXT.getFromGithub)];
}

// UPDATE-01 to UPDATE-05, as CineSort shows them. `check` is the Check for updates button's
// state and what it found.
export function renderUpdateCard(update, isAutomatic, check) {
  const card = dialogElements.updateCard;
  const isNew = update.allowed && Boolean(update.latest);
  card.classList.toggle("is-new", isNew);
  if (!isNew) {
    card.replaceChildren(currentVersionRow(update, isAutomatic, check), ...checkNote(check));
    return;
  }
  const problem = update.problem ? [create("p", "problem", problemText(update.problem))] : [];
  card.replaceChildren(...newVersionHeading(update), ...problem, ...nextStep(update));
}

// What the prompt after a download says and offers at each step.
function promptStep(update) {
  const isAppImage = update.package === PACKAGES.appimage;
  if (update.step === UPDATE_STEPS.installing) {
    return {
      title: fill(UPDATE_TEXT.installingTitle, update),
      lines: [isAppImage ? UPDATE_TEXT.replacing : UPDATE_TEXT.installing],
      later: UPDATE_TEXT.hide,
    };
  }
  if (update.step === UPDATE_STEPS.installed) {
    return {
      title: fill(UPDATE_TEXT.installedTitle, update),
      lines: [UPDATE_TEXT.restartBody, UPDATE_TEXT.untouched],
      action: UPDATE_ACTIONS.restart,
      actionLabel: UPDATE_TEXT.restart,
      later: UPDATE_TEXT.restartLater,
    };
  }
  if (update.problem?.reason === REASONS.noPrivilegeProgram) {
    return {
      title: fill(UPDATE_TEXT.manualTitle, update),
      lines: [installYourself(update)],
      later: UPDATE_TEXT.close,
    };
  }
  return {
    title: fill(UPDATE_TEXT.readyTitle, update),
    lines: [
      UPDATE_TEXT.readyChecked,
      isAppImage ? UPDATE_TEXT.readyAppImage : UPDATE_TEXT.readyPackage,
      UPDATE_TEXT.untouched,
    ],
    problem: update.problem ? problemText(update.problem) : "",
    action: UPDATE_ACTIONS.install,
    actionLabel: isAppImage ? UPDATE_TEXT.replaceAppImage : UPDATE_TEXT.install,
    later: UPDATE_TEXT.later,
  };
}

// UPDATE-04 and UPDATE-05: after a download, the page asks to install it and then to restart,
// as CineSort's prompt does.
export function renderUpdatePrompt(update) {
  const step = promptStep(update);
  const elements = dialogElements;
  elements.updatePromptTitle.textContent = step.title;
  const lines = step.lines.map((line) => create("p", "", line));
  const problem = step.problem ? [create("p", "problem", step.problem)] : [];
  elements.updatePromptBody.replaceChildren(...lines, ...problem);
  elements.updatePromptLater.textContent = step.later;
  elements.updatePromptAct.hidden = !step.action;
  elements.updatePromptAct.textContent = step.actionLabel ?? "";
  elements.updatePromptAct.dataset.updateAction = step.action ?? "";
}

export function renderUpdatePromptMessage(message) {
  dialogElements.updatePromptMessage.textContent = message;
  dialogElements.updatePromptMessage.hidden = !message;
}

export function renderUpdateBanner(update, isDismissed) {
  const { updateBanner, updateBannerText, updateBannerOpen } = dialogElements;
  updateBanner.hidden = !update.latest || isDismissed;
  if (updateBanner.hidden) {
    return;
  }
  updateBannerText.textContent = fill(UPDATE_TEXT.banner, update);
  updateBannerOpen.textContent =
    update.package === PACKAGES.docker ? UPDATE_TEXT.bannerOpenDocker : UPDATE_TEXT.bannerOpen;
}

// HIST-02: groups of results; those with a key can be opened.
export function renderSearchResults(groups, isSearching) {
  const sections = groups
    .filter((group) => group.results.length > 0)
    .map((group) => {
      const section = create("section", "search-group");
      section.append(create("h3", "", group.title));
      const list = create("ul", "plain-list");
      list.append(...group.results.map(searchResult));
      section.append(list);
      return section;
    });
  if (sections.length === 0) {
    const message = isSearching ? SEARCH_TEXT.searching : SEARCH_TEXT.noResults;
    sections.push(create("p", "muted", message));
  }
  dialogElements.searchResults.replaceChildren(...sections);
}

function searchResult(result) {
  const item = create("li");
  const body = create(result.key ? "button" : "div", "search-result");
  if (result.key) {
    body.type = "button";
    body.dataset.result = result.key;
  }
  const title = create("span", "search-title", result.title);
  if (result.tag) {
    title.append(create("span", "search-tag", result.tag));
  }
  body.append(title);
  if (result.detail) {
    body.append(create("span", "search-detail", result.detail));
  }
  item.append(body);
  return item;
}

export function searchResultButtons() {
  return [...dialogElements.searchResults.querySelectorAll("button.search-result")];
}
