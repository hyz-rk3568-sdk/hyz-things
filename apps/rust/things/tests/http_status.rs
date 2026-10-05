#![cfg(feature = "native")]

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use hyz_contract::router::{ControlOperation, ControlResult};
use hyz_things::{
    adapters::inbound::http::{app, app_with_control},
    application::{
        ports::{PortalControlError, PortalControlHandler},
        status::PortalStatus,
    },
    domain::{
        panel::{DisplayStatus, PanelSnapshot},
        status::{
            Component, Issue, LanTunEffective, LanTunStatus, LocalSystemProxyEffective,
            LocalSystemProxyStatus, MihomoCoreStatus, ProxyResourceState, ProxyStatus,
            RouterStatus, SnapshotState, StatusSnapshot, SystemStats,
        },
    },
};

#[derive(Default)]
struct FakeControl {
    display_mutations: AtomicUsize,
    fail_display: bool,
}

#[async_trait]
impl PortalControlHandler for FakeControl {
    async fn handle(
        &self,
        operation: ControlOperation,
    ) -> Result<ControlResult, PortalControlError> {
        match operation {
            ControlOperation::Status {} => Ok(ControlResult::Status {
                snapshot: Box::new(StatusSnapshot {
                    state: SnapshotState::Degraded,
                    observed_at_unix_ms: 123,
                    router: Component::available(RouterStatus::default()),
                    proxy: Component::unavailable(Issue::new(
                        "proxy_unavailable",
                        "Proxy status is unavailable",
                    )),
                    tailscale: Component::unavailable(Issue::new(
                        "tailscale_unavailable",
                        "Tailscale status is unavailable",
                    )),
                    system: Component::degraded(
                        SystemStats::default(),
                        Issue::new("stats_partial", "Some system statistics are unavailable"),
                    ),
                }),
            }),
            ControlOperation::PanelStatus { .. } => Ok(ControlResult::PanelStatus {
                snapshot: Box::new(PanelSnapshot {
                    display: Component::available(DisplayStatus {
                        enabled: false,
                        brightness: 0,
                        actual_brightness: 0,
                        max_brightness: 255,
                    }),
                    proxy_groups: Component::available(Vec::new()),
                }),
            }),
            ControlOperation::Display { .. } => {
                self.display_mutations.fetch_add(1, Ordering::SeqCst);
                if self.fail_display {
                    Err(PortalControlError::remote(
                        "display_conflict",
                        "internal sensitive detail: /userdata/credentials",
                    ))
                } else {
                    Ok(ControlResult::Completed {
                        message: "display applied".to_owned(),
                    })
                }
            }
            ControlOperation::ProxyDelayRefresh { .. } => {
                Ok(ControlResult::ProxyDelays { groups: Vec::new() })
            }
            _ => Err(PortalControlError::remote(
                "test_unsupported_operation",
                "unsupported fake operation",
            )),
        }
    }
}

fn read_status(control: Arc<dyn PortalControlHandler>) -> PortalStatus {
    PortalStatus::new(control)
}

fn http_request(address: SocketAddr, method: &str, path: &str) -> String {
    http_request_with_headers(address, method, path, &[])
}

