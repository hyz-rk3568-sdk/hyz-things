use std::{
    collections::HashMap,
    io::{self, Cursor, Read},
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, on, MethodFilter},
    Json, Router,
};
use serde::Serialize;

use crate::{
    adapters::inbound::control::{
        ControlHandler, ControlOperation, ControlProxyMode, ControlResult,
    },
    application::status::ReadStatus,
    domain::{
        panel::{
            DisplayRequest, PanelBootstrap, ProxyDelayRefreshRequest, ProxyDelayRequest,
            ProxyModeRequest, ProxySelectionRequest,
        },
        status::ProxyMode,
    },
};

const EMBEDDED_FRONTEND: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/frontend.tar"));
const CSP: &str = "default-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'";
pub const LAN_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 8, 1);
pub const DEFAULT_HTTP_PORT: u16 = 8080;
pub const DEFAULT_BIND_ATTEMPTS: usize = 15;

#[derive(Clone)]
struct AppState {
    read_status: ReadStatus,
    control: Option<Arc<dyn ControlHandler>>,
    csrf_token: Arc<str>,
    allowed_origin: Arc<str>,
    assets: AssetStore,
}

#[derive(Clone)]
struct AssetStore {
    files: Arc<HashMap<String, Arc<[u8]>>>,
}

impl AssetStore {
    fn embedded() -> io::Result<Self> {
        Self::from_tar(EMBEDDED_FRONTEND)
    }

    fn from_tar(bytes: &[u8]) -> io::Result<Self> {
        let mut files = HashMap::new();
        for item in tar::Archive::new(Cursor::new(bytes)).entries()? {
            let mut item = item?;
            if !item.header().entry_type().is_file() {
                continue;
            }
            let path = item.path()?.to_string_lossy().replace('\\', "/");
            let path = path.trim_start_matches("./").to_owned();
            if !safe_asset_path(&path) {
                continue;
            }
            let mut contents = Vec::new();
            item.read_to_end(&mut contents)?;
            files.insert(path, Arc::from(contents));
        }
        if !files.contains_key("index.html") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "embedded frontend has no index.html",
            ));
        }
        Ok(Self {
            files: Arc::new(files),
        })
    }

    fn get(&self, path: &str) -> Option<Arc<[u8]>> {
        self.files.get(path).cloned()
    }
}

pub fn app(read_status: ReadStatus) -> Router {
    let assets = AssetStore::embedded().expect("build script must embed a valid frontend archive");
    app_with_assets(read_status, None, "", DEFAULT_HTTP_PORT, assets)
}

pub fn app_with_control(
    read_status: ReadStatus,
    control: Arc<dyn ControlHandler>,
    csrf_token: String,
    port: u16,
) -> Router {
    let assets = AssetStore::embedded().expect("build script must embed a valid frontend archive");
    app_with_assets(read_status, Some(control), &csrf_token, port, assets)
}

fn app_with_assets(
    read_status: ReadStatus,
    control: Option<Arc<dyn ControlHandler>>,
    csrf_token: &str,
    port: u16,
    assets: AssetStore,
) -> Router {
    Router::new()
        .route("/api/v1/health", on(MethodFilter::GET, health))
        .route("/api/v1/status", on(MethodFilter::GET, status))
        .route("/api/v1/panel", on(MethodFilter::GET, panel))
        .route(
            "/api/v1/control/display",
            on(MethodFilter::POST, control_display),
        )
        .route(
            "/api/v1/control/proxy/mode",
            on(MethodFilter::POST, control_proxy_mode),
        )
        .route(
            "/api/v1/control/proxy/selection",
            on(MethodFilter::POST, control_proxy_selection),
        )
        .route(
            "/api/v1/control/proxy/delay",
            on(MethodFilter::POST, control_proxy_delay),
        )
        .route(
            "/api/v1/control/proxy/delays",
            on(MethodFilter::POST, control_proxy_delay_refresh),
        )
        .route("/", get(frontend_root))
        .route("/{*path}", get(frontend_asset))
        .fallback(api_or_method_not_found)
        .layer(DefaultBodyLimit::max(4 * 1024))
        .layer(middleware::from_fn(security_headers))
        .with_state(AppState {
            read_status,
            control,
            csrf_token: Arc::from(csrf_token),
            allowed_origin: Arc::from(format!("http://{LAN_ADDRESS}:{port}")),
            assets,
        })
}

