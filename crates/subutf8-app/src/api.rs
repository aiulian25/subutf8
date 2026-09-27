use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as RoutePath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use subutf8_core::batch::Origin;
use subutf8_core::encoding_catalog::{find_manual_choice, manual_choices};
use subutf8_core::input_scan::SkipReason;
use subutf8_core::language::SubtitleLanguage;
use subutf8_core::output_naming::{dropped_file_name, split_srt_name};

use crate::folders::list_folder;
use crate::server::{AppState, lock};
use crate::session::{
    SessionProblem, SessionSettings, detect_again, gather, origin_bytes, prepare, preview_file,
};
use crate::settings::Mode;
use crate::views::{
    AddedView, ListingView, PreviewView, Problem, Reason, SettingsView, StateView, added_view,
    gathered_view, listing_view, manual_choice_problem, preview_view, read_problem_description,
    skip_reason_problem, state_view,
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
    Json(state_view(&app.settings, &app.session(), window_open))
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
    let language = match request.language.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(tag) => Some(SubtitleLanguage::parse(tag).map_err(|_| {
            ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, Reason::InvalidLanguage)
        })?),
    };
    let current_folder = app.session().settings.output_folder.clone();
    let requested_folder = PathBuf::from(&request.output_folder);
    let output_folder = if requested_folder == current_folder {
        current_folder
    } else {
        checked_output_folder(&app, requested_folder).await?
    };
    let settings = SessionSettings {
        language: language.clone(),
        destination: request.destination.into(),
        output_folder,
        collision_policy: request.collision_policy.into(),
    };
    let language_changed = app.session().update_settings(settings)?;
    if !language_changed {
        return Ok(StatusCode::NO_CONTENT);
    }
    let files = app.session().detection_dependent();
    let detections = blocking(move || detect_again(files, language.as_ref())).await?;
    let mut session = app.session();
    for (id, detection) in detections {
        session.set_detection(id, detection);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn checked_output_folder(app: &AppState, folder: PathBuf) -> Result<PathBuf, ApiError> {
    let area = app.settings.allowed_area.clone();
    blocking(move || {
        let real = area.real_location(&folder)?;
        if !real.is_dir() {
            return Err(SkipReason::NotAFolder);
        }
        Ok(real)
    })
    .await?
    .map_err(ApiError::refused)
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
    let worker_session = Arc::clone(&app.session);
    let conversion = tokio::task::spawn_blocking(move || {
        conversion.run(|finished, outcome| {
            lock(&worker_session).record_outcome(finished, outcome);
        });
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
        ADD_ROUTE, BROWSE_ROUTE, CONVERT_ROUTE, MAXIMUM_UPLOAD_BYTES, SETTINGS_ROUTE, STATE_ROUTE,
        UPLOAD_ROUTE,
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
