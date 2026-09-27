use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use serde_json::Value;
use subutf8_core::constants::UTF8_BYTE_ORDER_MARK;
use subutf8_core::input_scan::AllowedArea;
use tokio::sync::watch;
use tower::ServiceExt;

use crate::access::allowed_hosts;
use crate::constants::{API_PREFIX, CONTAINER_ADDRESS, CONTAINER_PORT, STATE_ROUTE, TOKEN_HEADER};
use crate::server::{AppState, router};
use crate::settings::{Mode, Settings};

pub const TOKEN: &str = "0123456789abcdef0123456789abcdef";
pub const LOCAL_HOST: &str = "127.0.0.1:61880";
const JSON_CONTENT_TYPE: &str = "application/json";
const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../subutf8-core/tests/fixtures"
);
const WAIT_STEP: Duration = Duration::from_millis(20);
const WAIT_ATTEMPTS: usize = 500;

pub fn fixture_source(name: &str) -> PathBuf {
    Path::new(FIXTURES)
        .join("source")
        .join(format!("{name}.srt"))
}

/// ENC-14: what the fixture's converted output holds.
pub fn fixture_output(name: &str) -> String {
    let text = fs::read_to_string(
        Path::new(FIXTURES)
            .join("expected")
            .join(format!("{name}.txt")),
    )
    .unwrap();
    [UTF8_BYTE_ORDER_MARK, &text].concat()
}

/// The fixture names of the parity set, from `golden.sha256`.
pub fn parity_set() -> Vec<String> {
    fs::read_to_string(Path::new(FIXTURES).join("golden.sha256"))
        .unwrap()
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(|name| name.trim_end_matches(".srt").to_owned())
        .collect()
}

pub fn route(path: &str) -> String {
    format!("{API_PREFIX}{path}")
}

/// A Docker app whose mounted folders are `root` and `output`, writing dropped files to
/// `output`. It needs no token.
pub fn test_app(root: &Path, output: &Path) -> Router {
    app_with(None, root, output)
}

/// An app that, like the desktop one, requires `TOKEN`.
pub fn test_app_with_token(root: &Path) -> Router {
    app_with(Some(String::from(TOKEN)), root, root)
}

fn app_with(token: Option<String>, root: &Path, output: &Path) -> Router {
    let settings = Settings {
        mode: Mode::Container,
        token,
        allowed_hosts: allowed_hosts(&[]),
        listen_address: SocketAddr::from((Ipv4Addr::from(CONTAINER_ADDRESS), CONTAINER_PORT)),
        allowed_area: AllowedArea::new([root.to_path_buf(), output.to_path_buf()]),
        browse_start: root.to_path_buf(),
        default_output_folder: fs::canonicalize(output).unwrap(),
        files_to_open: Vec::new(),
    };
    let (shutdown, _) = watch::channel(false);
    router(AppState::new(settings, None, Arc::new(shutdown)))
}

pub async fn send(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, headers, body.to_vec())
}

pub fn api_request(method: Method, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, LOCAL_HOST)
        .header(TOKEN_HEADER, TOKEN)
        .header(header::CONTENT_TYPE, JSON_CONTENT_TYPE)
        .body(body)
        .unwrap()
}

pub async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    json: Option<Value>,
) -> (StatusCode, Value) {
    let body = json.map_or_else(Body::empty, |json| Body::from(json.to_string()));
    let (status, _, body) = send(app, api_request(method, uri, body)).await;
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

pub async fn wait_for_conversion(app: &Router) -> Value {
    for _ in 0..WAIT_ATTEMPTS {
        let (_, state) = call(app, Method::GET, &route(STATE_ROUTE), None).await;
        if state["conversion"].is_null() {
            return state;
        }
        tokio::time::sleep(WAIT_STEP).await;
    }
    panic!("the conversion did not finish");
}
