import { api } from "./api.js";
import {
  DATE_PART_LENGTH,
  DATE_SEPARATOR,
  DATE_WORDS,
  KEYS,
  MODES,
  MONTH_OFFSET,
  SEARCH_DELAY_MILLISECONDS,
  SEARCH_TEXT,
  SETTING_ENTRIES,
  STATUS_LABELS,
  WORD_SEPARATOR,
  YEAR_LENGTH,
} from "./constants.js";
import {
  dialogElements as elements,
  renderSearchResults,
  searchResultButtons,
  showTime,
} from "./dialogs.js";
import { fill } from "./view.js";

// HIST-02: one box finds settings, files in the list and files converted before. Settings
// and the list are searched in the page; the history is searched by the app.

const RESULT_KINDS = { setting: "setting", file: "file" };
const KEY_SEPARATOR = ":";

const search = { context: null, timer: null, generation: 0 };

// `context` gives the page's state and opens what a result points to.
export function setUpSearch(context) {
  search.context = context;
  elements.searchInput.addEventListener("input", () => {
    clearTimeout(search.timer);
    search.timer = setTimeout(runSearch, SEARCH_DELAY_MILLISECONDS);
  });
  elements.searchInput.addEventListener("keydown", (event) => {
    // A search field would only clear its text on the first Escape; one press closes.
    if (event.key === KEYS.escape) {
      event.preventDefault();
      elements.searchDialog.close();
      return;
    }
    const [first] = searchResultButtons();
    if (event.key === KEYS.arrowDown && first) {
      event.preventDefault();
      first.focus();
    }
    if (event.key === KEYS.enter && first) {
      event.preventDefault();
      activate(first.dataset.result);
    }
  });
  elements.searchResults.addEventListener("click", (event) => {
    const key = event.target.closest("[data-result]")?.dataset.result;
    if (key) {
      activate(key);
    }
  });
  elements.searchResults.addEventListener("keydown", moveBetweenResults);
}

export function openSearch() {
  if (elements.searchDialog.open) {
    return;
  }
  elements.searchInput.value = "";
  elements.searchDialog.showModal();
  elements.searchInput.focus();
  runSearch();
}

function moveBetweenResults(event) {
  const buttons = searchResultButtons();
  const index = buttons.indexOf(document.activeElement);
  const step = { [KEYS.arrowDown]: 1, [KEYS.arrowUp]: -1 }[event.key];
  if (index < 0 || step === undefined) {
    return;
  }
  event.preventDefault();
  const next = buttons[index + step];
  if (next) {
    next.focus();
    return;
  }
  if (step < 0) {
    elements.searchInput.focus();
  }
}

function wordsOf(text) {
  return text.toLowerCase().split(WORD_SEPARATOR).filter(Boolean);
}

function matchesAll(text, words) {
  const haystack = text.toLowerCase();
  return words.every((word) => haystack.includes(word));
}

// HIST-02: "today" and "yesterday" become dates, as the history writes them.
function withDates(query) {
  return query
    .split(WORD_SEPARATOR)
    .map((word) => {
      const daysAgo = DATE_WORDS[word.toLowerCase()];
      return daysAgo === undefined ? word : localDate(daysAgo);
    })
    .join(" ");
}

function localDate(daysAgo) {
  const now = new Date();
  const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysAgo);
  const pad = (number, length) => String(number).padStart(length, "0");
  return [
    pad(date.getFullYear(), YEAR_LENGTH),
    pad(date.getMonth() + MONTH_OFFSET, DATE_PART_LENGTH),
    pad(date.getDate(), DATE_PART_LENGTH),
  ].join(DATE_SEPARATOR);
}

function settingResults(app, words) {
  return SETTING_ENTRIES.filter((entry) => !entry.containerOnly || app.mode === MODES.container)
    .filter((entry) => matchesAll(`${entry.label} ${entry.words}`, words))
    .map((entry) => ({
      key: [RESULT_KINDS.setting, entry.target].join(KEY_SEPARATOR),
      title: entry.label,
    }));
}

function fileResults(app, words) {
  return app.files
    .filter((file) => {
      const text = [file.name, file.folder, STATUS_LABELS[file.status], file.encoding, file.output];
      return matchesAll(text.filter(Boolean).join(" "), words);
    })
    .map((file) => ({
      key: [RESULT_KINDS.file, file.id].join(KEY_SEPARATOR),
      title: file.name,
      detail: fill(SEARCH_TEXT.fileDetail, {
        status: STATUS_LABELS[file.status],
        folder: file.folder ?? "",
      }),
    }));
}

function historyResults(records) {
  return records.map((record) => ({
    title: record.name,
    tag: record.watched ? SEARCH_TEXT.watched : "",
    detail: fill(SEARCH_TEXT.historyDetail, {
      time: showTime(record.time),
      encoding: record.encoding,
      output: record.output,
    }),
  }));
}

// Only the latest search is shown, so a slow answer never replaces a newer one.
async function runSearch() {
  const app = search.context.getApp();
  if (!app) {
    return;
  }
  const query = withDates(elements.searchInput.value.trim());
  const words = wordsOf(query);
  const groups = [
    { title: SEARCH_TEXT.settingsGroup, results: settingResults(app, words) },
    { title: SEARCH_TEXT.filesGroup, results: fileResults(app, words) },
  ];
  search.generation += 1;
  const generation = search.generation;
  renderSearchResults(groups, true);
  let records = [];
  try {
    records = await api.history(query);
  } catch (error) {
    if (search.context.handleFailure(error)) {
      elements.searchDialog.close();
      return;
    }
  }
  if (generation !== search.generation) {
    return;
  }
  groups.push({ title: SEARCH_TEXT.historyGroup, results: historyResults(records) });
  renderSearchResults(groups, false);
}

function activate(key) {
  const separator = key.indexOf(KEY_SEPARATOR);
  const kind = key.slice(0, separator);
  const value = key.slice(separator + KEY_SEPARATOR.length);
  elements.searchDialog.close();
  if (kind === RESULT_KINDS.setting) {
    search.context.openSettings(value);
    return;
  }
  search.context.selectFile(Number(value));
}
