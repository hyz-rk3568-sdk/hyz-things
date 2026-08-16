//! Write-only secret strings that cross the control protocol.

use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroize;

/// A secret whose value cannot be read back through `Debug`/`Display` and is
/// zeroized on drop. Used for subscription URLs and admin credentials.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
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
