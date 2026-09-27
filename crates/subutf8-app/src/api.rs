use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as RoutePath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use subutf8_core::batch::Origin;
use subutf8_core::encoding_catalog::{find_manual_choice, manual_choices};
use subutf8_core::input_scan::{AllowedArea, SkipReason};
use subutf8_core::language::SubtitleLanguage;
use subutf8_core::output_naming::{dropped_file_name, split_srt_name};

use crate::constants::{
    MAXIMUM_QUERY_CHARACTERS, MAXIMUM_WATCH_FOLDERS, MINIMUM_MANUAL_CHECK_INTERVAL,
};
use crate::defaults::Defaults;
use crate::folders::list_folder;
use crate::history::HistoryRecord;
use crate::instance;
use crate::server::{AppState, lock};
use crate::session::{
    SessionProblem, SessionSettings, detect_again, gather, origin_bytes, prepare, preview_file,
};
use crate::settings::Mode;
use crate::update::{self, Step, UpdateProblem};
use crate::views::{
    AddedView, DestinationName, HistoryEntryView, ListingView, PreviewView, Problem, Reason,
    SettingsView, StateExtras, StateView, UpdateView, WatchLogView, added_view, data_problem,
    gathered_view, history_view, listing_view, manual_choice_problem, preview_view,
    read_problem_description, skip_reason_problem, state_view, store_problem, update_view,
    watch_log_view,
};

/// An error answer: the HTTP status and the reason the interface shows.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    problem: Problem,
}

impl ApiError {
    fn new(status: StatusCode, reason: Reason) -> Self {
        Self {
            status,
            problem: Problem::of(reason),
        }
    }

    fn refused(reason: SkipReason) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            problem: skip_reason_problem(reason),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.problem)).into_response()
    }
}

impl From<SessionProblem> for ApiError {
    fn from(problem: SessionProblem) -> Self {
        match problem {
            SessionProblem::UnknownFile => Self::new(StatusCode::NOT_FOUND, Reason::UnknownFile),
            SessionProblem::ConversionRunning => {
                Self::new(StatusCode::CONFLICT, Reason::ConversionRunning)
            }
            SessionProblem::TooMuchDropped => {
                Self::new(StatusCode::PAYLOAD_TOO_LARGE, Reason::TooMuchDropped)
            }
            SessionProblem::ListFull => Self::new(StatusCode::CONFLICT, Reason::ListFull),
            SessionProblem::ManualChoice(refusal) => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                problem: manual_choice_problem(refusal),
            },
        }
    }
}

/// Runs file-system work off the server's threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, Reason::Internal))
}

pub async fn state(State(app): State<AppState>) -> Json<StateView> {
    let window_open = app
        .interface
        .as_ref()
        .is_some_and(|interface| interface.is_window_open());
    let update = current_update_view(&app);
    let history_problem = app.history().problem;
    let (defaults, data_problem) = {
        let store = app.defaults();
        let problem = data_problem(&store, history_problem, app.data_folder_writable);
        (store.current.clone(), problem)
    };
    let extras = StateExtras {
        window_open,
        defaults,
        data_problem,
        update,
    };
    Json(state_view(&app.settings, &app.session(), extras))
}

fn current_update_view(app: &AppState) -> UpdateView {
    update_view(&app.update(), app.settings.update_check_allowed)
}

