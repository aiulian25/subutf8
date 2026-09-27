import { api } from "./api.js";
import { MODES, ROMANIAN_LETTERS, SETTINGS_TEXT, UPDATE_STEPS, UPDATE_TEXT } from "./constants.js";
import {
  UPDATE_ACTIONS,
  applyTheme,
  dialogElements as elements,
  renderDraft,
  renderSettingsMessage,
  renderUpdateCard,
  renderUpdatePrompt,
  renderUpdatePromptMessage,
  renderWatchLog,
} from "./dialogs.js";

// SET-01: the Settings dialog edits a copy of the saved defaults, and Save sends them together.
// The update card and the prompt after a download act at once, as in CineSort.

const NO_CHECK = { isChecking: false, note: "", isWarning: false };

// UPDATE-04: the steps that open the prompt by themselves, once each per download.
const PROMPTING_STEPS = [UPDATE_STEPS.idle, UPDATE_STEPS.installed];

const settings = { context: null, draft: null, check: NO_CHECK, promptKey: null };

const UPDATE_CALLS = {
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
    settings.check = NO_CHECK;
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
  renderUpdateCard(app.update, app.defaults.updateCheck, settings.check);
}

// UPDATE-04 and UPDATE-05: once a download is checked, the page asks whether to install it, and
// once it is installed, whether to restart, also when Settings is closed. Later closes the
// prompt; the Settings card keeps the same steps.
export function refreshUpdatePrompt() {
  const { update } = currentApp();
  const { updatePrompt, updatePromptAct } = elements;
  const isPromptingStep = Boolean(update.downloaded) && PROMPTING_STEPS.includes(update.step);
  const key = isPromptingStep ? JSON.stringify([update.downloaded, update.step]) : null;
  const isNewStep = key !== null && key !== settings.promptKey;
  if (!isNewStep && !updatePrompt.open) {
    return;
  }
  renderUpdatePrompt(update);
  if (!isNewStep) {
    return;
  }
  settings.promptKey = key;
  if (!updatePrompt.open) {
    renderUpdatePromptMessage("");
    updatePrompt.showModal();
  }
  updatePromptAct.focus();
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
    [
      elements.defaultRomanianLetters,
      "romanianCommaLetters",
      () => elements.defaultRomanianLetters.value === ROMANIAN_LETTERS.comma,
    ],
  ];
  for (const [control, field, value] of bindings) {
    control.addEventListener("change", () => {
      settings.draft[field] = value();
    });
  }
  // ENC-23: the Romanian letters show as soon as the language typed is Romanian.
  elements.defaultLanguage.addEventListener("input", () => {
    settings.draft.language = languageValue();
    render();
  });
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
  elements.updatePromptLater.addEventListener("click", () => elements.updatePrompt.close());
  elements.updatePromptAct.addEventListener("click", () =>
    runUpdateAction(elements.updatePromptAct.dataset.updateAction, renderUpdatePromptMessage),
  );
}

async function tryCall(call, showMessage = renderSettingsMessage) {
  try {
    await call();
    return true;
  } catch (error) {
    if (!settings.context.handleFailure(error)) {
      showMessage(settings.context.errorMessage(error));
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

// `showMessage` puts a failure where the step was asked for: in Settings or in the prompt.
async function runUpdateAction(action, showMessage = renderSettingsMessage) {
  showMessage("");
  if (action === UPDATE_ACTIONS.check) {
    await checkForUpdate();
    return;
  }
  await tryCall(UPDATE_CALLS[action], showMessage);
  await settings.context.refresh();
}

// UPDATE-01: as in CineSort, a check someone asked for says what it found, so a click never
// looks like nothing happened.
async function checkForUpdate() {
  settings.check = { ...NO_CHECK, isChecking: true };
  render();
  try {
    settings.check = checkOutcome(await api.checkForUpdate());
  } catch (error) {
    if (settings.context.handleFailure(error)) {
      return;
    }
    settings.check = { ...NO_CHECK, note: settings.context.errorMessage(error), isWarning: true };
  }
  await settings.context.refresh();
}

// A newer version needs no note: the card shows it.
function checkOutcome(update) {
  if (update.checkFailed) {
    return { ...NO_CHECK, note: UPDATE_TEXT.checkFailed, isWarning: true };
  }
  return { ...NO_CHECK, note: update.latest ? "" : UPDATE_TEXT.checkedLatest };
}
