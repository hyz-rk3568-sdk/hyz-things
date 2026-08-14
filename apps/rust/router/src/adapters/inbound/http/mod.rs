use std::{
    collections::HashMap,
    io::{self, Cursor, Read},
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use axum::{
    body::Body,
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, on, MethodFilter},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    adapters::inbound::control::{
        ControlHandler, ControlOperation, ControlProxyMode, ControlResult,
    },
    application::{
        admin::{AdminApplication, AdminError},
        status::ReadStatus,
        wifi::{ApPrepareRequest, StaCandidateRequest, WifiScanEntry},
    },
    domain::{
        admin::{AdminAuthorization, AdminLoginRequest, AdminPasswordChangeRequest, SecretString},
        device_policy::DevicePolicyUpdateRequest,
        network_config::{NetworkConfigSummary, PendingNetworkConfigSummary},
        panel::{
            DisplayRequest, PanelBootstrap, ProxyDelayRefreshRequest, ProxyDelayRequest,
            ProxyModeRequest, ProxySelectionRequest,
        },
        status::{ProxyMode, TailscaleStatus},
        subscription::SubscriptionSummary,
        tailscale::TailscaleMode,
    },
};

const EMBEDDED_FRONTEND: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/frontend.tar"));
const CSP: &str = "default-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'";
pub const LAN_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 8, 1);
pub const DEFAULT_HTTP_PORT: u16 = 8080;
pub const DEFAULT_BIND_ATTEMPTS: usize = 15;
pub const ADMIN_SESSION_COOKIE: &str = "hyz_admin_session";
const MAX_HTTP_JSON_BODY_BYTES: usize = 4 * 1024;

#[derive(Clone)]
struct AppState {
    read_status: ReadStatus,
    control: Option<Arc<dyn ControlHandler>>,
    admin: Option<Arc<AdminApplication>>,
    csrf_token: Arc<str>,
    allowed_origin: Arc<str>,
    allow_tailscale_self_stop: bool,
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
    app_with_assets(
        read_status,
        None,
        None,
        "",
        format!("http://{LAN_ADDRESS}:{DEFAULT_HTTP_PORT}"),
        true,
        assets,
    )
}

pub fn app_with_control(
    read_status: ReadStatus,
    control: Arc<dyn ControlHandler>,
    csrf_token: String,
    port: u16,
) -> Router {
    let assets = AssetStore::embedded().expect("build script must embed a valid frontend archive");
    app_with_assets(
        read_status,
        Some(control),
        None,
        &csrf_token,
        format!("http://{LAN_ADDRESS}:{port}"),
        true,
        assets,
    )
}

pub fn app_with_admin_control(
    read_status: ReadStatus,
    control: Arc<dyn ControlHandler>,
    admin: Arc<AdminApplication>,
    csrf_token: String,
    port: u16,
) -> Router {
    let assets = AssetStore::embedded().expect("build script must embed a valid frontend archive");
    app_with_assets(
        read_status,
        Some(control),
        Some(admin),
        &csrf_token,
        format!("http://{LAN_ADDRESS}:{port}"),
        true,
        assets,
    )
}

pub fn app_with_admin_control_at_address(
    read_status: ReadStatus,
    control: Arc<dyn ControlHandler>,
    admin: Arc<AdminApplication>,
    csrf_token: String,
    address: Ipv4Addr,
    port: u16,
) -> Router {
    let assets = AssetStore::embedded().expect("build script must embed a valid frontend archive");
    app_with_assets(
        read_status,
        Some(control),
        Some(admin),
        &csrf_token,
        format!("http://{address}:{port}"),
        false,
        assets,
    )
}

#[cfg(feature = "e2e")]
pub fn app_with_loopback_runtime_frontend(
    read_status: ReadStatus,
    control: Arc<dyn ControlHandler>,
    admin: Arc<AdminApplication>,
    csrf_token: String,
    exact_loopback_origin: String,
    frontend_tar: &[u8],
) -> io::Result<Router> {
    validate_exact_loopback_origin(&exact_loopback_origin)?;
    let assets = AssetStore::from_tar(frontend_tar)?;
    Ok(app_with_assets(
        read_status,
        Some(control),
        Some(admin),
        &csrf_token,
        exact_loopback_origin,
        true,
        assets,
    ))
}