/// UI-09.
pub async fn encodings() -> Json<Vec<&'static str>> {
    Json(
        manual_choices()
            .iter()
            .map(|encoding| encoding.name())
            .collect(),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseRequest {
    path: Option<PathBuf>,
}

/// SAFE-13: only folders inside the allowed area can be listed.
pub async fn browse(
    State(app): State<AppState>,
    Json(request): Json<BrowseRequest>,
) -> Result<Json<ListingView>, ApiError> {
    let path = request
        .path
        .unwrap_or_else(|| app.settings.browse_start.clone());
    let area = app.settings.allowed_area.clone();
    let listing = blocking(move || list_folder(&path, &area))
        .await?
        .map_err(ApiError::refused)?;
    Ok(Json(listing_view(listing)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddRequest {
    paths: Vec<PathBuf>,
    #[serde(default)]
    include_subfolders: bool,
}

pub async fn add(
    State(app): State<AppState>,
    Json(request): Json<AddRequest>,
) -> Result<Json<AddedView>, ApiError> {
    let added = add_paths(&app, request.paths, request.include_subfolders).await?;
    Ok(Json(added))
}

/// Reads and classifies outside the session lock, so the interface stays responsive.
pub async fn add_paths(
    app: &AppState,
    paths: Vec<PathBuf>,
    include_subfolders: bool,
) -> Result<AddedView, ApiError> {
    let (room, language) = {
        let session = app.session();
        (session.room_left(), session.settings.language.clone())
    };
    let area = app.settings.allowed_area.clone();
    let mut gathered =
        blocking(move || gather(&paths, include_subfolders, &area, room, language.as_ref()))
            .await?;
    let added = app.session().add(std::mem::take(&mut gathered.prepared));
    Ok(gathered_view(added, gathered))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRequest {
    paths: Vec<PathBuf>,
}

/// ACCESS-08: a second launch hands its files over and asks for the interface to show.
pub async fn open(
    State(app): State<AppState>,
    Json(request): Json<OpenRequest>,
) -> Result<Json<AddedView>, ApiError> {
    let Some(interface) = app.interface.clone() else {
        return Err(ApiError::new(StatusCode::FORBIDDEN, Reason::NotAvailable));
    };
    let added = add_paths(&app, request.paths, false).await?;
    interface.show();
    Ok(Json(added))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadQuery {
    name: String,
}

/// SAFE-15, NAME-01, NAME-07, LIMIT-01 and LIMIT-02: a dropped file stays in memory.
pub async fn upload(
    State(app): State<AppState>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<AddedView>, ApiError> {
    let name = dropped_file_name(&query.name)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, Reason::UnusableDroppedName))?
        .to_owned();
    if split_srt_name(&name).is_none() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, Reason::NotSrt));
    }
    let size = body.len();
    let language = {
        let session = app.session();
        session.check_dropped_room(size)?;
        session.settings.language.clone()
    };
    let origin = Origin::Dropped {
        name,
        bytes: Arc::from(body.as_ref()),
    };
    let prepared = blocking(move || prepare(origin, language.as_ref())).await?;
    let mut session = app.session();
    session.check_dropped_room(size)?;
    let added = session.add(vec![prepared]);
    Ok(Json(added_view(added, Vec::new(), false)))
}

/// UI-04.
pub async fn preview(
    State(app): State<AppState>,
    RoutePath(id): RoutePath<u64>,
) -> Result<Json<PreviewView>, ApiError> {
    let (origin, encoding) = {
        let session = app.session();
        let file = session.find(id)?;
        (file.origin.clone(), file.current_encoding())
    };
    let Some(encoding) = encoding else {
        return Ok(Json(preview_view(None, Ok(Vec::new()))));
    };
    let preview = blocking(move || preview_file(&origin, encoding)).await?;
    Ok(Json(preview_view(Some(encoding), preview)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncodingRequest {
    encoding: String,
}

/// ENC-12: only the exact name of an offered encoding is accepted.
pub async fn choose_encoding(
    State(app): State<AppState>,
    RoutePath(id): RoutePath<u64>,
    Json(request): Json<EncodingRequest>,
) -> Result<StatusCode, ApiError> {
    let encoding = find_manual_choice(&request.encoding)
        .ok_or_else(|| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, Reason::NotOffered))?;
    let origin = app.session().find(id)?.origin.clone();
    let bytes = blocking(move || origin_bytes(&origin))
        .await?
        .map_err(|problem| {
            let (_, problem) = read_problem_description(problem);
            ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                problem,
            }
        })?;
    app.session().choose_encoding(id, encoding, &bytes)?;
    Ok(StatusCode::NO_CONTENT)
}

/// UI-06.
pub async fn remove(
    State(app): State<AppState>,
    RoutePath(id): RoutePath<u64>,
) -> Result<StatusCode, ApiError> {
    app.session().remove(id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear(State(app): State<AppState>) -> Result<StatusCode, ApiError> {
    app.session().clear()?;
    Ok(StatusCode::NO_CONTENT)
}

/// UI-05, NAME-05 and SAFE-13: the language tag is validated before any file is touched,
/// and a new output folder must lie inside the allowed area.
pub async fn change_settings(
    State(app): State<AppState>,
    Json(request): Json<SettingsView>,
) -> Result<StatusCode, ApiError> {
    let language = parsed_language(request.language.as_deref())?;
    let current_folder = app.session().settings.output_folder.clone();
    let requested_folder = PathBuf::from(&request.output_folder);
    let output_folder = if requested_folder == current_folder {
        current_folder
    } else {
        let area = app.settings.allowed_area.clone();
        blocking(move || real_folder(&area, &requested_folder))
            .await?
            .map_err(ApiError::refused)?
    };
    let settings = SessionSettings {
        language,
        destination: request.destination.into(),
        output_folder,
        organise_by_day: request.organise_by_day,
        collision_policy: request.collision_policy.into(),
    };
    apply_session_settings(&app, settings).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// NAME-05: empty means no language.
fn parsed_language(tag: Option<&str>) -> Result<Option<SubtitleLanguage>, ApiError> {
    match tag.map(str::trim) {
        None | Some("") => Ok(None),
        Some(tag) => SubtitleLanguage::parse(tag)
            .map(Some)
            .map_err(|_| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, Reason::InvalidLanguage)),
    }
}

/// SAFE-13: a folder's real location, inside the allowed area.
fn real_folder(area: &AllowedArea, folder: &Path) -> Result<PathBuf, SkipReason> {
    let real = area.real_location(folder)?;
    if !real.is_dir() {
        return Err(SkipReason::NotAFolder);
    }
    Ok(real)
}

/// UI-05 and ENC-13: detection runs again when the language changes.
async fn apply_session_settings(app: &AppState, settings: SessionSettings) -> Result<(), ApiError> {
    let language = settings.language.clone();
    let language_changed = app.session().update_settings(settings)?;
    if !language_changed {
        return Ok(());
    }
    let files = app.session().detection_dependent();
    let detections = blocking(move || detect_again(files, language.as_ref())).await?;
    let mut session = app.session();
    for (id, detection) in detections {
        session.set_detection(id, detection);
    }
    Ok(())
}

/// SET-01 and SET-04: every value is checked before anything is saved, and the current
/// session takes the new defaults.
pub async fn save_defaults(
    State(app): State<AppState>,
    Json(request): Json<Defaults>,
) -> Result<StatusCode, ApiError> {
    check_no_conversion(&app)?;
    let defaults = checked_defaults(&app, request).await?;
    let session_settings = defaults.session_settings();
    let store = app.clone();
    blocking(move || store.defaults().save(defaults))
        .await?
        .map_err(|problem| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            problem: store_problem(problem),
        })?;
    apply_session_settings(&app, session_settings).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// SET-03.
pub async fn restore_defaults(State(app): State<AppState>) -> Result<StatusCode, ApiError> {
    check_no_conversion(&app)?;
    let store = app.clone();
    let session_settings = blocking(move || {
        let mut store = store.defaults();
        store.restore().map(|()| store.current.session_settings())
    })
    .await?
    .map_err(|problem| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        problem: store_problem(problem),
    })?;
    apply_session_settings(&app, session_settings).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn check_no_conversion(app: &AppState) -> Result<(), ApiError> {
    if app.session().conversion.is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            Reason::ConversionRunning,
        ));
    }
    Ok(())
}

/// SET-04 and WATCH-05: folders inside the allowed area; watching is Docker's alone, and a
/// watched folder may not hold the output folder, where its own outputs would land.
async fn checked_defaults(app: &AppState, defaults: Defaults) -> Result<Defaults, ApiError> {
    let language =
        parsed_language(defaults.language.as_deref())?.map(|language| language.tag().to_owned());
    if app.settings.mode == Mode::Desktop && !defaults.watch_folders.is_empty() {
        return Err(ApiError::new(StatusCode::FORBIDDEN, Reason::NotAvailable));
    }
    if defaults.watch_folders.len() > MAXIMUM_WATCH_FOLDERS {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            Reason::TooManyWatchFolders,
        ));
    }
    let area = app.settings.allowed_area.clone();
    blocking(move || {
        let output_folder =
            real_folder(&area, &defaults.output_folder).map_err(ApiError::refused)?;
        let writes_to_output_folder = defaults.destination == DestinationName::OutputFolder;
        let mut watch_folders: Vec<PathBuf> = Vec::new();
        for folder in &defaults.watch_folders {
            let real = real_folder(&area, folder).map_err(ApiError::refused)?;
            if writes_to_output_folder && output_folder.starts_with(&real) {
                return Err(ApiError {
                    status: StatusCode::UNPROCESSABLE_ENTITY,
                    problem: Problem::about(Reason::WatchFolderHoldsOutputFolder, &text(&real)),
                });
            }
            if !watch_folders.contains(&real) {
                watch_folders.push(real);
            }
        }
        Ok(Defaults {
            language,
            output_folder,
            watch_folders,
            ..defaults
        })
    })
    .await?
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    query: String,
}

