import {
  BINARY_CONTENT_TYPE,
  CONTENT_TYPE_HEADER,
  FILE_ACTIONS,
  HTTP_METHODS,
  JSON_CONTENT_TYPE,
  NETWORK_FAILURE_STATUS,
  ROUTES,
  TOKEN_HEADER,
  TOKEN_PATTERN,
  TOKEN_STORAGE_KEY,
  UPLOAD_NAME_PARAMETER,
} from "./constants.js";

export class ApiError extends Error {
  constructor(status, problem) {
    super(problem?.reason ?? String(status));
    this.status = status;
    this.problem = problem;
  }
}

let token = "";

function readStoredToken() {
  try {
    return sessionStorage.getItem(TOKEN_STORAGE_KEY);
  } catch {
    return null;
  }
}

function storeToken(value) {
  try {
    sessionStorage.setItem(TOKEN_STORAGE_KEY, value);
  } catch {
    // The token then lives only in this page, which is enough until it reloads.
  }
}

// ACCESS-03: the desktop app opens this page with its token after "#"; the page keeps it for
// the tab and clears it from the address bar. Docker needs no token.
export function connect() {
  const match = window.location.hash.match(TOKEN_PATTERN);
  if (!match) {
    token = readStoredToken() ?? "";
    return;
  }
  token = decodeURIComponent(match[1]);
  storeToken(token);
  history.replaceState(null, "", window.location.pathname);
}

function parseJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

// Every request carries the app's header with the token (ACCESS-04); a refusal carries the
// reason as JSON.
async function send(method, path, body) {
  const headers = { [TOKEN_HEADER]: token };
  let payload;
  if (body instanceof Blob) {
    headers[CONTENT_TYPE_HEADER] = BINARY_CONTENT_TYPE;
    payload = body;
  } else if (body !== undefined) {
    headers[CONTENT_TYPE_HEADER] = JSON_CONTENT_TYPE;
    payload = JSON.stringify(body);
  }
  let response;
  try {
    response = await fetch(path, { method, headers, body: payload, cache: "no-store" });
  } catch {
    throw new ApiError(NETWORK_FAILURE_STATUS, null);
  }
  if (!response.ok) {
    throw new ApiError(response.status, parseJson(await response.text()));
  }
  return response;
}

async function request(method, path, body) {
  const response = await send(method, path, body);
  return parseJson(await response.text());
}

function fileRoute(id, action) {
  return `${ROUTES.files}/${encodeURIComponent(id)}/${action}`;
}

export const api = {
  state: () => request(HTTP_METHODS.get, ROUTES.state),
  encodings: () => request(HTTP_METHODS.get, ROUTES.encodings),
  languages: () => request(HTTP_METHODS.get, ROUTES.languages),
  browse: (path) => request(HTTP_METHODS.post, ROUTES.browse, { path }),
  add: (paths, includeSubfolders) =>
    request(HTTP_METHODS.post, ROUTES.add, { paths, includeSubfolders }),
  upload: (file) => {
    const query = new URLSearchParams({ [UPLOAD_NAME_PARAMETER]: file.name });
    return request(HTTP_METHODS.post, `${ROUTES.upload}?${query}`, file);
  },
  preview: (id) => request(HTTP_METHODS.get, fileRoute(id, FILE_ACTIONS.preview)),
  candidates: (id) => request(HTTP_METHODS.get, fileRoute(id, FILE_ACTIONS.candidates)),
  chooseEncoding: (id, encoding, keepUtf8Lines = false) =>
    request(HTTP_METHODS.post, fileRoute(id, FILE_ACTIONS.encoding), { encoding, keepUtf8Lines }),
  setLanguage: (id, language) =>
    request(HTTP_METHODS.post, fileRoute(id, FILE_ACTIONS.language), { language }),
  repair: (id, repair) =>
    request(HTTP_METHODS.post, fileRoute(id, FILE_ACTIONS.repair), { repair }),
  // UI-17: a plain link could not carry the token, so the page fetches the file itself.
  output: async (id) => (await send(HTTP_METHODS.get, fileRoute(id, FILE_ACTIONS.output))).blob(),
  remove: (id) => request(HTTP_METHODS.post, fileRoute(id, FILE_ACTIONS.remove)),
  clear: () => request(HTTP_METHODS.post, ROUTES.clear),
  convert: () => request(HTTP_METHODS.post, ROUTES.convert),
  cancel: () => request(HTTP_METHODS.post, ROUTES.cancel),
  quit: () => request(HTTP_METHODS.post, ROUTES.quit),
  saveDefaults: (defaults) => request(HTTP_METHODS.post, ROUTES.defaults, defaults),
  restoreDefaults: () => request(HTTP_METHODS.post, ROUTES.restoreDefaults),
  history: (query) => request(HTTP_METHODS.post, ROUTES.history, { query }),
  watchLog: () => request(HTTP_METHODS.get, ROUTES.watchLog),
  checkForUpdate: () => request(HTTP_METHODS.post, ROUTES.updateCheck),
  downloadUpdate: () => request(HTTP_METHODS.post, ROUTES.updateDownload),
  installUpdate: () => request(HTTP_METHODS.post, ROUTES.updateInstall),
  restartAfterUpdate: () => request(HTTP_METHODS.post, ROUTES.updateRestart),
};