#[cfg(feature = "e2e")]
fn validate_exact_loopback_origin(origin: &str) -> io::Result<()> {
    let parsed = url::Url::parse(origin).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "e2e HTTP origin must be an exact loopback origin",
        )
    })?;
    let valid = parsed.scheme() == "http"
        && parsed.host_str() == Some("127.0.0.1")
        && parsed.port().is_some_and(|port| port != 0)
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.path() == "/"
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && origin == format!("http://127.0.0.1:{}", parsed.port().unwrap());
    if valid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "e2e HTTP origin must be exactly http://127.0.0.1:<non-zero-port>",
        ))
    }
}

fn app_with_assets(
    read_status: ReadStatus,
    control: Option<Arc<dyn ControlHandler>>,
    admin: Option<Arc<AdminApplication>>,
    csrf_token: &str,
    allowed_origin: String,
    allow_tailscale_self_stop: bool,
    assets: AssetStore,
) -> Router {
    Router::new()
        .route("/api/v1/health", on(MethodFilter::GET, health))
        .route("/api/v1/status", on(MethodFilter::GET, status))
        .route("/api/v1/panel", on(MethodFilter::GET, panel))
        .route("/api/v1/auth/login", on(MethodFilter::POST, admin_login))
        .route("/api/v1/auth/logout", on(MethodFilter::POST, admin_logout))
        .route(
            "/api/v1/auth/password",
            on(MethodFilter::POST, admin_password),
        )
        .route("/api/v1/auth/session", on(MethodFilter::GET, admin_session))
        .route(
            "/api/v1/network/config",
            on(MethodFilter::GET, network_config),
        )
        .route(
            "/api/v1/network/pending",
            on(MethodFilter::GET, network_pending),
        )
        .route(
            "/api/v1/control/network/sta/scan",
            on(MethodFilter::POST, network_sta_scan),
        )
        .route(
            "/api/v1/control/network/sta/apply",
            on(MethodFilter::POST, network_sta_apply),
        )
        .route(
            "/api/v1/control/network/ap/prepare",
            on(MethodFilter::POST, network_ap_prepare),
        )
        .route(
            "/api/v1/control/network/ap/apply",
            on(MethodFilter::POST, network_ap_apply),
        )
        .route(
            "/api/v1/control/network/ap/confirm",
            on(MethodFilter::POST, network_ap_confirm),
        )
        .route(
            "/api/v1/control/network/ap/cancel",
            on(MethodFilter::POST, network_ap_cancel),
        )
        .route(
            "/api/v1/proxy/device-policies",
            on(MethodFilter::GET, proxy_device_policies),
        )
        .route(
            "/api/v1/control/proxy/device-policies",
            on(MethodFilter::POST, proxy_device_policies_update),
        )
        .route(
            "/api/v1/proxy/subscription",
            on(MethodFilter::GET, proxy_subscription),
        )
        .route("/api/v1/tailscale", on(MethodFilter::GET, tailscale_status))
        .route(
            "/api/v1/control/tailscale/mode",
            on(MethodFilter::POST, tailscale_mode),
        )
        .route(
            "/api/v1/control/tailscale/login",
            on(MethodFilter::POST, tailscale_login),
        )
        .route(
            "/api/v1/control/tailscale/logout",
            on(MethodFilter::POST, tailscale_logout),
        )
        .route(
            "/api/v1/control/proxy/subscription/source",
            on(MethodFilter::POST, proxy_subscription_source),
        )
        .route(
            "/api/v1/control/proxy/subscription/refresh",
            on(MethodFilter::POST, proxy_subscription_refresh),
        )
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
        .layer(DefaultBodyLimit::max(MAX_HTTP_JSON_BODY_BYTES))
        .layer(middleware::from_fn(security_headers))
        .with_state(AppState {
            read_status,
            control,
            admin,
            csrf_token: Arc::from(csrf_token),
            allowed_origin: Arc::from(allowed_origin),
            allow_tailscale_self_stop,
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

#[derive(Serialize)]
struct AdminSessionResponse {
    authenticated: bool,
    must_change: bool,
}

pub enum AdminRequirement {
    Normal,
    PasswordChangeSession,
}

async fn admin_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<AdminLoginRequest>, JsonRejection>,
) -> Response {
    if !authorize_same_origin_csrf(&state, &headers) {
        return authentication_error_json(StatusCode::FORBIDDEN);
    }
    let Ok(Json(request)) = payload else {
        return authentication_error_json(StatusCode::BAD_REQUEST);
    };
    let Some(admin) = state.admin.clone() else {
        return authentication_error_json(StatusCode::SERVICE_UNAVAILABLE);
    };
    let result = tokio::task::spawn_blocking(move || admin.login(&request)).await;
    let login = match result {
        Ok(Ok(login)) => login,
        Ok(Err(error)) => return admin_error_json(error),
        Err(_) => return authentication_error_json(StatusCode::SERVICE_UNAVAILABLE),
    };
    let cookie = match session_cookie(login.token.expose()) {
        Some(cookie) => cookie,
        None => return authentication_error_json(StatusCode::SERVICE_UNAVAILABLE),
    };
    let mut response = Json(AdminSessionResponse {
        authenticated: true,
        must_change: login.must_change_password,
    })
    .into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
}

async fn admin_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorize_same_origin_csrf(&state, &headers) {
        return authentication_error_json(StatusCode::FORBIDDEN);
    }
    let token = match require_admin(&state, &headers, AdminRequirement::PasswordChangeSession).await
    {
        Ok((token, _)) => token,
        Err(response) => return response,
    };
    let Some(admin) = state.admin.clone() else {
        return authentication_error_json(StatusCode::SERVICE_UNAVAILABLE);
    };
    let result = tokio::task::spawn_blocking(move || admin.logout(&token)).await;
    match result {
        Ok(Ok(())) => {
            let mut response = Json(AdminSessionResponse {
                authenticated: false,
                must_change: false,
            })
            .into_response();
            response
                .headers_mut()
                .insert(header::SET_COOKIE, expired_session_cookie());
            response
        }
        Ok(Err(error)) => admin_error_json(error),
        Err(_) => authentication_error_json(StatusCode::SERVICE_UNAVAILABLE),
    }
}

async fn admin_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<AdminPasswordChangeRequest>, JsonRejection>,
) -> Response {
    if !authorize_same_origin_csrf(&state, &headers) {
        return authentication_error_json(StatusCode::FORBIDDEN);
    }
    let Ok(Json(request)) = payload else {
        return authentication_error_json(StatusCode::BAD_REQUEST);
    };
    let token = match require_admin(&state, &headers, AdminRequirement::PasswordChangeSession).await
    {
        Ok((token, _)) => token,
        Err(response) => return response,
    };
    let Some(admin) = state.admin.clone() else {
        return authentication_error_json(StatusCode::SERVICE_UNAVAILABLE);
    };
    let result = tokio::task::spawn_blocking(move || admin.change_password(&token, &request)).await;
    match result {
        Ok(Ok(())) => Json(AdminSessionResponse {
            authenticated: true,
            must_change: false,
        })
        .into_response(),
        Ok(Err(error)) => admin_error_json(error),
        Err(_) => authentication_error_json(StatusCode::SERVICE_UNAVAILABLE),
    }
}

