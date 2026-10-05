use super::*;

mod apps;
mod auth;
mod camera;
mod flashcards;
mod network;
mod proxy;
mod status;
mod tailscale;

pub(crate) use apps::*;
pub(crate) use auth::*;
pub(crate) use camera::*;
pub(crate) use flashcards::*;
pub(crate) use network::*;
pub(crate) use proxy::*;
pub(crate) use status::*;
pub(crate) use tailscale::*;

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmptyRequest {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApiError {
    status: Option<u16>,
    code: Option<String>,
    message: String,
}

impl ApiError {
    fn local(message: String) -> Self {
        Self {
            status: None,
            code: None,
            message,
        }
    }

    fn http(status: u16, code: Option<String>, message: String) -> Self {
        Self {
            status: Some(status),
            code,
            message,
        }
    }

    pub(crate) const fn status(&self) -> Option<u16> {
        self.status
    }

    pub(crate) fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub(crate) fn is_code(&self, code: &str) -> bool {
        self.code() == Some(code)
    }

    pub(crate) fn is_auth_expired(&self) -> bool {
        self.status == Some(401) || self.is_code("auth_invalid_session")
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.status, self.code()) {
            (Some(status), Some(code)) => {
                write!(formatter, "{}（HTTP {status} · {code}）", self.message)
            }
            (Some(status), None) => write!(formatter, "{}（HTTP {status}）", self.message),
            (None, _) => formatter.write_str(&self.message),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiErrorBody {
    code: String,
    message: String,
}

async fn response_error(response: gloo_net::http::Response, label: &str) -> ApiError {
    let status = response.status();
    match response.json::<ApiErrorEnvelope>().await {
        Ok(envelope) => ApiError::http(
            status,
            Some(envelope.error.code),
            format!("{label}：{}", envelope.error.message),
        ),
        Err(_) => ApiError::http(status, None, format!("{label}接口请求失败")),
    }
}

pub(crate) async fn fetch_json_typed<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    label: &str,
) -> Result<T, ApiError> {
    let response = Request::get(endpoint)
        .credentials(RequestCredentials::SameOrigin)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| ApiError::local(format!("无法连接{label}接口：{error}")))?;
    if !response.ok() {
        return Err(response_error(response, label).await);
    }
    response
        .json::<T>()
        .await
        .map_err(|error| ApiError::local(format!("{label}数据格式无效：{error}")))
}

pub(crate) async fn fetch_json<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    label: &str,
) -> Result<T, String> {
    fetch_json_typed(endpoint, label)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn post_json_typed<T: serde::Serialize>(
    endpoint: &str,
    csrf: &str,
    body: &T,
    label: &str,
) -> Result<gloo_net::http::Response, ApiError> {
    let request = Request::post(endpoint)
        .credentials(RequestCredentials::SameOrigin)
        .header("Accept", "application/json")
        .header("X-HYZ-CSRF", csrf)
        .json(body)
        .map_err(|error| ApiError::local(format!("无法编码{label}请求：{error}")))?;
    let response = request
        .send()
        .await
        .map_err(|error| ApiError::local(format!("无法连接{label}接口：{error}")))?;
    if response.ok() {
        Ok(response)
    } else {
        Err(response_error(response, label).await)
    }
}

pub(crate) async fn post_json<T: serde::Serialize>(
    endpoint: &str,
    csrf: &str,
    body: &T,
    label: &str,
) -> Result<gloo_net::http::Response, String> {
    post_json_typed(endpoint, csrf, body, label)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn post_json_response_typed<
    T: serde::Serialize,
    R: serde::de::DeserializeOwned,
>(
    endpoint: &str,
    csrf: &str,
    body: &T,
    label: &str,
) -> Result<R, ApiError> {
    post_json_typed(endpoint, csrf, body, label)
        .await?
        .json::<R>()
        .await
        .map_err(|error| ApiError::local(format!("{label}响应格式无效：{error}")))
}

pub(crate) async fn post_json_response<T: serde::Serialize, R: serde::de::DeserializeOwned>(
    endpoint: &str,
    csrf: &str,
    body: &T,
    label: &str,
) -> Result<R, String> {
    post_json_response_typed(endpoint, csrf, body, label)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_logic_uses_status_and_code_instead_of_display_text() {
        let expired = ApiError::http(
            401,
            Some("auth_invalid_session".to_owned()),
            "completely different wording".to_owned(),
        );
        assert!(expired.is_auth_expired());

        let conflict = ApiError::http(
            409,
            Some("device_policy_generation_conflict".to_owned()),
            "wording may change".to_owned(),
        );
        assert_eq!(conflict.status(), Some(409));
        assert!(conflict.is_code("device_policy_generation_conflict"));
    }

    #[test]
    fn protected_web_flows_do_not_parse_http_status_from_strings() {
        let protected = include_str!("../protected_control.rs");
        let resources = include_str!("../resources.rs");
        assert!(!protected.contains("contains(\"HTTP 401\")"));
        assert!(!resources.contains("contains(\"HTTP 401\")"));
        assert!(!resources.contains("contains(\"HTTP 409\")"));
    }
}
