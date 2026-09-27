use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path as RoutePath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use subutf8_core::batch::Origin;
use subutf8_core::decoding::Reading;
use subutf8_core::detection;
use subutf8_core::encoding_catalog::{find_manual_choice, manual_choices};
use subutf8_core::input_scan::{AllowedArea, SkipReason};
use subutf8_core::language::{SubtitleLanguage, hinted_languages};
use subutf8_core::output_naming::{dropped_file_name, split_srt_name};
use subutf8_core::report::Outcome;

use crate::constants::{
    ATTACHMENT_DISPOSITION_PREFIX, MAXIMUM_QUERY_CHARACTERS, MAXIMUM_WATCH_FOLDERS,
    MINIMUM_MANUAL_CHECK_INTERVAL, SUBRIP_CONTENT_TYPE, UNRESERVED_NAME_PUNCTUATION,
};
use crate::defaults::Defaults;
use crate::folders::list_folder;
use crate::history::HistoryRecord;
use crate::instance;
use crate::json_path::JsonPath;
use crate::server::{AppState, lock};
use crate::session::{
    FilePreview, SessionProblem, SessionSettings, detect_again, gather, origin_bytes, prepare,
    preview_file, read_file,
};
use crate::settings::Mode;
use crate::update::{self, Step, UpdateProblem};
use crate::views::{
    AddedView, CandidatesView, DestinationName, HistoryEntryView, LanguagesView, ListingView,
    PreviewView, Problem, Reason, StateExtras, StateView, UpdateView, WatchLogView, added_view,
    candidates_view, data_problem, gathered_view, history_view, listing_view,
    manual_choice_problem, preview_view, read_problem_description, skip_reason_problem, state_view,
    store_problem, update_view, watch_log_view,
};

/// An error answer: the HTTP status and the reason the interface shows. The problem is boxed so
/// every `Result` that may carry it stays small.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    problem: Box<Problem>,
}

impl ApiError {
    fn with_problem(status: StatusCode, problem: Problem) -> Self {
        Self {
            status,
            problem: Box::new(problem),
        }
    }

    fn new(status: StatusCode, reason: Reason) -> Self {
        Self::with_problem(status, Problem::of(reason))
    }

    fn refused(reason: SkipReason) -> Self {
        Self::with_problem(StatusCode::FORBIDDEN, skip_reason_problem(reason))
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
            SessionProblem::ManualChoice(refusal) => Self::with_problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                manual_choice_problem(refusal),
            ),
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

/// UI-19.
pub async fn languages() -> Json<LanguagesView> {
    Json(LanguagesView {
        hinted: hinted_languages(),
    })
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
    paths: Vec<JsonPath>,
    #[serde(default)]
    include_subfolders: bool,
}

pub async fn add(
    State(app): State<AppState>,
    Json(request): Json<AddRequest>,
) -> Result<Json<AddedView>, ApiError> {
    let paths = requested_paths(request.paths)?;
    let added = add_paths(&app, paths, request.include_subfolders).await?;
    Ok(Json(added))
}

/// NAME-11: names in any encoding; a path whose hexadecimal does not read is refused whole.
fn requested_paths(paths: Vec<JsonPath>) -> Result<Vec<PathBuf>, ApiError> {
    paths
        .into_iter()
        .map(JsonPath::into_path)
        .collect::<Option<_>>()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, Reason::InvalidPath))
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
    paths: Vec<JsonPath>,
}