async fn admin_session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if state.admin.is_none() {
        return authentication_error_json(StatusCode::SERVICE_UNAVAILABLE);
    }
    let Some(token) = admin_session_token(&headers) else {
        return Json(AdminSessionResponse {
            authenticated: false,
            must_change: false,
        })
        .into_response();
    };
    let Some(admin) = state.admin.clone() else {
        return authentication_error_json(StatusCode::SERVICE_UNAVAILABLE);
    };
    let result = tokio::task::spawn_blocking(move || admin.session(&token)).await;
    match result {
        Ok(Ok(authorization)) => Json(AdminSessionResponse {
            authenticated: true,
            must_change: authorization.must_change_password,
        })
        .into_response(),
        Ok(Err(AdminError::InvalidSession)) => Json(AdminSessionResponse {
            authenticated: false,
            must_change: false,
        })
        .into_response(),
        Ok(Err(error)) => admin_error_json(error),
        Err(_) => authentication_error_json(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// Shared authorization boundary for future sensitive administrator routes. Normal routes reject a
/// bootstrap session; only password-change and logout flows may request `PasswordChangeSession`.
async fn require_admin(
    state: &AppState,
    headers: &HeaderMap,
    requirement: AdminRequirement,
) -> Result<(SecretString, AdminAuthorization), Response> {
    let token = admin_session_token(headers)
        .ok_or_else(|| authentication_error_json(StatusCode::UNAUTHORIZED))?;
    let admin = state
        .admin
        .clone()
        .ok_or_else(|| authentication_error_json(StatusCode::SERVICE_UNAVAILABLE))?;
    let token_for_worker = SecretString::new(token.expose());
    let result = tokio::task::spawn_blocking(move || match requirement {
        AdminRequirement::Normal => admin.authorize(&token_for_worker),
        AdminRequirement::PasswordChangeSession => admin.session(&token_for_worker),
    })
    .await;
    match result {
        Ok(Ok(authorization)) => Ok((token, authorization)),
        Ok(Err(error)) => Err(admin_error_json(error)),
        Err(_) => Err(authentication_error_json(StatusCode::SERVICE_UNAVAILABLE)),
    }
}

fn admin_session_token(headers: &HeaderMap) -> Option<SecretString> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE).iter() {
        let value = value.to_str().ok()?;
        for cookie in value.split(';') {
            let Some((name, value)) = cookie.trim().split_once('=') else {
                continue;
            };
            if name != ADMIN_SESSION_COOKIE {
                continue;
            }
            if found.is_some()
                || value.len() != 64
                || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return None;
            }
            found = Some(SecretString::new(value));
        }
    }
    found
}

