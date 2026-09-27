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

async function request(method, path, body) {
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
  const data = parseJson(await response.text());
  if (!response.ok) {
    throw new ApiError(response.status, data);
  }
  return data;
}

function fileRoute(id, action) {
  return `${ROUTES.files}/${encodeURIComponent(id)}/${action}`;
}

export const api = {
  state: () => request(HTTP_METHODS.get, ROUTES.state),
  encodings: () => request(HTTP_METHODS.get, ROUTES.encodings),
  browse: (path) => request(HTTP_METHODS.post, ROUTES.browse, { path }),
  add: (paths, includeSubfolders) =>
    request(HTTP_METHODS.post, ROUTES.add, { paths, includeSubfolders }),
  upload: (file) => {
    const query = new URLSearchParams({ [UPLOAD_NAME_PARAMETER]: file.name });
    return request(HTTP_METHODS.post, `${ROUTES.upload}?${query}`, file);
  },
  preview: (id) => request(HTTP_METHODS.get, fileRoute(id, FILE_ACTIONS.preview)),
  chooseEncoding: (id, encoding) =>
    request(HTTP_METHODS.post, fileRoute(id, FILE_ACTIONS.encoding), { encoding }),
  remove: (id) => request(HTTP_METHODS.post, fileRoute(id, FILE_ACTIONS.remove)),
  clear: () => request(HTTP_METHODS.post, ROUTES.clear),
  saveSettings: (settings) => request(HTTP_METHODS.post, ROUTES.settings, settings),
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
