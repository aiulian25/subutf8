import { api } from "./api.js";
import { MODES, SETTINGS_TEXT } from "./constants.js";
import {
  UPDATE_ACTIONS,
  applyTheme,
  dialogElements as elements,
  renderDraft,
  renderSettingsMessage,
  renderUpdateCard,
  renderWatchLog,
} from "./dialogs.js";

// SET-01: the Settings dialog edits a copy of the saved defaults, and Save sends them together.
// The update card acts at once, as in CineSort.

const settings = { context: null, draft: null };

const UPDATE_CALLS = {
  [UPDATE_ACTIONS.check]: api.checkForUpdate,
  [UPDATE_ACTIONS.download]: api.downloadUpdate,
  [UPDATE_ACTIONS.install]: api.installUpdate,
  [UPDATE_ACTIONS.restart]: api.restartAfterUpdate,
};

// `context` connects the dialog to the page: its state, a refresh, error handling, and a
// folder chooser (the system's picker in the desktop window, the Browse view elsewhere).
export function setUpSettingsDialog(context) {
  settings.context = context;
  bindDraftControls();
  bindButtons();
}

function currentApp() {
  return settings.context.getApp();
}

function copyOf(defaults) {
  return { ...defaults, watchFolders: [...defaults.watchFolders] };
}

export function isSettingsOpen() {
  return elements.settingsDialog.open;
}

// Opens the dialog, or keeps what is being edited when it is open; a search result names the
// control to show.
export async function openSettings(focusTargetId) {
  const app = currentApp();
  if (!app) {
    return;
  }
  if (!isSettingsOpen()) {
    settings.draft = copyOf(app.defaults);
    renderSettingsMessage("");
    render();
    elements.settingsDialog.showModal();
  }
  const target = focusTargetId ? document.getElementById(focusTargetId) : null;
  target?.scrollIntoView({ block: "center" });
  target?.focus();
  await refreshWatchLog();
}

// Every refresh while the dialog is open: the update card and the watch log follow the app;
// the values being edited stay as they are.
export function refreshSettings() {
  if (!isSettingsOpen()) {
    return;
  }
  render();
  refreshWatchLog();
}

function render() {
  const app = currentApp();
  renderDraft(settings.draft, app);
  renderUpdateCard(app.update, app.windowOpen);
}

async function refreshWatchLog() {
  if (currentApp()?.mode !== MODES.container || !isSettingsOpen()) {
    return;
  }
  try {
    renderWatchLog(await api.watchLog());
  } catch (error) {
    settings.context.handleFailure(error);
  }
}

function chooseFolder(startPath, onChosen) {
  settings.context.chooseFolder(startPath, (path) => {
    onChosen(path);
    render();
  });
}

function bindDraftControls() {
  const bindings = [
    [elements.defaultDestination, "destination", () => elements.defaultDestination.value],
    [elements.defaultByDay, "organiseByDay", () => elements.defaultByDay.checked],
    [elements.defaultCollision, "collisionPolicy", () => elements.defaultCollision.value],
    [elements.defaultUpdateCheck, "updateCheck", () => elements.defaultUpdateCheck.checked],
    [elements.defaultLanguage, "language", languageValue],
  ];
  for (const [control, field, value] of bindings) {
    control.addEventListener("change", () => {
      settings.draft[field] = value();
    });
  }
  // UI-13: the theme shows at once; Cancel puts the saved one back.
  elements.defaultTheme.addEventListener("change", () => {
    settings.draft.theme = elements.defaultTheme.value;
    applyTheme(settings.draft.theme);
  });
  elements.defaultOutputFolderButton.addEventListener("click", () =>
    chooseFolder(settings.draft.outputFolder, (path) => {
      settings.draft.outputFolder = path;
    }),
  );
  elements.watchAddButton.addEventListener("click", () =>
    chooseFolder(currentApp().browseStart, (path) => {
      if (!settings.draft.watchFolders.includes(path)) {
        settings.draft.watchFolders.push(path);
      }
    }),
  );
  elements.watchFolders.addEventListener("click", (event) => {
    const index = event.target.closest("[data-remove-watch]")?.dataset.removeWatch;
    if (index !== undefined) {
      settings.draft.watchFolders.splice(Number(index), 1);
      render();
    }
  });
}

function languageValue() {
  return elements.defaultLanguage.value.trim() || null;
}

function bindButtons() {
  elements.settingsSave.addEventListener("click", save);
  elements.settingsCancel.addEventListener("click", () => elements.settingsDialog.close());
  elements.settingsRestore.addEventListener("click", restore);
  elements.settingsDialog.addEventListener("close", () => applyTheme(currentApp().defaults.theme));
  elements.updateCard.addEventListener("click", (event) => {
    const action = event.target.closest("[data-update-action]")?.dataset.updateAction;
    if (action) {
      runUpdateAction(action);
    }
  });
}

async function tryCall(call) {
  try {
    await call();
    return true;
  } catch (error) {
    if (!settings.context.handleFailure(error)) {
      renderSettingsMessage(settings.context.errorMessage(error));
    }
    return false;
  }
}

async function save() {
  settings.draft.language = languageValue();
  if (!(await tryCall(() => api.saveDefaults(settings.draft)))) {
    return;
  }
  await settings.context.refresh();
  elements.settingsDialog.close();
}

// SET-03.
async function restore() {
  if (!(await tryCall(() => api.restoreDefaults()))) {
    return;
  }
  await settings.context.refresh();
  settings.draft = copyOf(currentApp().defaults);
  applyTheme(settings.draft.theme);
  render();
  renderSettingsMessage(SETTINGS_TEXT.restored);
}

async function runUpdateAction(action) {
  renderSettingsMessage("");
  await tryCall(UPDATE_CALLS[action]);
  await settings.context.refresh();
}