fn session_cookie(token: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{ADMIN_SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/"
    ))
    .ok()
}

fn expired_session_cookie() -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{ADMIN_SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
    ))
    .expect("fixed administrator cookie attributes must be a valid header")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyJsonRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionSourceRequest {
    url: SecretString,
}

#[derive(Serialize)]
struct NetworkConfigResponse {
    config: NetworkConfigSummary,
}

#[derive(Serialize)]
struct NetworkPendingResponse {
    pending: Option<PendingNetworkConfigSummary>,
    applied: bool,
    remaining_seconds: Option<u64>,
}

#[derive(Serialize)]
struct NetworkScanResponse {
    entries: Vec<WifiScanEntry>,
}

#[derive(Serialize)]
struct SubscriptionResponse {
    subscription: SubscriptionSummary,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TailscaleModeRequest {
    mode: TailscaleMode,
}

#[derive(Serialize)]
struct TailscaleResponse {
    tailscale: TailscaleStatus,
}

#[derive(Serialize)]
struct TailscaleMutationResponse {
    tailscale: TailscaleStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    login_url: Option<String>,
}

#[derive(Clone, Copy)]
enum SensitiveResult {
    NetworkConfig,
    NetworkPending,
    NetworkPendingStatus,
    NetworkScan,
    DevicePolicies,
    Subscription,
    Tailscale,
}

async fn network_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    invoke_sensitive_control(
        &state,
        &headers,
        ControlOperation::WifiStatus {},
        SensitiveResult::NetworkConfig,
    )
    .await
}

async fn network_pending(State(state): State<AppState>, headers: HeaderMap) -> Response {
    invoke_sensitive_control(
        &state,
        &headers,
        ControlOperation::WifiPending {},
        SensitiveResult::NetworkPendingStatus,
    )
    .await
}

async fn network_sta_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    invoke_empty_sensitive_control(
        &state,
        &headers,
        payload,
        ControlOperation::WifiScan {},
        SensitiveResult::NetworkScan,
    )
    .await
}

async fn network_sta_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<StaCandidateRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Ok(Json(request)) = payload else {
        return invalid_request_json();
    };
    invoke_sensitive_control_authorized(
        &state,
        ControlOperation::WifiStaApply { request },
        SensitiveResult::NetworkConfig,
    )
    .await
}

async fn network_ap_prepare(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ApPrepareRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Ok(Json(request)) = payload else {
        return invalid_request_json();
    };
    invoke_sensitive_control_authorized(
        &state,
        ControlOperation::WifiApPrepare { request },
        SensitiveResult::NetworkPending,
    )
    .await
}