/// HIST-02: the words travel in the body, so file names never appear in an address, where a
/// reverse proxy might log them.
pub async fn history(
    State(app): State<AppState>,
    Json(request): Json<HistoryRequest>,
) -> Json<Vec<HistoryEntryView>> {
    let query: String = request
        .query
        .chars()
        .take(MAXIMUM_QUERY_CHARACTERS)
        .collect();
    Json(history_view(app.history().search(&query)))
}

/// WATCH-04.
pub async fn watch_log(State(app): State<AppState>) -> Json<WatchLogView> {
    Json(watch_log_view(
        &app.watch_log(),
        app.settings.watch_interval,
    ))
}

/// UPDATE-02: a manual check; one made in the last half minute is answered from memory.
pub async fn check_for_update(State(app): State<AppState>) -> Result<Json<UpdateView>, ApiError> {
    if !app.settings.update_check_allowed {
        return Err(ApiError::new(StatusCode::FORBIDDEN, Reason::UpdateCheckOff));
    }
    let checked_recently = app
        .update()
        .last_check
        .is_some_and(|time| time.elapsed() < MINIMUM_MANUAL_CHECK_INTERVAL);
    if !checked_recently {
        run_update_check(&app).await;
    }
    Ok(Json(current_update_view(&app)))
}

