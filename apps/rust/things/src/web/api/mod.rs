use super::*;

mod apps;
mod auth;
mod camera;
mod network;
mod proxy;
mod status;
mod tailscale;

pub(crate) use apps::*;
pub(crate) use auth::*;
pub(crate) use camera::*;
pub(crate) use network::*;
pub(crate) use proxy::*;
pub(crate) use status::*;
pub(crate) use tailscale::*;

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmptyRequest {}

pub(crate) async fn fetch_json<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    label: &str,
) -> Result<T, String> {
    let response = Request::get(endpoint)
        .credentials(RequestCredentials::SameOrigin)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| format!("无法连接{label}接口：{error}"))?;
    if !response.ok() {
        return Err(format!("{label}接口返回 HTTP {}", response.status()));
    }
    response
        .json::<T>()
        .await
        .map_err(|error| format!("{label}数据格式无效：{error}"))
}

pub(crate) async fn post_json<T: serde::Serialize>(
    endpoint: &str,
    csrf: &str,
    body: &T,
    label: &str,
) -> Result<gloo_net::http::Response, String> {
    let request = Request::post(endpoint)
        .credentials(RequestCredentials::SameOrigin)
        .header("Accept", "application/json")
        .header("X-HYZ-CSRF", csrf)
        .json(body)
        .map_err(|error| format!("无法编码{label}请求：{error}"))?;
    let response = request
        .send()
        .await
        .map_err(|error| format!("无法连接{label}接口：{error}"))?;
    if response.ok() {
        Ok(response)
    } else {
        Err(format!("{label}接口返回 HTTP {}", response.status()))
    }
}

pub(crate) async fn post_json_response<T: serde::Serialize, R: serde::de::DeserializeOwned>(
    endpoint: &str,
    csrf: &str,
    body: &T,
    label: &str,
) -> Result<R, String> {
    post_json(endpoint, csrf, body, label)
        .await?
        .json::<R>()
        .await
        .map_err(|error| format!("{label}响应格式无效：{error}"))
}