async fn network_ap_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    invoke_empty_sensitive_control(
        &state,
        &headers,
        payload,
        ControlOperation::WifiApApply {},
        SensitiveResult::NetworkPending,
    )
    .await
}

async fn network_ap_confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    invoke_empty_sensitive_control(
        &state,
        &headers,
        payload,
        ControlOperation::WifiApConfirm {},
        SensitiveResult::NetworkConfig,
    )
    .await
}

async fn network_ap_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    invoke_empty_sensitive_control(
        &state,
        &headers,
        payload,
        ControlOperation::WifiApCancel {},
        SensitiveResult::NetworkConfig,
    )
    .await
}

async fn proxy_device_policies(State(state): State<AppState>, headers: HeaderMap) -> Response {
    invoke_sensitive_control(
        &state,
        &headers,
        ControlOperation::DevicePoliciesGet {},
        SensitiveResult::DevicePolicies,
    )
    .await
}

async fn proxy_device_policies_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<DevicePolicyUpdateRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Ok(Json(request)) = payload else {
        return invalid_request_json();
    };
    invoke_sensitive_control_authorized(
        &state,
        ControlOperation::DevicePoliciesSet { request },
        SensitiveResult::DevicePolicies,
    )
    .await
}

async fn proxy_subscription(State(state): State<AppState>, headers: HeaderMap) -> Response {
    invoke_sensitive_control(
        &state,
        &headers,
        ControlOperation::SubscriptionGet {},
        SensitiveResult::Subscription,
    )
    .await
}

async fn tailscale_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    invoke_sensitive_control(
        &state,
        &headers,
        ControlOperation::TailscaleGet {},
        SensitiveResult::Tailscale,
    )
    .await
}

async fn tailscale_mode(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<TailscaleModeRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Ok(Json(request)) = payload else {
        return invalid_request_json();
    };
    if !state.allow_tailscale_self_stop && request.mode == TailscaleMode::Disabled {
        return tailscale_self_stop_conflict_json();
    }
    invoke_tailscale_mutation_authorized(
        &state,
        ControlOperation::TailscaleMode { mode: request.mode },
    )
    .await
}

async fn tailscale_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    invoke_empty_tailscale_mutation(
        &state,
        &headers,
        payload,
        ControlOperation::TailscaleLogin {},
    )
    .await
}

async fn tailscale_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    if payload.is_err() {
        return invalid_request_json();
    }
    if !state.allow_tailscale_self_stop {
        return tailscale_self_stop_conflict_json();
    }
    invoke_tailscale_mutation_authorized(&state, ControlOperation::TailscaleLogout {}).await
}

async fn invoke_empty_tailscale_mutation(
    state: &AppState,
    headers: &HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
    operation: ControlOperation,
) -> Response {
    if let Err(response) = authorize_sensitive_control(state, headers).await {
        return response;
    }
    if payload.is_err() {
        return invalid_request_json();
    }
    invoke_tailscale_mutation_authorized(state, operation).await
}

async fn invoke_tailscale_mutation_authorized(
    state: &AppState,
    operation: ControlOperation,
) -> Response {
    if operation.validate().is_err() {
        return invalid_request_json();
    }
    let Some(control) = &state.control else {
        return service_unavailable_json();
    };
    match control.handle(operation).await {
        Ok(ControlResult::TailscaleMutation { status, login_url }) => {
            Json(TailscaleMutationResponse {
                tailscale: status,
                login_url: login_url.map(|url| url.as_str().to_owned()),
            })
            .into_response()
        }
        Ok(_) => service_unavailable_json(),
        Err(_) => control_failed_json(),
    }
}

async fn proxy_subscription_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<SubscriptionSourceRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Ok(Json(request)) = payload else {
        return invalid_request_json();
    };
    invoke_sensitive_control_authorized(
        &state,
        ControlOperation::SubscriptionSet { url: request.url },
        SensitiveResult::Subscription,
    )
    .await
}

async fn proxy_subscription_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    invoke_empty_sensitive_control(
        &state,
        &headers,
        payload,
        ControlOperation::SubscriptionRefresh {},
        SensitiveResult::Subscription,
    )
    .await
}