/// UPDATE-01 and UPDATE-02.
pub async fn run_update_check(app: &AppState) {
    let latest = tokio::task::spawn_blocking(update::fetch_latest)
        .await
        .unwrap_or(Err(UpdateProblem::CheckFailed));
    app.update().record_check(latest);
}

/// UPDATE-03: the page names nothing; the release file comes from SubUTF8's own check.
pub async fn download_update(State(app): State<AppState>) -> Result<StatusCode, ApiError> {
    let (release, asset) = {
        let mut update = app.update();
        if update.is_busy() {
            return Err(ApiError::new(StatusCode::CONFLICT, Reason::UpdateBusy));
        }
        let release = update
            .newer
            .clone()
            .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, Reason::NoUpdate))?;
        let asset = update
            .release_file()
            .cloned()
            .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, Reason::NoReleaseFile))?;
        update.downloaded = None;
        update.step = Step::Downloading {
            received: 0,
            total: asset.size,
        };
        (release, asset)
    };
    // The desktop's Downloads folder, where CineSort keeps its updates too.
    let folder = app.settings.default_output_folder.clone();
    let state = app.clone();
    tokio::task::spawn_blocking(move || {
        let downloaded = update::download(&release, &asset, &folder, |received| {
            if let Step::Downloading {
                received: shown, ..
            } = &mut state.update().step
            {
                *shown = received;
            }
        });
        let mut update = state.update();
        match downloaded {
            Ok(downloaded) => {
                update.downloaded = Some(downloaded);
                update.step = Step::Idle;
            }
            Err(problem) => update.step = Step::Failed(problem),
        }
    });
    Ok(StatusCode::ACCEPTED)
}

/// UPDATE-04.
pub async fn install_update(State(app): State<AppState>) -> Result<StatusCode, ApiError> {
    let (downloaded, package) = {
        let mut update = app.update();
        if update.is_busy() {
            return Err(ApiError::new(StatusCode::CONFLICT, Reason::UpdateBusy));
        }
        let downloaded = update
            .downloaded
            .clone()
            .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, Reason::UpdateNotDownloaded))?;
        update.step = Step::Installing;
        (downloaded, update.package.clone())
    };
    let state = app.clone();
    tokio::task::spawn_blocking(move || {
        let installed = update::install(&downloaded, &package);
        state.update().step = match installed {
            Ok(()) => Step::Installed,
            Err(problem) => Step::Failed(problem),
        };
    });
    Ok(StatusCode::ACCEPTED)
}