/// ACCESS-08: a second launch hands its files over and asks for the interface to show.
pub async fn open(
    State(app): State<AppState>,
    Json(request): Json<OpenRequest>,
) -> Result<Json<AddedView>, ApiError> {
    let Some(interface) = app.interface.clone() else {
        return Err(ApiError::new(StatusCode::FORBIDDEN, Reason::NotAvailable));
    };
    let paths = requested_paths(request.paths)?;
    let added = add_paths(&app, paths, false).await?;
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

/// UI-04, ENC-22 for a garbled file, and ENC-23 with the file's language.
pub async fn preview(
    State(app): State<AppState>,
    RoutePath(id): RoutePath<u64>,
) -> Result<Json<PreviewView>, ApiError> {
    let (origin, reading, misreading, language, comma_letters) = {
        let session = app.session();
        let file = session.find(id)?;
        let language = file.effective_language(session.settings.language.as_ref());
        (
            file.origin.clone(),
            file.current_reading(),
            file.misreading,
            language.cloned(),
            session.settings.romanian_comma_letters,
        )
    };
    let Some(reading) = reading else {
        let nothing = FilePreview {
            cues: Ok(Vec::new()),
            repair_sample: None,
        };
        return Ok(Json(preview_view(None, nothing)));
    };
    let preview = blocking(move || {
        preview_file(
            &origin,
            reading,
            misreading,
            language.as_ref(),
            comma_letters,
        )
    })
    .await?;
    Ok(Json(preview_view(Some(reading.encoding()), preview)))
}

/// UI-15: encodings to suggest, each with a sample line; none for a file that cannot be read.
pub async fn candidates(
    State(app): State<AppState>,
    RoutePath(id): RoutePath<u64>,
) -> Result<Json<CandidatesView>, ApiError> {
    let (origin, classification, language) = {
        let session = app.session();
        let file = session.find(id)?;
        let language = session.settings.language.clone();
        (file.origin.clone(), file.classification, language)
    };
    let Ok(classification) = classification else {
        return Ok(Json(CandidatesView::default()));
    };
    let found = blocking(move || {
        origin_bytes(&origin)
            .map(|bytes| detection::candidates(&bytes, classification, language.as_ref()))
            .unwrap_or_default()
    })
    .await?;
    Ok(Json(candidates_view(found)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EncodingRequest {
    encoding: String,
    /// ENC-21: a damaged or mixed file keeps its lines that are UTF-8.
    #[serde(default)]
    keep_utf8_lines: bool,
}

/// ENC-12 and ENC-21: only the exact name of an offered encoding is accepted.
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
            ApiError::with_problem(StatusCode::UNPROCESSABLE_ENTITY, problem)
        })?;
    let reading = if request.keep_utf8_lines {
        Reading::Utf8LinesElse(encoding)
    } else {
        Reading::Whole(encoding)
    };
    app.session().choose_reading(id, reading, &bytes)?;
    Ok(StatusCode::NO_CONTENT)
}

/// UI-17: a converted file as it was written, for a browser to save. Only the output recorded for
/// a listed file is sent, read as sources are, and only from inside the allowed area (SAFE-13).
pub async fn output(
    State(app): State<AppState>,
    RoutePath(id): RoutePath<u64>,
) -> Result<Response, ApiError> {
    let output = match &app.session().find(id)?.outcome {
        Some(Outcome::Converted { output, .. }) => output.clone(),
        _ => return Err(ApiError::new(StatusCode::CONFLICT, Reason::NotConverted)),
    };
    let name = output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let area = app.settings.allowed_area.clone();
    let bytes = blocking(move || {
        let real = area.real_location(&output).map_err(ApiError::refused)?;
        read_file(&real).map_err(|problem| {
            let (_, problem) = read_problem_description(problem);
            ApiError::with_problem(StatusCode::UNPROCESSABLE_ENTITY, problem)
        })
    })
    .await??;
    let headers = [
        (header::CONTENT_TYPE, SUBRIP_CONTENT_TYPE.to_owned()),
        (
            header::CONTENT_DISPOSITION,
            [ATTACHMENT_DISPOSITION_PREFIX, &percent_encoded(&name)].concat(),
        ),
    ];
    Ok((headers, bytes).into_response())
}

/// RFC 8187: letters, digits and a little punctuation stay as they are, and every other byte of
/// the name becomes `%XX`, so no name can break the header.
fn percent_encoded(name: &str) -> String {
    name.bytes()
        .map(|byte| {
            let is_unreserved =
                byte.is_ascii_alphanumeric() || UNRESERVED_NAME_PUNCTUATION.contains(&byte);
            if is_unreserved {
                return char::from(byte).to_string();
            }
            format!("%{byte:02X}")
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairRequest {
    repair: bool,
}

/// ENC-22: Repair, or Keep as it is.
pub async fn repair(
    State(app): State<AppState>,
    RoutePath(id): RoutePath<u64>,
    Json(request): Json<RepairRequest>,
) -> Result<StatusCode, ApiError> {
    app.session().choose_repair(id, request.repair)?;
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

/// SET-01 and ENC-13: the list takes the saved settings at once, and detection runs again when
/// the language changes.
async fn apply_session_settings(app: &AppState, settings: SessionSettings) -> Result<(), ApiError> {
    let language_changed = app.session().update_settings(settings)?;
    if !language_changed {
        return Ok(());
    }
    let files = app.session().detection_dependent();
    detect_files_again(app, files).await
}

/// ENC-13: detects files again off the server's threads, each with the language it goes by.
async fn detect_files_again(
    app: &AppState,
    files: Vec<(u64, Origin, Option<SubtitleLanguage>)>,
) -> Result<(), ApiError> {
    let redetections = blocking(move || detect_again(files)).await?;
    let mut session = app.session();
    for redetection in redetections {
        session.set_detection(redetection);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileLanguageRequest {
    language: Option<String>,
}

/// UI-16: a file's own subtitle language, or none to follow the saved one again. The file is
/// detected again with it (ENC-13); NAME-05 checks the tag first.
pub async fn set_file_language(
    State(app): State<AppState>,
    RoutePath(id): RoutePath<u64>,
    Json(request): Json<FileLanguageRequest>,
) -> Result<StatusCode, ApiError> {
    let language = parsed_language(request.language.as_deref())?;
    let to_detect = {
        let mut session = app.session();
        let changed = session.set_file_language(id, language)?;
        let file = session.find(id)?;
        let language = file.effective_language(session.settings.language.as_ref());
        let needs_detection = changed && file.depends_on_language();
        needs_detection.then(|| vec![(id, file.origin.clone(), language.cloned())])
    };
    if let Some(files) = to_detect {
        detect_files_again(&app, files).await?;
    }
    Ok(StatusCode::NO_CONTENT)
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
        .map_err(|problem| {
            ApiError::with_problem(StatusCode::INTERNAL_SERVER_ERROR, store_problem(problem))
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
    .map_err(|problem| {
        ApiError::with_problem(StatusCode::INTERNAL_SERVER_ERROR, store_problem(problem))
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
                return Err(ApiError::with_problem(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Problem::about(Reason::WatchFolderHoldsOutputFolder, &text(&real)),
                ));
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
            records.extend(HistoryRecord::of(job, outcome, false));
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
        ADD_ROUTE, BROWSE_ROUTE, CLEAR_ROUTE, CONVERT_ROUTE, DEFAULTS_ROUTE, HISTORY_ROUTE,
        LANGUAGES_ROUTE, MAXIMUM_UPLOAD_BYTES, RESTORE_DEFAULTS_ROUTE, STATE_ROUTE, UPLOAD_ROUTE,
    };
    use crate::json_path::encode_hex;
    use crate::test_support::{
        api_request, call, fixture_output, fixture_source, parity_set, route, send, test_app,
        wait_for_conversion,
    };
    use axum::body::Body;
    use axum::http::Method;
    use serde_json::{Value, json};
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    use subutf8_core::language::with_romanian_comma_letters;

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
        defaults["organiseByDay"] = json!(true);
        let (status, _) = post(&app, DEFAULTS_ROUTE, defaults).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["defaults"]["language"], "ro");
        assert_eq!(
            state["defaults"]["watchFolders"],
            json!([fs::canonicalize(&watched).unwrap()])
        );
        assert!(served_page(&app).await.contains("data-theme=\"dark\""));
        // The next conversion uses the saved settings: the language tag, and the day's folder
        // in the output folder.
        let original = data.path().join("Film.srt");
        fs::copy(fixture_source("windows-1250-romanian"), &original).unwrap();
        post(&app, ADD_ROUTE, json!({ "paths": [original] })).await;
        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        wait_for_conversion(&app).await;
        let output_folder = fs::canonicalize(data.path()).unwrap();
        let written = output_folder
            .join(crate::clock::today().name())
            .join("Film.ro.srt");
        assert_eq!(
            fs::read_to_string(written).unwrap(),
            fixture_output("windows-1250-romanian")
        );
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

    async fn first_file_id(app: &axum::Router) -> Value {
        let (_, state) = call(app, Method::GET, &route(STATE_ROUTE), None).await;
        state["files"][0]["id"].clone()
    }

    /// ENC-12: a refused encoding names the line where it stops fitting.
    #[tokio::test]
    async fn refused_choice_reports_its_line() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        upload_bytes(
            &app,
            "markup.srt",
            fs::read(fixture_source("markup")).unwrap(),
        )
        .await;
        let id = first_file_id(&app).await;
        let (status, body) = call(
            &app,
            Method::POST,
            &file_route(&id, "encoding"),
            Some(json!({ "encoding": "ISO-8859-2" })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["reason"], "does-not-decode");
        assert_eq!(body["line"], 3);
    }

    /// UI-14: UTF-32 starts like UTF-16, so its preview stops on its first line.
    #[tokio::test]
    async fn failed_preview_shows_the_broken_line() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let utf32_little_endian = vec![0xFF, 0xFE, 0x00, 0x00, b'H', 0x00, 0x00, 0x00];
        upload_bytes(&app, "wide.srt", utf32_little_endian).await;
        let id = first_file_id(&app).await;
        let (_, preview) = call(&app, Method::GET, &file_route(&id, "preview"), None).await;
        assert_eq!(preview["problem"]["reason"], "damaged-utf16");
        assert_eq!(preview["problem"]["line"], 1);
        assert_eq!(
            preview["brokenLine"],
            json!({ "line": 1, "text": "\u{FFFD}H\u{FFFD}" })
        );
    }

    /// UI-15: "Bună!" has one accented letter, too little to detect, so it needs review.
    #[tokio::test]
    async fn candidates_are_offered_for_a_short_file() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let bytes = fs::read(fixture_source("short-romanian")).unwrap();
        upload_bytes(&app, "short.srt", bytes).await;
        let id = first_file_id(&app).await;
        let (status, body) = call(&app, Method::GET, &file_route(&id, "candidates"), None).await;
        assert_eq!(status, StatusCode::OK);
        let found = body["candidates"].as_array().unwrap();
        assert!(found.iter().any(|candidate| candidate["sample"] == "Bună!"));

        let (status, _) = call(
            &app,
            Method::GET,
            &file_route(&json!(0), "candidates"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    async fn set_language(app: &axum::Router, id: &Value, language: &str) -> (StatusCode, Value) {
        let body = json!({ "language": language });
        call(app, Method::POST, &file_route(id, "language"), Some(body)).await
    }

    /// UI-16 and ENC-13: a file's own language replaces the saved one, for its detection and
    /// for its output name.
    #[tokio::test]
    async fn a_file_language_overrides_the_list() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let bytes = fs::read(fixture_source("windows-1252-french")).unwrap();
        upload_bytes(&app, "french.srt", bytes).await;
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let mut defaults = state["defaults"].clone();
        defaults["language"] = json!("ro");
        let (status, _) = post(&app, DEFAULTS_ROUTE, defaults).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let id = state["files"][0]["id"].clone();
        assert_eq!(state["files"][0]["status"], "needs-review");
        assert_eq!(
            state["files"][0]["problem"]["reason"],
            "language-hint-disagrees"
        );

        let (status, body) = set_language(&app, &id, "../x").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["reason"], "invalid-language");
        let (status, _) = set_language(&app, &id, "FR").await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["files"][0]["status"], "ready");
        assert_eq!(state["files"][0]["language"], "fr");

        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        let state = wait_for_conversion(&app).await;
        let output = state["files"][0]["output"].as_str().unwrap();
        assert!(output.ends_with("french.fr.srt"), "{output}");
    }

    /// ENC-21: a mixed file converts only with its UTF-8 lines kept, and then exactly.
    #[tokio::test]
    async fn mixed_file_converts_with_its_utf8_lines() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let bytes = fs::read(fixture_source("mixed-utf8-1250")).unwrap();
        upload_bytes(&app, "mixed.srt", bytes).await;
        let id = first_file_id(&app).await;
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["files"][0]["canKeepUtf8Lines"], true);
        assert_eq!(state["files"][0]["keepsUtf8Lines"], true);

        let whole = json!({ "encoding": "windows-1250" });
        let (status, body) = post_to(&app, &file_route(&id, "encoding"), whole).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["reason"], "does-not-decode");
        let kept = json!({ "encoding": "windows-1250", "keepUtf8Lines": true });
        let (status, _) = post_to(&app, &file_route(&id, "encoding"), kept).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["files"][0]["status"], "ready");
        assert_eq!(state["files"][0]["encoding"], "windows-1250");
        assert_eq!(state["files"][0]["reading"], "UTF-8 + windows-1250");

        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        wait_for_conversion(&app).await;
        let written = fs::read_to_string(folder.path().join("mixed.srt")).unwrap();
        assert_eq!(written, fixture_output("mixed-utf8-1250"));
        assert!(!written.contains("BunÄƒ"));
    }

    /// ENC-22: a garbled file waits until it is repaired or kept as it is.
    #[tokio::test]
    async fn garbled_file_waits_until_repaired_or_kept() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let bytes = fs::read(fixture_source("garbled-romanian")).unwrap();
        upload_bytes(&app, "garbled.srt", bytes).await;
        let id = first_file_id(&app).await;
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let file = &state["files"][0];
        assert_eq!(file["status"], "needs-review");
        assert_eq!(file["problem"]["reason"], "looks-garbled");
        assert_eq!(file["problem"]["encoding"], "windows-1250");
        assert_eq!(file["problem"]["misreadAs"], "windows-1252");
        assert_eq!(file["canRepair"], true);
        let (_, preview) = call(&app, Method::GET, &file_route(&id, "preview"), None).await;
        assert_eq!(
            preview["repair"]["asIs"],
            "Bunã dimineaþa! ªtii ce înseamnã asta?"
        );
        assert_eq!(
            preview["repair"]["repaired"],
            "Bună dimineaţa! Ştii ce înseamnă asta?"
        );

        let repair = json!({ "repair": true });
        let (status, _) = post_to(&app, &file_route(&id, "repair"), repair).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        assert_eq!(state["files"][0]["status"], "ready");
        assert_eq!(state["files"][0]["reading"], "windows-1250 (repaired)");
        assert_eq!(state["files"][0]["isRepaired"], true);

        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        wait_for_conversion(&app).await;
        let written = fs::read_to_string(folder.path().join("garbled.srt")).unwrap();
        assert_eq!(written, fixture_output("windows-1250-romanian"));
    }

    /// ENC-22: UTF-8 with a control character and no repair says so, with its line.
    #[tokio::test]
    async fn utf8_with_control_characters_names_its_line() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let text = "1\n00:00:01,000 --> 00:00:02,000\nBună \u{81}ziua\n";
        upload_bytes(&app, "control.srt", text.as_bytes().to_vec()).await;
        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        let state = wait_for_conversion(&app).await;
        let file = &state["files"][0];
        assert_eq!(file["status"], "failed");
        assert_eq!(file["problem"]["reason"], "control-characters-in-utf8");
        assert_eq!(file["problem"]["line"], 3);
    }

    /// ENC-23: with Romanian and the comma letters saved, the preview and the output use ș ț.
    #[tokio::test]
    async fn romanian_comma_letters_show_in_the_preview_and_the_output() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let mut defaults = state["defaults"].clone();
        assert_eq!(defaults["romanianCommaLetters"], false);
        defaults["language"] = json!("ro");
        defaults["romanianCommaLetters"] = json!(true);
        let (status, _) = post(&app, DEFAULTS_ROUTE, defaults).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let bytes = fs::read(fixture_source("windows-1250-romanian")).unwrap();
        upload_bytes(&app, "Film.srt", bytes).await;
        let id = first_file_id(&app).await;
        let (_, preview) = call(&app, Method::GET, &file_route(&id, "preview"), None).await;
        assert_eq!(
            preview["cues"][0]["text"],
            "Bună dimineața! Știi ce înseamnă asta?"
        );

        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        wait_for_conversion(&app).await;
        let written = fs::read_to_string(folder.path().join("Film.ro.srt")).unwrap();
        let output = fixture_output("windows-1250-romanian");
        assert_eq!(written, with_romanian_comma_letters(&output));
    }

    /// UI-17: a converted file comes back as it was written, named as RFC 8187 allows.
    #[tokio::test]
    async fn converted_output_can_be_downloaded() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let bytes = fs::read(fixture_source("windows-1250-romanian")).unwrap();
        upload_bytes(&app, "Bună ziua.srt", bytes).await;
        let id = first_file_id(&app).await;
        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        wait_for_conversion(&app).await;
        let request = api_request(Method::GET, &file_route(&id, "output"), Body::empty());
        let (status, headers, body) = send(&app, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, fixture_output("windows-1250-romanian").as_bytes());
        assert_eq!(headers[header::CONTENT_TYPE], SUBRIP_CONTENT_TYPE);
        assert_eq!(
            headers[header::CONTENT_DISPOSITION],
            "attachment; filename*=UTF-8''Bun%C4%83%20ziua.srt"
        );
    }

    /// UI-17: only a converted file in the list can be downloaded.
    #[tokio::test]
    async fn unconverted_output_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let bytes = fs::read(fixture_source("windows-1250-romanian")).unwrap();
        upload_bytes(&app, "Film.srt", bytes).await;
        let id = first_file_id(&app).await;
        let (status, body) = call(&app, Method::GET, &file_route(&id, "output"), None).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["reason"], "not-converted");
        let unknown = file_route(&json!(u64::MAX), "output");
        let (status, body) = call(&app, Method::GET, &unknown, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["reason"], "unknown-file");
    }

    /// UI-19: the page's language suggestions come from the hint table (ENC-13).
    #[tokio::test]
    async fn languages_are_published() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let (status, body) = call(&app, Method::GET, &route(LANGUAGES_ROUTE), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["hinted"], json!(hinted_languages()));
        assert_eq!(body["hinted"][0], "ro");
    }

    /// NAME-11: a name that is not UTF-8 is added by its bytes, read in the file's encoding,
    /// converted to a UTF-8 name beside the untouched original, offered by Browse, and its
    /// output is recognised when the folder is added again (SAFE-18).
    #[tokio::test]
    async fn legacy_name_is_listed_and_converted_with_a_utf8_name() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let original = fs::read(fixture_source("windows-1250-romanian")).unwrap();
        let legacy = fs::canonicalize(folder.path())
            .unwrap()
            .join(OsStr::from_bytes(b"Fat\xe3.srt"));
        fs::write(&legacy, &original).unwrap();
        let by_bytes = json!({ "hex": encode_hex(legacy.as_os_str().as_bytes()) });
        let (status, _) = post(&app, ADD_ROUTE, json!({ "paths": [by_bytes] })).await;
        assert_eq!(status, StatusCode::OK);
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let file = &state["files"][0];
        assert_eq!(file["name"], "Fată.srt");
        assert_eq!(file["status"], "ready");
        assert_eq!(file["encoding"], "windows-1250");

        call(&app, Method::POST, &route(CONVERT_ROUTE), None).await;
        wait_for_conversion(&app).await;
        let written = fs::read_to_string(folder.path().join("Fată1.srt")).unwrap();
        assert_eq!(written, fixture_output("windows-1250-romanian"));
        assert_eq!(fs::read(&legacy).unwrap(), original);

        let (_, listing) = post(&app, BROWSE_ROUTE, json!({ "path": folder.path() })).await;
        assert_eq!(listing["files"], json!(["Fată1.srt"]));
        assert_eq!(listing["rawFiles"][0]["display"], "Fat\u{FFFD}.srt");
        assert_eq!(listing["rawFiles"][0]["hex"], by_bytes["hex"]);

        call(&app, Method::POST, &route(CLEAR_ROUTE), None).await;
        post(&app, ADD_ROUTE, json!({ "paths": [folder.path()] })).await;
        let (_, state) = call(&app, Method::GET, &route(STATE_ROUTE), None).await;
        let copy = state["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["name"] == "Fată1.srt")
            .unwrap();
        assert_eq!(copy["problem"]["reason"], "converted-copy");
        assert_eq!(copy["problem"]["relatedName"], "Fată.srt");

        let bad = json!({ "paths": [{ "hex": "2f7" }] });
        let (status, body) = post(&app, ADD_ROUTE, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["reason"], "invalid-path");
    }

    async fn post_to(app: &axum::Router, full_route: &str, body: Value) -> (StatusCode, Value) {
        call(app, Method::POST, full_route, Some(body)).await
    }
}