async fn invoke_empty_sensitive_control(
    state: &AppState,
    headers: &HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
    operation: ControlOperation,
    expected: SensitiveResult,
) -> Response {
    if let Err(response) = authorize_sensitive_control(state, headers).await {
        return response;
    }
    if payload.is_err() {
        return invalid_request_json();
    }
    invoke_sensitive_control_authorized(state, operation, expected).await
}

async fn invoke_sensitive_control(
    state: &AppState,
    headers: &HeaderMap,
    operation: ControlOperation,
    expected: SensitiveResult,
) -> Response {
    if let Err(response) = authorize_sensitive_read(state, headers).await {
        return response;
    }
    invoke_sensitive_control_authorized(state, operation, expected).await
}

async fn authorize_sensitive_read(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let fetch_site = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok());
    if fetch_site.is_some_and(|value| value != "same-origin") {
        return Err(forbidden_json());
    }
    require_admin(state, headers, AdminRequirement::Normal)
        .await
        .map(|_| ())
}

async fn authorize_sensitive_control(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), Response> {
    if !authorize_same_origin_csrf(state, headers) {
        return Err(forbidden_json());
    }
    require_admin(state, headers, AdminRequirement::Normal)
        .await
        .map(|_| ())
}

async fn invoke_sensitive_control_authorized(
    state: &AppState,
    operation: ControlOperation,
    expected: SensitiveResult,
) -> Response {
    if operation.validate().is_err() {
        return invalid_request_json();
    }
    let Some(control) = &state.control else {
        return service_unavailable_json();
    };
    match (expected, control.handle(operation).await) {
        (SensitiveResult::NetworkConfig, Ok(ControlResult::WifiConfig { config })) => {
            Json(NetworkConfigResponse { config }).into_response()
        }
        (SensitiveResult::NetworkPending, Ok(ControlResult::WifiPending { pending })) => {
            Json(NetworkPendingResponse {
                pending: Some(pending),
                applied: false,
                remaining_seconds: None,
            })
            .into_response()
        }
        (
            SensitiveResult::NetworkPendingStatus,
            Ok(ControlResult::WifiPendingStatus {
                pending,
                applied,
                remaining_seconds,
            }),
        ) => Json(NetworkPendingResponse {
            pending,
            applied,
            remaining_seconds,
        })
        .into_response(),
        (SensitiveResult::NetworkScan, Ok(ControlResult::WifiScan { entries })) => {
            Json(NetworkScanResponse { entries }).into_response()
        }
        (SensitiveResult::DevicePolicies, Ok(ControlResult::DevicePolicies { snapshot })) => {
            Json(snapshot).into_response()
        }
        (SensitiveResult::Subscription, Ok(ControlResult::Subscription { summary })) => {
            Json(SubscriptionResponse {
                subscription: summary,
            })
            .into_response()
        }
        (SensitiveResult::Tailscale, Ok(ControlResult::Tailscale { status })) => {
            Json(TailscaleResponse { tailscale: status }).into_response()
        }
        (_, Ok(_)) => service_unavailable_json(),
        (_, Err(_)) => control_failed_json(),
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
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let mode = match request.mode {
        ProxyMode::Explicit => ControlProxyMode::Explicit,
        ProxyMode::Tun => ControlProxyMode::Tun,
        ProxyMode::Disabled => ControlProxyMode::Disabled,
        ProxyMode::Unknown => return invalid_request_json(),
    };
    invoke_control_authorized(&state, ControlOperation::Proxy { mode }).await
}

async fn control_proxy_selection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProxySelectionRequest>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    invoke_control_authorized(&state, ControlOperation::ProxySelection { request }).await
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
    invoke_control_authorized(state, operation).await
}

async fn invoke_control_authorized(state: &AppState, operation: ControlOperation) -> Response {
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
    authorize_same_origin_csrf(state, headers)
}

fn authorize_same_origin_csrf(state: &AppState, headers: &HeaderMap) -> bool {
    let token = headers
        .get("x-hyz-csrf")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    authorize_json_origin(state, headers)
        && constant_time_equal(token.as_bytes(), state.csrf_token.as_bytes())
}

fn authorize_json_origin(state: &AppState, headers: &HeaderMap) -> bool {
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
    content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim() == "application/json")
        && origin == state.allowed_origin.as_ref()
        && fetch_site.is_none_or(|value| value == "same-origin")
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