/// UPDATE-05: the new copy starts, and this one stops. The instance record goes first, so the
/// new copy does not hand itself back to this one (ACCESS-08).
pub async fn restart_after_update(State(app): State<AppState>) -> Result<StatusCode, ApiError> {
    let package = {
        let update = app.update();
        if update.step != Step::Installed {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                Reason::UpdateNotInstalled,
            ));
        }
        update.package.clone()
    };
    instance::remove(app.settings.listen_address.port());
    update::start_new_copy(&package)
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, Reason::Internal))?;
    app.session().cancel_conversion();
    let _ = app.shutdown.send(true);
    Ok(StatusCode::ACCEPTED)
}

/// UI-05 and UI-06: converts every Ready file in the background; progress shows in the state.
/// TARGET-02: nothing is written to an output folder that is not mounted.
pub async fn convert(State(app): State<AppState>) -> Result<StatusCode, ApiError> {
    let conversion = {
        let mut session = app.session();
        let output_folder_usable = app
            .settings
            .allowed_area
            .real_location(&session.settings.output_folder)
            .is_ok();
        if session.needs_output_folder() && !output_folder_usable {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                Reason::OutputFolderUnavailable,
            ));
        }
        session.start_conversion()?
    };
    let worker = app.clone();
    let conversion = tokio::task::spawn_blocking(move || {
        let mut records = Vec::new();
        conversion.run(|finished, job, outcome| {
            lock(&worker.session).record_outcome(finished, outcome);
            records.extend(HistoryRecord::of(
                job,
                outcome,
                conversion.language(),
                false,
            ));
        });
        worker.history().append(records);
    });
    let session = Arc::clone(&app.session);
    tokio::spawn(async move {
        let _ = conversion.await;
        lock(&session).finish_conversion();
    });
    Ok(StatusCode::ACCEPTED)
}

/// UI-06: stops after the current file.
pub async fn cancel(State(app): State<AppState>) -> StatusCode {
    app.session().cancel_conversion();
    StatusCode::NO_CONTENT
}