pub fn fixed_lan_address(port: u16) -> io::Result<SocketAddr> {
    if port == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTP port must be non-zero",
        ));
    }
    Ok(SocketAddr::from((LAN_ADDRESS, port)))
}

pub async fn bind_fixed_lan_with_retry(
    port: u16,
    attempts: usize,
    retry_delay: Duration,
) -> io::Result<tokio::net::TcpListener> {
    if attempts == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bind attempts must be non-zero",
        ));
    }
    let address = fixed_lan_address(port)?;
    let mut last_error = None;
    for attempt in 1..=attempts {
        match tokio::net::TcpListener::bind(address).await {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == io::ErrorKind::AddrNotAvailable => {
                last_error = Some(error);
                if attempt < attempts {
                    tokio::time::sleep(retry_delay).await;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "LAN address is unavailable",
        )
    }))
}

#[derive(Serialize)]
struct Health<'a> {
    status: &'a str,
    check: &'a str,
    readiness_assessed: bool,
    service: &'a str,
    version: &'a str,
}

async fn health() -> Json<Health<'static>> {
    Json(Health {
        status: "alive",
        check: "liveness",
        readiness_assessed: false,
        service: "hyz-router",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.read_status.execute().await)
}

async fn panel(State(state): State<AppState>) -> Response {
    let Some(control) = &state.control else {
        return service_unavailable_json();
    };
    match control.handle(ControlOperation::PanelStatus {}).await {
        Ok(ControlResult::PanelStatus { snapshot }) => Json(PanelBootstrap {
            csrf_token: state.csrf_token.to_string(),
            panel: *snapshot,
        })
        .into_response(),
        Ok(_) | Err(_) => service_unavailable_json(),
    }
}

async fn control_display(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DisplayRequest>,
) -> Response {
    invoke_control(&state, &headers, ControlOperation::Display { request }).await
}

async fn control_proxy_mode(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProxyModeRequest>,
) -> Response {
    let mode = match request.mode {
        ProxyMode::Explicit => ControlProxyMode::Explicit,
        ProxyMode::Tun => ControlProxyMode::Tun,
        ProxyMode::Disabled => ControlProxyMode::Disabled,
        ProxyMode::Unknown => return invalid_request_json(),
    };
    invoke_control(&state, &headers, ControlOperation::Proxy { mode }).await
}

async fn control_proxy_selection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProxySelectionRequest>,
) -> Response {
    invoke_control(
        &state,
        &headers,
        ControlOperation::ProxySelection { request },
    )
    .await
}

async fn control_proxy_delay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProxyDelayRequest>,
) -> Response {
    invoke_control(&state, &headers, ControlOperation::ProxyDelay { request }).await
}

async fn control_proxy_delay_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProxyDelayRefreshRequest>,
) -> Response {
    invoke_control(
        &state,
        &headers,
        ControlOperation::ProxyDelayRefresh { request },
    )
    .await
}

async fn invoke_control(
    state: &AppState,
    headers: &HeaderMap,
    operation: ControlOperation,
) -> Response {
    if !authorize_control(state, headers) {
        return forbidden_json();
    }
    if operation.validate().is_err() {
        return invalid_request_json();
    }
    let Some(control) = &state.control else {
        return service_unavailable_json();
    };
    match control.handle(operation).await {
        Ok(result @ ControlResult::Completed { .. })
        | Ok(result @ ControlResult::ProxyDelay { .. })
        | Ok(result @ ControlResult::ProxyDelays { .. }) => Json(result).into_response(),
        Ok(_) => service_unavailable_json(),
        Err(_) => control_failed_json(),
    }
}

fn authorize_control(state: &AppState, headers: &HeaderMap) -> bool {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let fetch_site = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok());
    let token = headers
        .get("x-hyz-csrf")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim() == "application/json")
        && origin == state.allowed_origin.as_ref()
        && fetch_site.is_none_or(|value| value == "same-origin")
        && constant_time_equal(token.as_bytes(), state.csrf_token.as_bytes())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