fn admin_error_json(error: AdminError) -> Response {
    let status = match error {
        AdminError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        AdminError::InvalidNewPassword => StatusCode::BAD_REQUEST,
        AdminError::PasswordChangeRequired => StatusCode::FORBIDDEN,
        AdminError::InvalidCredentials | AdminError::InvalidSession => StatusCode::UNAUTHORIZED,
        AdminError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
    };
    authentication_error_json(status)
}

fn authentication_error_json(status: StatusCode) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": "authentication_failed",
                "message": "Authentication request rejected"
            }
        })),
    )
        .into_response()
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

fn tailscale_self_stop_conflict_json() -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": {
                "code": "tailscale_self_stop_forbidden",
                "message": "Disable or logout must be requested from LAN management or the Unix control socket"
            }
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

fn is_post_path(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/auth/login"
            | "/api/v1/auth/logout"
            | "/api/v1/auth/password"
            | "/api/v1/control/network/sta/scan"
            | "/api/v1/control/network/sta/apply"
            | "/api/v1/control/network/ap/prepare"
            | "/api/v1/control/network/ap/apply"
            | "/api/v1/control/network/ap/confirm"
            | "/api/v1/control/network/ap/cancel"
            | "/api/v1/control/proxy/device-policies"
            | "/api/v1/control/proxy/subscription/source"
            | "/api/v1/control/proxy/subscription/refresh"
            | "/api/v1/control/tailscale/mode"
            | "/api/v1/control/tailscale/login"
            | "/api/v1/control/tailscale/logout"
            | "/api/v1/control/display"
            | "/api/v1/control/proxy/mode"
            | "/api/v1/control/proxy/selection"
            | "/api/v1/control/proxy/delay"
            | "/api/v1/control/proxy/delays"
    )
}

