use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroize;

pub const ADMIN_USERNAME: &str = "admin";
/// Factory bootstrap password for the built-in administrator.
///
/// This value is intentionally public so a fresh device can be provisioned without a per-device
/// secret. A bootstrap login is restricted to changing this password: persisted credentials start
/// with `must_change = true`, and normal authorization must reject that session until it is changed.
pub const DEFAULT_ADMIN_BOOTSTRAP_PASSWORD: &str = "admin";

pub const MIN_ADMIN_PASSWORD_BYTES: usize = 12;
pub const MAX_ADMIN_PASSWORD_BYTES: usize = 1_024;

#[derive(Deserialize, Serialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminLoginRequest {
    pub password: SecretString,
}

impl fmt::Debug for AdminLoginRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminLoginRequest")
            .field("password", &self.password)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminPasswordChangeRequest {
    pub current_password: SecretString,
    pub new_password: SecretString,
}

impl fmt::Debug for AdminPasswordChangeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminPasswordChangeRequest")
            .field("current_password", &self.current_password)
            .field("new_password", &self.new_password)
            .finish()
    }
}

#[derive(Debug, Serialize)]
pub struct AdminLoginResponse {
    pub token: SecretString,
    pub must_change_password: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AdminAuthorization {
    pub must_change_password: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct AdminPasswordHash(String);

impl AdminPasswordHash {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AdminPasswordHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdminPasswordHash([REDACTED])")
    }
}

impl Drop for AdminPasswordHash {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdminCredential {
    pub password_hash: AdminPasswordHash,
    pub must_change: bool,
}

pub fn valid_new_admin_password(password: &str) -> bool {
    (MIN_ADMIN_PASSWORD_BYTES..=MAX_ADMIN_PASSWORD_BYTES).contains(&password.len())
        && !password.chars().any(char::is_control)
        && password != DEFAULT_ADMIN_BOOTSTRAP_PASSWORD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted_from_debug_output() {
        let secret = SecretString::new("not-for-logs");
        let hash = AdminPasswordHash::new("$argon2id$not-for-logs");
        assert!(!format!("{secret:?}").contains("not-for-logs"));
        assert!(!format!("{hash:?}").contains("not-for-logs"));
    }

    #[test]
    fn replacement_password_is_bounded_and_cannot_restore_bootstrap_value() {
        assert!(valid_new_admin_password("a-long-passphrase"));
        assert!(!valid_new_admin_password(DEFAULT_ADMIN_BOOTSTRAP_PASSWORD));
        assert!(!valid_new_admin_password("short"));
        assert!(!valid_new_admin_password("line-break-is-bad\n"));
        assert!(!valid_new_admin_password(
            &"x".repeat(MAX_ADMIN_PASSWORD_BYTES + 1)
        ));
    }
}
