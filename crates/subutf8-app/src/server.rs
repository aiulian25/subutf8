use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use subutf8_core::batch::folder_is_writable;
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

use crate::access::{host_is_allowed, request_is_allowed};
use crate::api;
use crate::constants::{
    ADD_ROUTE, API_PREFIX, ASSETS, BROWSE_ROUTE, CANCEL_ROUTE, CANDIDATES_ROUTE, CLEAR_ROUTE,
    CONTAINER_FOLDERS_MESSAGE, CONTAINER_RUNNING_MESSAGE, CONTENT_SECURITY_POLICY, CONVERT_ROUTE,
    DATA_FOLDER_READ_ONLY_MESSAGE, DEFAULTS_ROUTE, DENY_FRAMING, ENCODING_ROUTE, ENCODINGS_ROUTE,
    HISTORY_ROUTE, IDLE_CHECK_INTERVAL, IDLE_LIMIT, INDEX_PATH, LANGUAGE_ROUTE, LANGUAGES_ROUTE,
    LOOPBACK_HOST, MAXIMUM_UPLOAD_BYTES, NO_FOLDERS_MESSAGE, NO_REFERRER, NO_SNIFF, NO_STORE,
    OPEN_ROUTE, OUTPUT_FOLDER_READ_ONLY_MESSAGE, OUTPUT_ROUTE, PREVIEW_ROUTE, QUIT_ROUTE,
    REMOVE_ROUTE, REPAIR_ROUTE, RESTORE_DEFAULTS_ROUTE, STATE_ROUTE, THEME_ATTRIBUTE,
    THEME_PLACEHOLDER, TOKEN_FRAGMENT, TOKEN_HEADER, UPDATE_CHECK_INTERVAL, UPDATE_CHECK_ROUTE,
    UPDATE_DOWNLOAD_ROUTE, UPDATE_INSTALL_ROUTE, UPDATE_RESTART_ROUTE, UPLOAD_ROUTE,
    WATCH_LOG_ROUTE,
};
use crate::defaults::{Defaults, DefaultsStore};
use crate::history::History;
use crate::instance;
use crate::launcher::Interface;
use crate::session::Session;
use crate::settings::{Mode, Settings};
use crate::update::{Package, UpdateState, detect_package};
use crate::watch::{WatchLog, watch_loop};

/// Shared by every request. Each lock is held only for quick changes, and never while
/// another is taken; reading files, detection, conversion and downloads run outside them.
#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub session: Arc<Mutex<Session>>,
    pub defaults: Arc<Mutex<DefaultsStore>>,
    pub history: Arc<Mutex<History>>,
    pub watch_log: Arc<Mutex<WatchLog>>,
    pub update: Arc<Mutex<UpdateState>>,
    /// SET-02: settings and the history can be kept.
    pub data_folder_writable: bool,
    pub interface: Option<Arc<Interface>>,
    pub shutdown: Arc<watch::Sender<bool>>,
    last_activity: Arc<Mutex<Instant>>,
}

/// A panic in one request must not lock everyone out, and the session holds no invariant
/// that a half-finished change could break.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl AppState {
    /// SET-02: reads the saved defaults and the history; the session starts from the defaults.
    pub fn new(
        settings: Settings,
        interface: Option<Arc<Interface>>,
        shutdown: Arc<watch::Sender<bool>>,
        package: Package,
    ) -> Self {
        let defaults = DefaultsStore::load(&settings.data_folder, Defaults::factory(&settings));
        let history = History::load(&settings.data_folder);
        let session = Session::new(defaults.current.session_settings());
        Self {
            data_folder_writable: folder_is_writable(&settings.data_folder),
            settings: Arc::new(settings),
            session: Arc::new(Mutex::new(session)),
            defaults: Arc::new(Mutex::new(defaults)),
            history: Arc::new(Mutex::new(history)),
            watch_log: Arc::new(Mutex::new(WatchLog::default())),
            update: Arc::new(Mutex::new(UpdateState::new(package))),
            interface,
            shutdown,
            last_activity: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub fn session(&self) -> MutexGuard<'_, Session> {
        lock(&self.session)
    }

    pub fn defaults(&self) -> MutexGuard<'_, DefaultsStore> {
        lock(&self.defaults)
    }

    pub fn history(&self) -> MutexGuard<'_, History> {
        lock(&self.history)
    }

    pub fn watch_log(&self) -> MutexGuard<'_, WatchLog> {
        lock(&self.watch_log)
    }

    pub fn update(&self) -> MutexGuard<'_, UpdateState> {
        lock(&self.update)
    }

    fn record_activity(&self) {
        *lock(&self.last_activity) = Instant::now();
    }

    fn idle_time(&self) -> Duration {
        lock(&self.last_activity).elapsed()
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route(STATE_ROUTE, get(api::state))
        .route(ENCODINGS_ROUTE, get(api::encodings))
        .route(LANGUAGES_ROUTE, get(api::languages))
        .route(BROWSE_ROUTE, post(api::browse))
        .route(ADD_ROUTE, post(api::add))
        .route(OPEN_ROUTE, post(api::open))
        .route(
            UPLOAD_ROUTE,
            post(api::upload).layer(DefaultBodyLimit::max(MAXIMUM_UPLOAD_BYTES)),
        )
        .route(PREVIEW_ROUTE, get(api::preview))
        .route(CANDIDATES_ROUTE, get(api::candidates))
        .route(ENCODING_ROUTE, post(api::choose_encoding))
        .route(LANGUAGE_ROUTE, post(api::set_file_language))
        .route(REPAIR_ROUTE, post(api::repair))
        .route(OUTPUT_ROUTE, get(api::output))
        .route(REMOVE_ROUTE, post(api::remove))
        .route(CLEAR_ROUTE, post(api::clear))
        .route(CONVERT_ROUTE, post(api::convert))
        .route(CANCEL_ROUTE, post(api::cancel))
        .route(QUIT_ROUTE, post(api::quit))
        .route(DEFAULTS_ROUTE, post(api::save_defaults))
        .route(RESTORE_DEFAULTS_ROUTE, post(api::restore_defaults))
        .route(HISTORY_ROUTE, post(api::history))
        .route(WATCH_LOG_ROUTE, get(api::watch_log))
        .route(UPDATE_CHECK_ROUTE, post(api::check_for_update))
        .route(UPDATE_DOWNLOAD_ROUTE, post(api::download_update))
        .route(UPDATE_INSTALL_ROUTE, post(api::install_update))
        .route(UPDATE_RESTART_ROUTE, post(api::restart_after_update))
        .route_layer(middleware::from_fn_with_state(state.clone(), check_access));
    Router::new()
        .nest(API_PREFIX, api)
        .fallback(get(serve_asset))
        .layer(middleware::from_fn_with_state(state.clone(), check_host))
        .layer(middleware::from_fn(add_security_headers))
        .with_state(state)
}