async fn frontend_root(State(state): State<AppState>) -> Response {
    asset_response(&state.assets, "index.html", true)
}

async fn frontend_asset(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    if path == "api" || path.starts_with("api/") {
        return not_found_json();
    }
    if !safe_asset_path(&path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if state.assets.get(&path).is_some() {
        asset_response(&state.assets, &path, path == "router-bootstrap.js")
    } else if static_asset_path(&path) {
        StatusCode::NOT_FOUND.into_response()
    } else {
        asset_response(&state.assets, "index.html", true)
    }
}

async fn api_or_method_not_found(request: Request) -> Response {
    if request.uri().path().starts_with("/api/") {
        if request.method() == Method::GET {
            not_found_json()
        } else {
            method_not_allowed_json()
        }
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

fn forbidden_json() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": { "code": "forbidden", "message": "Control request rejected" }
        })),
    )
        .into_response()
}

fn invalid_request_json() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": { "code": "invalid_request", "message": "Control request is invalid" }
        })),
    )
        .into_response()
}

fn control_failed_json() -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": { "code": "control_failed", "message": "Control operation did not complete" }
        })),
    )
        .into_response()
}

fn service_unavailable_json() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": { "code": "service_unavailable", "message": "Control service is unavailable" }
        })),
    )
        .into_response()
}

fn not_found_json() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": { "code": "not_found", "message": "Resource not found" }
        })),
    )
        .into_response()
}

fn method_not_allowed_json() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "GET, POST")],
        Json(serde_json::json!({
            "error": { "code": "method_not_allowed", "message": "Method is not allowed" }
        })),
    )
        .into_response()
}

fn asset_response(assets: &AssetStore, path: &str, no_cache: bool) -> Response {
    let Some(contents) = assets.get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = Response::new(Body::from(contents.to_vec()));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if no_cache {
            "no-store"
        } else {
            "public, max-age=3600"
        }),
    );
    response
}

fn safe_asset_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn static_asset_path(path: &str) -> bool {
    matches!(
        path.rsplit_once('.').map(|(_, extension)| extension),
        Some(
            "css"
                | "gif"
                | "ico"
                | "jpeg"
                | "jpg"
                | "js"
                | "json"
                | "map"
                | "png"
                | "svg"
                | "wasm"
                | "webp"
                | "woff"
                | "woff2"
        )
    )
}

fn is_control_path(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/control/display"
            | "/api/v1/control/proxy/mode"
            | "/api/v1/control/proxy/selection"
            | "/api/v1/control/proxy/delay"
            | "/api/v1/control/proxy/delays"
    )
}

async fn security_headers(request: Request, next: Next) -> Response {
    // The status surface remains GET-only. Only five exact control paths accept POST, and each
    // handler independently requires JSON, an exact same-origin Origin, and the per-daemon token.
    let path = request.uri().path();
    let allowed = request.method() == Method::GET
        || request.method() == Method::POST && is_control_path(path);
    let reject_api_method = !allowed && path.starts_with("/api/");
    let mut response = if reject_api_method {
        method_not_allowed_json()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert("content-security-policy", HeaderValue::from_static(CSP));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
    );
    headers.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "x-permitted-cross-domain-policies",
        HeaderValue::from_static("none"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_binding_never_uses_a_wildcard_address() {
        assert_eq!(
            fixed_lan_address(DEFAULT_HTTP_PORT).unwrap(),
            SocketAddr::from((LAN_ADDRESS, DEFAULT_HTTP_PORT))
        );
        assert!(fixed_lan_address(0).is_err());
    }

    #[test]
    fn rejects_unsafe_asset_paths_and_identifies_missing_static_assets() {
        assert!(safe_asset_path("assets/app.js"));
        assert!(!safe_asset_path("../secret"));
        assert!(!safe_asset_path("assets//app.js"));
        assert!(!safe_asset_path("/absolute"));
        assert!(static_asset_path("router-old.js"));
        assert!(static_asset_path("router-old_bg.wasm"));
        assert!(static_asset_path("styles-old.css"));
        assert!(!static_asset_path("dashboard/route"));
    }
}