/// ACCESS-09: Quit stops the desktop app, after the file being converted, if any.
pub async fn quit(State(app): State<AppState>) -> Result<StatusCode, ApiError> {
    if app.settings.mode == Mode::Container {
        return Err(ApiError::new(StatusCode::FORBIDDEN, Reason::NotAvailable));
    }
    app.session().cancel_conversion();
    let _ = app.shutdown.send(true);
    Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{
        ADD_ROUTE, BROWSE_ROUTE, CONVERT_ROUTE, DEFAULTS_ROUTE, HISTORY_ROUTE,
        MAXIMUM_UPLOAD_BYTES, RESTORE_DEFAULTS_ROUTE, SETTINGS_ROUTE, STATE_ROUTE, UPLOAD_ROUTE,
    };
    use crate::test_support::{
        api_request, call, fixture_output, fixture_source, parity_set, route, send, test_app,
        wait_for_conversion,
    };
    use axum::body::Body;
    use axum::http::Method;
    use serde_json::{Value, json};
    use std::fs;
    use std::os::unix::fs::symlink;

    async fn post(app: &axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
        call(app, Method::POST, &route(path), Some(body)).await
    }

    async fn served_page(app: &axum::Router) -> String {
        let (_, _, body) = send(app, api_request(Method::GET, "/", Body::empty())).await;
        String::from_utf8(body).unwrap()
    }

    fn file_route(id: &Value, action: &str) -> String {
        route(&format!("/files/{id}/{action}"))
    }

    async fn upload_bytes(app: &axum::Router, name: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
        let uri = format!("{}?name={}", route(UPLOAD_ROUTE), encode_query(name));
        let (status, _, body) = send(app, api_request(Method::POST, &uri, Body::from(bytes))).await;
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    fn encode_query(text: &str) -> String {
        text.bytes()
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => {
                    char::from(byte).to_string()
                }
                _ => format!("%{byte:02X}"),
            })
            .collect()
    }

    /// SAFE-12 and SAFE-13.
    #[tokio::test]
    async fn browse_cannot_leave_allowed_folder() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        fs::create_dir_all(&data).unwrap();
        symlink(root.path(), data.join("escape")).unwrap();
        let app = test_app(&data, &data);
        for path in [data.join(".."), data.join("escape"), PathBuf::from("/etc")] {
            let (status, body) = call(
                &app,
                Method::POST,
                &route(BROWSE_ROUTE),
                Some(json!({ "path": path })),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{}", path.display());
            assert_eq!(body["reason"], "outside-allowed-area");
        }
        let (status, body) = call(&app, Method::POST, &route(BROWSE_ROUTE), Some(json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["path"], json!(fs::canonicalize(&data).unwrap()));
        assert_eq!(body["parent"], Value::Null);
    }

    /// LIMIT-01.
    #[tokio::test]
    async fn oversized_upload_is_rejected() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let (status, _) = upload_bytes(&app, "big.srt", vec![b'a'; MAXIMUM_UPLOAD_BYTES + 1]).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["files"], json!([]));
    }

    /// NAME-01, NAME-07, SAFE-02 and SAFE-15.
    #[tokio::test]
    async fn upload_name_is_reduced_to_base_name() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("output");
        fs::create_dir_all(&output).unwrap();
        let app = test_app(&output, &output);
        let bytes = fs::read(fixture_source("windows-1250-romanian")).unwrap();
        for (name, reason) in [("..", "unusable-dropped-name"), ("notes.txt", "not-srt")] {
            let (status, body) = upload_bytes(&app, name, bytes.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
            assert_eq!(body["reason"], reason);
        }
        let (status, body) = upload_bytes(&app, "../../outside/Film.srt", bytes).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["added"], 1);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["files"][0]["name"], "Film.srt");
        assert_eq!(state["files"][0]["folder"], Value::Null);
        assert_eq!(state["files"][0]["status"], "ready");
        let (status, _) = call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let state = wait_for_conversion(&app).await;
        assert_eq!(state["files"][0]["status"], "converted");
        let written = fs::read_to_string(output.join("Film.srt")).unwrap();
        assert_eq!(written, fixture_output("windows-1250-romanian"));
        assert!(!root.path().join("outside").exists());
    }

    /// TARGET-01 and UI-05: browsed files convert to exactly the golden outputs, written beside
    /// their originals with `1` after the name (NAME-03).
    #[tokio::test]
    async fn browsed_files_convert_to_the_golden_outputs() {
        let data = tempfile::tempdir().unwrap();
        let names = parity_set();
        let paths: Vec<PathBuf> = names
            .iter()
            .map(|name| {
                let path = data.path().join(format!("{name}.srt"));
                fs::copy(fixture_source(name), &path).unwrap();
                path
            })
            .collect();
        let app = test_app(data.path(), data.path());
        let (status, body) = call(
            &app,
            Method::POST,
            &route(ADD_ROUTE),
            Some(json!({ "paths": paths })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["added"], names.len());
        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        let state = wait_for_conversion(&app).await;
        for (file, name) in state["files"].as_array().unwrap().iter().zip(&names) {
            assert_eq!(file["status"], "converted", "{name}");
            let written = fs::read_to_string(data.path().join(format!("{name}1.srt"))).unwrap();
            assert_eq!(written, fixture_output(name), "{name}");
        }
    }

    /// ENC-06 and ENC-14: a file already in UTF-8 is converted too, and gains the byte-order
    /// mark.
    #[tokio::test]
    async fn utf8_without_byte_order_mark_gains_one() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("Film.srt");
        fs::copy(fixture_source("utf8-romanian"), &path).unwrap();
        let app = test_app(data.path(), data.path());
        let paths = json!({ "paths": [path] });
        call(&app, Method::POST, &route(ADD_ROUTE), Some(paths)).await;
        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        let state = wait_for_conversion(&app).await;
        assert_eq!(state["files"][0]["status"], "converted");
        let written = fs::read_to_string(data.path().join("Film1.srt")).unwrap();
        assert_eq!(written, fixture_output("utf8-romanian"));
    }

    /// ENC-12 and UI-04.
    #[tokio::test]
    async fn manual_choice_is_decoded_strictly() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("short.srt");
        fs::copy(fixture_source("short-romanian"), &path).unwrap();
        let app = test_app(data.path(), data.path());
        call(
            &app,
            Method::POST,
            &route(ADD_ROUTE),
            Some(json!({ "paths": [path] })),
        )
        .await;
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let id = state["files"][0]["id"].clone();
        assert_eq!(state["files"][0]["status"], "needs-review");
        let (status, body) = call(
            &app,
            Method::POST,
            &file_route(&id, "encoding"),
            Some(json!({ "encoding": "UTF-8" })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["reason"], "not-offered");
        let (status, _) = call(
            &app,
            Method::POST,
            &file_route(&id, "encoding"),
            Some(json!({ "encoding": "windows-1250" })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["files"][0]["status"], "ready");
        assert_eq!(state["files"][0]["encoding"], "windows-1250");
        let (_, preview) = call(&app, Method::GET, &file_route(&id, "preview"), None).await;
        assert_eq!(preview["encoding"], "windows-1250");
        assert!(!preview["cues"].as_array().unwrap().is_empty());
    }

    /// SET-01 to SET-04, WATCH-05 and UI-13.
    #[tokio::test]
    async fn defaults_are_checked_saved_served_and_restored() {
        let data = tempfile::tempdir().unwrap();
        let watched = data.path().join("incoming");
        fs::create_dir_all(&watched).unwrap();
        let app = test_app(data.path(), data.path());
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let factory = state["defaults"].clone();
        assert_eq!(factory["theme"], "system");
        let refusals = [
            (
                "language",
                json!("../x"),
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid-language",
            ),
            (
                "outputFolder",
                json!("/etc"),
                StatusCode::FORBIDDEN,
                "outside-allowed-area",
            ),
        ];
        for (field, value, expected_status, reason) in refusals {
            let mut defaults = factory.clone();
            defaults[field] = value;
            let (status, body) = post(&app, DEFAULTS_ROUTE, defaults).await;
            assert_eq!(
                (status, body["reason"].clone()),
                (expected_status, json!(reason))
            );
        }
        let mut defaults = factory.clone();
        defaults["destination"] = json!("output-folder");
        defaults["watchFolders"] = json!([data.path()]);
        let (status, body) = post(&app, DEFAULTS_ROUTE, defaults.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["reason"], "watch-folder-holds-output-folder");
        defaults["watchFolders"] = json!([watched]);
        defaults["theme"] = json!("dark");
        defaults["language"] = json!("RO");
        let (status, _) = post(&app, DEFAULTS_ROUTE, defaults).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["defaults"]["language"], "ro");
        assert_eq!(
            state["defaults"]["watchFolders"],
            json!([fs::canonicalize(&watched).unwrap()])
        );
        assert_eq!(state["settings"]["destination"], "output-folder");
        assert_eq!(state["settings"]["language"], "ro");
        assert!(served_page(&app).await.contains("data-theme=\"dark\""));
        let (status, _) = call(&app, Method::POST, &route(RESTORE_DEFAULTS_ROUTE), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["defaults"], factory);
        assert!(served_page(&app).await.contains("data-theme=\"system\""));
    }

    /// HIST-01 and HIST-02.
    #[tokio::test]
    async fn converted_files_are_found_in_the_history() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("Film.srt");
        fs::copy(fixture_source("windows-1250-romanian"), &path).unwrap();
        let app = test_app(data.path(), data.path());
        let paths = json!({ "paths": [path] });
        call(&app, Method::POST, &route(ADD_ROUTE), Some(paths)).await;
        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        wait_for_conversion(&app).await;
        let (status, found) = post(
            &app,
            HISTORY_ROUTE,
            json!({ "query": "film.SRT windows-1250" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(found.as_array().unwrap().len(), 1);
        assert_eq!(found[0]["name"], "Film.srt");
        assert!(found[0]["output"].as_str().unwrap().ends_with("Film1.srt"));
        assert_eq!(found[0]["watched"], false);
        let (_, found) = post(&app, HISTORY_ROUTE, json!({ "query": "nothing" })).await;
        assert_eq!(found, json!([]));
    }

    /// NAME-05 and SAFE-13.
    #[tokio::test]
    async fn settings_are_validated() {
        let data = tempfile::tempdir().unwrap();
        let app = test_app(data.path(), data.path());
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let mut settings = state["settings"].clone();
        settings["language"] = json!("../x");
        let (status, body) = call(
            &app,
            Method::POST,
            &route(SETTINGS_ROUTE),
            Some(settings.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["reason"], "invalid-language");
        settings["language"] = json!("ro");
        settings["outputFolder"] = json!("/etc");
        let (status, body) = call(
            &app,
            Method::POST,
            &route(SETTINGS_ROUTE),
            Some(settings.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["reason"], "outside-allowed-area");
        settings["outputFolder"] = state["settings"]["outputFolder"].clone();
        let (status, _) = call(&app, Method::POST, &route(SETTINGS_ROUTE), Some(settings)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["settings"]["language"], "ro");
    }
}