/// ACCESS-06.
async fn add_security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static(NO_SNIFF),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static(NO_REFERRER),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static(DENY_FRAMING),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    response
}

/// ACCESS-05: blocks DNS rebinding, where another site's name points at this machine.
async fn check_host(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !host_is_allowed(host, &state.settings.allowed_hosts) {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

/// ACCESS-04 and ACCESS-09: every data request carries the app's header, with the token on
/// the desktop, and counts as activity.
async fn check_access(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let header = request
        .headers()
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok());
    if !request_is_allowed(state.settings.token.as_deref(), header) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.record_activity();
    next.run(request).await
}

/// The interface files hold no user data, so they need no token (ACCESS-04). The page comes
/// with the saved theme already set (UI-13).
async fn serve_asset(State(app): State<AppState>, uri: Uri) -> Response {
    let Some((path, content_type, body)) = ASSETS.iter().find(|(path, ..)| *path == uri.path())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if *path != INDEX_PATH {
        return ([(header::CONTENT_TYPE, *content_type)], *body).into_response();
    }
    let theme = app.defaults().current.theme.attribute_value();
    let page = String::from_utf8_lossy(body).replacen(
        THEME_PLACEHOLDER,
        &format!("{THEME_ATTRIBUTE}=\"{theme}\""),
        1,
    );
    ([(header::CONTENT_TYPE, *content_type)], page).into_response()
}

pub async fn run(mut settings: Settings) -> io::Result<()> {
    let listener = TcpListener::bind(settings.listen_address).await?;
    let port = listener.local_addr()?.port();
    settings.listen_address.set_port(port);
    let (shutdown, _) = watch::channel(false);
    let shutdown = Arc::new(shutdown);
    let interface = settings.token.as_ref().map(|token| {
        let address = format!("http://{LOOPBACK_HOST}:{port}/{TOKEN_FRAGMENT}{token}");
        Interface::new(address, Arc::clone(&shutdown))
    });
    let files_to_open = settings.files_to_open.clone();
    let mode = settings.mode;
    let package = tokio::task::spawn_blocking(move || detect_package(mode))
        .await
        .unwrap_or(Package::Other);
    let state = AppState::new(settings, interface.clone(), Arc::clone(&shutdown), package);
    if !files_to_open.is_empty() {
        let _ = api::add_paths(&state, files_to_open, false).await;
    }
    match (&interface, &state.settings.token) {
        (Some(interface), Some(token)) => {
            let _ = instance::record(port, token);
            tokio::spawn(stop_when_idle(state.clone()));
            interface.show();
        }
        _ => {
            print_start_up(&state);
            tokio::spawn(watch_loop(state.clone()));
        }
    }
    if state.settings.update_check_allowed {
        tokio::spawn(check_for_updates_daily(state.clone()));
    }
    let served = axum::serve(listener, router(state.clone()))
        .with_graceful_shutdown(wait_for_shutdown(shutdown.subscribe()))
        .await;
    state.session().cancel_conversion();
    if state.settings.mode == Mode::Desktop {
        instance::remove(port);
    }
    served
}

/// ACCESS-07, TARGET-02, TARGET-04 and SET-02: Docker says where to open it, which folders
/// it can use, and which mounted folders it cannot write to.
fn print_start_up(state: &AppState) {
    let settings = &state.settings;
    println!("{CONTAINER_RUNNING_MESSAGE}");
    if !state.data_folder_writable {
        println!("{DATA_FOLDER_READ_ONLY_MESSAGE}");
    }
    let roots = settings.allowed_area.roots();
    if roots.is_empty() {
        println!("{NO_FOLDERS_MESSAGE}");
        return;
    }
    println!("{CONTAINER_FOLDERS_MESSAGE}");
    for root in roots {
        println!("  {}", root.display());
    }
    if settings.has_output_mount && !folder_is_writable(&settings.default_output_folder) {
        println!("{OUTPUT_FOLDER_READ_ONLY_MESSAGE}");
    }
}

/// UPDATE-01: at start and once a day, unless turned off in Settings.
async fn check_for_updates_daily(state: AppState) {
    let mut interval = tokio::time::interval(UPDATE_CHECK_INTERVAL);
    loop {
        interval.tick().await;
        if state.defaults().current.update_check {
            api::run_update_check(&state).await;
        }
    }
}

/// ACCESS-09: never during a conversion, and never while the app's own window is open.
async fn stop_when_idle(state: AppState) {
    let mut interval = tokio::time::interval(IDLE_CHECK_INTERVAL);
    loop {
        interval.tick().await;
        let is_converting = state.session().conversion.is_some();
        let window_open = state
            .interface
            .as_ref()
            .is_some_and(|interface| interface.is_window_open());
        if !is_converting && !window_open && state.idle_time() >= IDLE_LIMIT {
            let _ = state.shutdown.send(true);
            return;
        }
    }
}

async fn wait_for_shutdown(mut requested: watch::Receiver<bool>) {
    let mut terminate = signal(SignalKind::terminate()).ok();
    let terminated = async {
        match terminate.as_mut() {
            Some(terminate) => terminate.recv().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = requested.wait_for(|stop| *stop) => {}
        _ = tokio::signal::ctrl_c() => {}
        _ = terminated => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{LOCAL_HOST, call, route, send, test_app, test_app_with_token};
    use axum::body::Body;
    use axum::http::{Method, Request};

    fn plain_request(uri: &str, host: &str, token: Option<&str>) -> Request<Body> {
        let builder = Request::builder().uri(uri).header(header::HOST, host);
        let builder = match token {
            Some(token) => builder.header(TOKEN_HEADER, token),
            None => builder,
        };
        builder.body(Body::empty()).unwrap()
    }

    /// ACCESS-04.
    #[tokio::test]
    async fn desktop_request_without_token_is_rejected() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app_with_token(folder.path());
        let state = route(STATE_ROUTE);
        let (status, _, body) = send(&app, plain_request(&state, LOCAL_HOST, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.is_empty());
        let wrong = "f".repeat(crate::constants::TOKEN_BYTES * 2);
        let (status, ..) = send(&app, plain_request(&state, LOCAL_HOST, Some(&wrong))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(&app, Method::GET, &state, None).await;
        assert_eq!(status, StatusCode::OK);
    }

    /// ACCESS-04: Docker needs no token, but other websites cannot send the header.
    #[tokio::test]
    async fn docker_request_without_the_header_is_rejected() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let state = route(STATE_ROUTE);
        let (status, ..) = send(&app, plain_request(&state, LOCAL_HOST, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, ..) = send(&app, plain_request(&state, "192.168.0.10:61880", Some(""))).await;
        assert_eq!(status, StatusCode::OK);
    }

    /// ACCESS-05: also for the interface files, so a rebinding page cannot even load them.
    #[tokio::test]
    async fn request_for_another_host_is_rejected() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        let token = crate::test_support::TOKEN;
        for uri in ["/", route(STATE_ROUTE).as_str()] {
            let request = plain_request(uri, "rebind.example:61880", Some(token));
            assert_eq!(send(&app, request).await.0, StatusCode::FORBIDDEN, "{uri}");
        }
    }

    /// ACCESS-06.
    #[tokio::test]
    async fn responses_carry_security_headers() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_app(folder.path(), folder.path());
        for (uri, expected_status) in [
            ("/", StatusCode::OK),
            ("/missing.js", StatusCode::NOT_FOUND),
        ] {
            let (status, headers, _) = send(&app, plain_request(uri, LOCAL_HOST, None)).await;
            assert_eq!(status, expected_status, "{uri}");
            assert_eq!(
                headers[header::CONTENT_SECURITY_POLICY],
                CONTENT_SECURITY_POLICY
            );
            assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], NO_SNIFF);
            assert_eq!(headers[header::REFERRER_POLICY], NO_REFERRER);
            assert_eq!(headers[header::X_FRAME_OPTIONS], DENY_FRAMING);
            assert_eq!(headers[header::CACHE_CONTROL], NO_STORE);
        }
        let (_, headers, _) = send(&app, plain_request("/", LOCAL_HOST, None)).await;
        assert_eq!(
            headers[header::CONTENT_TYPE],
            crate::constants::HTML_CONTENT_TYPE
        );
    }
}