async fn security_headers(request: Request, next: Next) -> Response {
    // GET remains globally allowed so exact routes and the API fallback can distinguish 404 from
    // 405. Sensitive GET handlers still enforce normal administrator session and JSON Origin/CSRF.
    // Only exact mutation paths accept POST; every POST handler enforces its own policy.
    let path = request.uri().path();
    let allowed =
        request.method() == Method::GET || request.method() == Method::POST && is_post_path(path);
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

    #[test]
    fn administrator_cookie_has_fixed_non_tls_attributes_and_strict_parsing() {
        let token = "a".repeat(64);
        let cookie = session_cookie(&token).unwrap().to_str().unwrap().to_owned();
        assert!(cookie.starts_with(&format!("{ADMIN_SESSION_COOKIE}=")));
        assert!(cookie.contains("; HttpOnly"));
        assert!(cookie.contains("; SameSite=Strict"));
        assert!(cookie.contains("; Path=/"));
        assert!(!cookie.contains("; Secure"));

        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
        assert_eq!(admin_session_token(&headers).unwrap().expose(), token);
        headers.append(
            header::COOKIE,
            HeaderValue::from_str(&format!("{ADMIN_SESSION_COOKIE}={token}")).unwrap(),
        );
        assert!(admin_session_token(&headers).is_none());
    }

    #[test]
    fn sensitive_json_dtos_reject_unknown_fields_and_redact_subscription_urls() {
        assert!(serde_json::from_str::<EmptyJsonRequest>(r#"{"unexpected":true}"#).is_err());
        assert!(serde_json::from_str::<DevicePolicyUpdateRequest>(
            r#"{"expected_generation":0,"entries":[],"command":"iptables"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<SubscriptionSourceRequest>(
            r#"{"url":"https://example.com/sub","unexpected":true}"#
        )
        .is_err());

        assert!(serde_json::from_str::<TailscaleModeRequest>(
            r#"{"mode":"lan_subnet_access","login_url":"https://example.com"}"#
        )
        .is_err());
        assert!(
            serde_json::from_str::<TailscaleModeRequest>(r#"{"mode":"lan_subnet_access"}"#).is_ok()
        );
        assert!(serde_json::from_str::<EmptyJsonRequest>(r#"{"auth_key":"secret"}"#).is_err());

        let secret_url = "https://example.com/private-subscription-token";
        let request = serde_json::from_str::<SubscriptionSourceRequest>(&format!(
            r#"{{"url":"{secret_url}"}}"#
        ))
        .unwrap();
        let operation = ControlOperation::SubscriptionSet { url: request.url };
        assert!(!format!("{operation:?}").contains(secret_url));
        assert_eq!(MAX_HTTP_JSON_BODY_BYTES, 4 * 1024);
    }

    #[cfg(feature = "e2e")]
    #[test]
    fn e2e_origin_is_exact_loopback_and_runtime_tar_requires_index() {
        assert!(validate_exact_loopback_origin("http://127.0.0.1:18080").is_ok());
        assert!(validate_exact_loopback_origin("http://127.0.0.1:18080/").is_err());
        assert!(validate_exact_loopback_origin("http://localhost:18080").is_err());
        assert!(validate_exact_loopback_origin("http://0.0.0.0:18080").is_err());
        assert!(validate_exact_loopback_origin("https://127.0.0.1:18080").is_err());
        assert!(validate_exact_loopback_origin("http://127.0.0.1:0").is_err());

        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        let index = b"<!doctype html><title>runtime e2e</title>";
        header.set_size(index.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "index.html", index.as_slice())
            .unwrap();
        let archive = builder.into_inner().unwrap();
        let assets = AssetStore::from_tar(&archive).unwrap();
        assert_eq!(assets.get("index.html").unwrap().as_ref(), &index[..]);

        let empty_archive = tar::Builder::new(Vec::new()).into_inner().unwrap();
        assert!(AssetStore::from_tar(&empty_archive).is_err());
    }

    #[test]
    fn proxy_mode_and_selection_require_normal_administrator_authorization() {
        let source = include_str!("mod.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production HTTP source");
        for (start, end) in [
            (
                "async fn control_proxy_mode(",
                "async fn control_proxy_selection(",
            ),
            (
                "async fn control_proxy_selection(",
                "async fn control_proxy_delay(",
            ),
        ] {
            let body = source
                .split_once(start)
                .expect("proxy control handler")
                .1
                .split_once(end)
                .expect("end of proxy control handler")
                .0;
            assert!(body.contains("authorize_sensitive_control(&state, &headers).await"));
            assert!(body.contains("invoke_control_authorized"));
            assert!(!body.contains("invoke_control(&state, &headers"));
        }
    }

    #[test]
    fn post_allowlist_keeps_auth_sensitive_and_lan_control_boundaries_exact() {
        assert!(is_post_path("/api/v1/auth/login"));
        assert!(is_post_path("/api/v1/auth/logout"));
        assert!(is_post_path("/api/v1/auth/password"));
        assert!(is_post_path("/api/v1/control/network/sta/scan"));
        assert!(is_post_path("/api/v1/control/network/sta/apply"));
        assert!(is_post_path("/api/v1/control/network/ap/prepare"));
        assert!(is_post_path("/api/v1/control/network/ap/apply"));
        assert!(is_post_path("/api/v1/control/network/ap/confirm"));
        assert!(is_post_path("/api/v1/control/network/ap/cancel"));
        assert!(is_post_path("/api/v1/control/proxy/device-policies"));
        assert!(is_post_path("/api/v1/control/proxy/subscription/source"));
        assert!(is_post_path("/api/v1/control/proxy/subscription/refresh"));
        assert!(is_post_path("/api/v1/control/tailscale/mode"));
        assert!(is_post_path("/api/v1/control/tailscale/login"));
        assert!(is_post_path("/api/v1/control/tailscale/logout"));
        assert!(is_post_path("/api/v1/control/display"));
        assert!(is_post_path("/api/v1/control/proxy/mode"));
        assert!(!is_post_path("/api/v1/auth/session"));
        assert!(!is_post_path("/api/v1/admin/login"));
        assert!(!is_post_path("/api/v1/network/config"));
        assert!(!is_post_path("/api/v1/proxy/device-policies"));
        assert!(!is_post_path("/api/v1/proxy/subscription"));
        assert!(!is_post_path("/api/v1/tailscale"));
        assert!(!is_post_path("/api/v1/control/tailscale"));
        assert!(!is_post_path("/api/v1/control/network/sta"));
        assert!(!is_post_path("/api/v1/control"));
    }
}