fn http_request_with_headers(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("connect to test server");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");
    let extra_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nAccept: application/json\r\n{extra_headers}\r\n"
    )
    .expect("write request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}

fn response_header(response: &str, name: &str) -> Option<String> {
    response
        .split("\r\n\r\n")
        .next()
        .into_iter()
        .flat_map(str::lines)
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
}

fn http_json_request(
    address: SocketAddr,
    path: &str,
    origin: &str,
    csrf: Option<&str>,
    body: &str,
) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("connect to test server");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");
    let csrf = csrf
        .map(|value| format!("X-HYZ-CSRF: {value}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 192.168.8.1\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{csrf}Connection: close\r\n\r\n{body}",
        body.len(),
    )
    .expect("write JSON request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}

#[tokio::test]
async fn serves_partial_degraded_status_with_strict_http_policy() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let address = listener.local_addr().expect("read test address");
    let control: Arc<dyn PortalControlHandler> = Arc::new(FakeControl::default());
    let server = tokio::spawn(async move {
        axum::serve(listener, app(read_status(control)))
            .await
            .expect("serve test app");
    });

    let status =
        tokio::task::spawn_blocking(move || http_request(address, "GET", "/api/v1/status"))
            .await
            .expect("join test client");
    assert!(status.starts_with("HTTP/1.1 200 OK\r\n"));
    let status_lower = status.to_ascii_lowercase();
    assert!(status_lower.contains("content-security-policy:"));
    assert!(status_lower.contains("script-src 'self' 'wasm-unsafe-eval'"));
    assert!(!status_lower.contains("'unsafe-inline'"));
    assert!(!status_lower.contains("script-src 'self' 'unsafe-eval'"));
    assert!(status_lower.contains("x-frame-options: deny"));
    assert!(status_lower.contains("x-hyz-request-id:"));
    assert!(status_lower.contains(
        "permissions-policy: camera=(), microphone=(self), geolocation=(), payment=(), usb=()"
    ));
    assert!(!status_lower.contains("access-control-allow-origin:"));
    assert!(status.contains("\"state\":\"degraded\""));
    assert!(status.contains("\"observed_at_unix_ms\":123"));
    assert!(status.contains("\"proxy\":{\"state\":\"unavailable\""));
    assert!(status.contains("\"tailscale\":{\"state\":\"unavailable\""));
    assert!(!status.contains("login_url"));
    assert!(!status.contains("auth_key"));
    assert!(status.contains("\"system\":{\"state\":\"degraded\""));

    let api_address = server_address(&server, address);
    let unknown = tokio::task::spawn_blocking(move || {
        http_request(api_address, "GET", "/api/v1/does-not-exist")
    })
    .await
    .expect("join unknown API client");
    assert!(unknown.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(unknown
        .to_ascii_lowercase()
        .contains("content-type: application/json"));
    assert!(!unknown.contains("Frontend bundle is not installed"));

    let method_address = server_address(&server, address);
    let head =
        tokio::task::spawn_blocking(move || http_request(method_address, "HEAD", "/api/v1/health"))
            .await
            .expect("join method client");
    assert!(head.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));

    let health_address = server_address(&server, address);
    let health =
        tokio::task::spawn_blocking(move || http_request(health_address, "GET", "/api/v1/health"))
            .await
            .expect("join health client");
    assert!(health.contains("\"status\":\"alive\""));
    assert!(health.contains("\"check\":\"liveness\""));
    assert!(health.contains("\"readiness_assessed\":false"));

    let old_auth_address = server_address(&server, address);
    let old_auth = tokio::task::spawn_blocking(move || {
        http_request(old_auth_address, "GET", "/api/v1/admin/session")
    })
    .await
    .expect("join removed administrator alias client");
    assert!(old_auth.starts_with("HTTP/1.1 404 Not Found\r\n"));

    let auth_address = server_address(&server, address);
    let auth = tokio::task::spawn_blocking(move || {
        http_request(auth_address, "GET", "/api/v1/auth/session")
    })
    .await
    .expect("join approved authentication path client");
    assert!(auth.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
    assert!(auth.contains("\"code\":\"auth_unavailable\""));
    assert!(auth.to_ascii_lowercase().contains("x-hyz-request-id:"));

    let root_address = server_address(&server, address);
    let root = tokio::task::spawn_blocking(move || http_request(root_address, "GET", "/"))
        .await
        .expect("join frontend root client");
    assert!(root.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(root.contains("cache-control: no-cache, must-revalidate"));
    let root_etag = response_header(&root, "etag").expect("frontend root etag");
    let root_not_modified_address = server_address(&server, address);
    let root_not_modified_etag = root_etag.clone();
    let root_not_modified = tokio::task::spawn_blocking(move || {
        http_request_with_headers(
            root_not_modified_address,
            "GET",
            "/",
            &[("If-None-Match", root_not_modified_etag.as_str())],
        )
    })
    .await
    .expect("join frontend root conditional client");
    assert!(root_not_modified.starts_with("HTTP/1.1 304 Not Modified\r\n"));
    assert!(root_not_modified.contains("cache-control: no-cache, must-revalidate"));
    assert!(root_not_modified.contains(&format!("etag: {root_etag}")));
    let bootstrap_path = root
        .split("src=\"")
        .skip(1)
        .filter_map(|value| value.split('"').next())
        .find(|path| path.starts_with("/router-bootstrap.js?v="))
        .expect("versioned bootstrap src");
    assert_eq!(bootstrap_path.len(), "/router-bootstrap.js?v=".len() + 16);

    let bootstrap_address = server_address(&server, address);
    let bootstrap_path = bootstrap_path.to_owned();
    let bootstrap = tokio::task::spawn_blocking(move || {
        http_request(bootstrap_address, "GET", &bootstrap_path)
    })
    .await
    .expect("join bootstrap client");
    assert!(bootstrap.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(bootstrap.contains("content-type: text/javascript"));
    assert!(bootstrap.contains("cache-control: public, max-age=31536000, immutable"));
    assert!(response_header(&bootstrap, "etag").is_some());
    assert!(bootstrap.contains("import init"));

    let stylesheet_path = root
        .split("rel=\"stylesheet\" href=\"")
        .nth(1)
        .and_then(|value| value.split('"').next())
        .expect("fingerprinted stylesheet path");
    let stylesheet_address = server_address(&server, address);
    let stylesheet_path = stylesheet_path.to_owned();
    let stylesheet = tokio::task::spawn_blocking(move || {
        http_request(stylesheet_address, "GET", &stylesheet_path)
    })
    .await
    .expect("join stylesheet client");
    assert!(stylesheet.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(stylesheet.contains("cache-control: public, max-age=31536000, immutable"));
    assert!(response_header(&stylesheet, "etag").is_some());

    let manifest_address = server_address(&server, address);
    let manifest = tokio::task::spawn_blocking(move || {
        http_request(manifest_address, "GET", "/manifest.webmanifest")
    })
    .await
    .expect("join manifest client");
    assert!(manifest.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(manifest.contains("cache-control: public, max-age=3600"));
    assert!(response_header(&manifest, "etag").is_some());

    let unversioned_bootstrap_address = server_address(&server, address);
    let unversioned_bootstrap = tokio::task::spawn_blocking(move || {
        http_request(unversioned_bootstrap_address, "GET", "/router-bootstrap.js")
    })
    .await
    .expect("join unversioned bootstrap client");
    assert!(unversioned_bootstrap.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(unversioned_bootstrap.contains("cache-control: no-store"));

    let stale_address = server_address(&server, address);
    let stale = tokio::task::spawn_blocking(move || {
        http_request(stale_address, "GET", "/router-web-stale.js")
    })
    .await
    .expect("join stale asset client");
    assert!(stale.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(!stale.contains("<!doctype html>"));

    let route_address = server_address(&server, address);
    let route =
        tokio::task::spawn_blocking(move || http_request(route_address, "GET", "/dashboard"))
            .await
            .expect("join SPA route client");
    assert!(route.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(route.contains("<!doctype html>"));

    server.abort();
}

#[tokio::test]
async fn remote_control_code_survives_http_mapping_without_leaking_remote_detail() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind remote error test listener");
    let address = listener
        .local_addr()
        .expect("read remote error test address");
    let control = Arc::new(FakeControl {
        fail_display: true,
        ..FakeControl::default()
    });
    let server_control: Arc<dyn PortalControlHandler> = control;
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app_with_control(
                read_status(server_control.clone()),
                server_control,
                "csrf-test".to_owned(),
                address.port(),
            ),
        )
        .await
        .expect("serve remote error test app");
    });

    let origin = format!("http://192.168.8.1:{}", address.port());
    let response = tokio::task::spawn_blocking(move || {
        http_json_request(
            address,
            "/api/v1/control/display",
            &origin,
            Some("csrf-test"),
            r#"{"enabled":true,"brightness":128}"#,
        )
    })
    .await
    .expect("join remote error client");

    assert!(response.starts_with("HTTP/1.1 409 Conflict\r\n"));
    assert!(response.contains(r#""code":"display_conflict""#));
    assert!(response.contains("Router control request conflicts with current state"));
    assert!(!response.contains("/userdata/credentials"));
    assert!(!response.contains("internal sensitive detail"));
    assert!(response.to_ascii_lowercase().contains("x-hyz-request-id:"));

    server.abort();
}

#[tokio::test]
async fn control_posts_require_exact_origin_token_and_typed_json() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind control test listener");
    let address = listener.local_addr().expect("read control test address");
    let control = Arc::new(FakeControl::default());
    let server_control: Arc<dyn PortalControlHandler> = control.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app_with_control(
                read_status(server_control.clone()),
                server_control,
                "csrf-test".to_owned(),
                address.port(),
            ),
        )
        .await
        .expect("serve control test app");
    });

    let panel_address = server_address(&server, address);
    let panel =
        tokio::task::spawn_blocking(move || http_request(panel_address, "GET", "/api/v1/panel"))
            .await
            .expect("join panel client");
    assert!(panel.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(panel.contains("\"csrf_token\":\"csrf-test\""));
    assert!(panel.contains("\"max_brightness\":255"));
    assert!(!panel.contains("controller.secret"));

    let tailscale_get_address = server_address(&server, address);
    let tailscale_get = tokio::task::spawn_blocking(move || {
        http_request(tailscale_get_address, "GET", "/api/v1/tailscale")
    })
    .await
    .expect("join anonymous Tailscale GET client");
    assert!(tailscale_get.starts_with("HTTP/1.1 401 Unauthorized\r\n"));

    let tailscale_peers_address = server_address(&server, address);
    let tailscale_peers = tokio::task::spawn_blocking(move || {
        http_request(tailscale_peers_address, "GET", "/api/v1/tailscale/peers")
    })
    .await
    .expect("join anonymous Tailscale peers GET client");
    assert!(tailscale_peers.starts_with("HTTP/1.1 401 Unauthorized\r\n"));

    let origin = format!("http://192.168.8.1:{}", address.port());

    let login_without_token_address = server_address(&server, address);
    let login_without_token_origin = origin.clone();
    let login_without_token = tokio::task::spawn_blocking(move || {
        http_json_request(
            login_without_token_address,
            "/api/v1/auth/login",
            &login_without_token_origin,
            None,
            r#"{"password":"admin"}"#,
        )
    })
    .await
    .expect("join login missing-token client");
    assert!(login_without_token.starts_with("HTTP/1.1 403 Forbidden\r\n"));

    let login_wrong_token_address = server_address(&server, address);
    let login_wrong_token_origin = origin.clone();
    let login_wrong_token = tokio::task::spawn_blocking(move || {
        http_json_request(
            login_wrong_token_address,
            "/api/v1/auth/login",
            &login_wrong_token_origin,
            Some("wrong-token"),
            r#"{"password":"admin"}"#,
        )
    })
    .await
    .expect("join login wrong-token client");
    assert!(login_wrong_token.starts_with("HTTP/1.1 403 Forbidden\r\n"));

    let login_valid_token_address = server_address(&server, address);
    let login_valid_token_origin = origin.clone();
    let login_valid_token = tokio::task::spawn_blocking(move || {
        http_json_request(
            login_valid_token_address,
            "/api/v1/auth/login",
            &login_valid_token_origin,
            Some("csrf-test"),
            r#"{"password":"admin"}"#,
        )
    })
    .await
    .expect("join login valid-token client");
    assert!(login_valid_token.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));

    let missing_token_address = server_address(&server, address);
    let missing_token_origin = origin.clone();
    let missing_token = tokio::task::spawn_blocking(move || {
        http_json_request(
            missing_token_address,
            "/api/v1/control/display",
            &missing_token_origin,
            None,
            r#"{"enabled":true,"brightness":128}"#,
        )
    })
    .await
    .expect("join missing-token client");
    assert!(missing_token.starts_with("HTTP/1.1 403 Forbidden\r\n"));

    let foreign_address = server_address(&server, address);
    let foreign = tokio::task::spawn_blocking(move || {
        http_json_request(
            foreign_address,
            "/api/v1/control/display",
            "http://attacker.invalid",
            Some("csrf-test"),
            r#"{"enabled":true,"brightness":128}"#,
        )
    })
    .await
    .expect("join foreign-origin client");
    assert!(foreign.starts_with("HTTP/1.1 403 Forbidden\r\n"));

    let valid_address = server_address(&server, address);
    let valid_origin = origin.clone();
    let valid = tokio::task::spawn_blocking(move || {
        http_json_request(
            valid_address,
            "/api/v1/control/display",
            &valid_origin,
            Some("csrf-test"),
            r#"{"enabled":true,"brightness":128}"#,
        )
    })
    .await
    .expect("join valid control client");
    assert!(valid.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(valid.contains("display applied"));
    assert_eq!(control.display_mutations.load(Ordering::SeqCst), 1);

    let delays_address = server_address(&server, address);
    let delays_origin = origin.clone();
    let delays = tokio::task::spawn_blocking(move || {
        http_json_request(
            delays_address,
            "/api/v1/control/proxy/delays",
            &delays_origin,
            Some("csrf-test"),
            r#"{}"#,
        )
    })
    .await
    .expect("join proxy delay refresh client");
    assert!(delays.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(delays.contains(r#""kind":"proxy_delays""#));
    assert!(delays.contains(r#""groups":[]"#));

    let tailscale_mode_address = server_address(&server, address);
    let tailscale_mode_origin = origin.clone();
    let tailscale_mode = tokio::task::spawn_blocking(move || {
        http_json_request(
            tailscale_mode_address,
            "/api/v1/control/tailscale/mode",
            &tailscale_mode_origin,
            Some("csrf-test"),
            r#"{"mode":"lan_subnet_access"}"#,
        )
    })
    .await
    .expect("join anonymous Tailscale mode client");
    assert!(tailscale_mode.starts_with("HTTP/1.1 401 Unauthorized\r\n"));

    let unknown_address = server_address(&server, address);
    let unknown = tokio::task::spawn_blocking(move || {
        http_json_request(
            unknown_address,
            "/api/v1/control/display",
            &origin,
            Some("csrf-test"),
            r#"{"enabled":true,"brightness":128,"path":"/sys"}"#,
        )
    })
    .await
    .expect("join unknown-field client");
    assert!(unknown.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    assert!(unknown.contains(r#""code":"invalid_request""#));
    assert!(unknown.to_ascii_lowercase().contains("x-hyz-request-id:"));
    assert_eq!(control.display_mutations.load(Ordering::SeqCst), 1);

    server.abort();
}

fn server_address<T>(_server: &tokio::task::JoinHandle<T>, address: SocketAddr) -> SocketAddr {
    address
}

#[test]
fn typed_proxy_dto_keeps_layered_wire_values_stable() {
    let value = serde_json::to_value(ProxyStatus {
        configured: true,
        mihomo: MihomoCoreStatus {
            configured_required: Some(true),
            process: ProxyResourceState::Ready,
            runtime_config: ProxyResourceState::Ready,
            mixed_port: ProxyResourceState::Ready,
        },
        lan_tun: LanTunStatus {
            desired: Some(true),
            effective: LanTunEffective::Ready,
            ordinary_nat_fallback: Some(false),
        },
        local_system_proxy: LocalSystemProxyStatus {
            desired: Some(false),
            effective: LocalSystemProxyEffective::Disabled,
        },
    })
    .expect("serialize proxy DTO");
    assert_eq!(value["mihomo"]["process"], "ready");
    assert_eq!(value["lan_tun"]["effective"], "ready");
    assert_eq!(value["local_system_proxy"]["effective"], "disabled");
}

#[test]
fn error_contract_keeps_machine_readable_failure_semantics() {
    let http = include_str!("../src/adapters/inbound/http/mod.rs");
    let router_client = include_str!("../src/adapters/outbound/router.rs");
    let ports = include_str!("../src/application/ports.rs");

    assert!(
        http.contains("\"auth_invalid_credentials\""),
        "authentication errors need stable machine-readable codes"
    );
    assert!(
        http.contains("\"auth_invalid_session\""),
        "session expiry must be distinguishable from bad credentials"
    );
    assert!(
        !http.contains("\"authentication_failed\""),
        "known authentication failures must not collapse into one generic code"
    );
    assert!(
        !router_client.contains("map_err(|error| error.to_string())"),
        "router remote errors must keep their typed code instead of becoming strings"
    );
    assert!(
        !ports.contains("Result<ControlResult, String>"),
        "PortalControlHandler must preserve typed transport/remote failures"
    );
}
