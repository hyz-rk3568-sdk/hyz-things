pub(crate) const AUTH_LOGIN_ENDPOINT: &str = "/api/v1/auth/login";

pub(crate) const AUTH_LOGOUT_ENDPOINT: &str = "/api/v1/auth/logout";

pub(crate) const AUTH_SESSION_ENDPOINT: &str = "/api/v1/auth/session";

pub(crate) const AUTH_PASSWORD_ENDPOINT: &str = "/api/v1/auth/password";

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthSessionDto {
    pub(crate) authenticated: bool,
    pub(crate) must_change: bool,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginRequest {
    pub(crate) password: String,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PasswordRequest {
    pub(crate) current_password: String,
    pub(crate) new_password: String,
}
