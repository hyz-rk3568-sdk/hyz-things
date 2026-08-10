use crate::{
    adapters::outbound::storage,
    application::ports::{AdminCredentialStorePort, AdminRandomPort, PlatformError},
    domain::admin::AdminCredential,
};

pub const ADMIN_CREDENTIAL_PATH: &str = "/userdata/hyz-router/admin/credential.json";
const MAX_ADMIN_CREDENTIAL_BYTES: usize = 4_096;

#[derive(Debug, Clone)]
pub struct AdminFileAdapter {
    credential_path: String,
}

impl Default for AdminFileAdapter {
    fn default() -> Self {
        Self::new(ADMIN_CREDENTIAL_PATH)
    }
}

impl AdminFileAdapter {
    pub fn new(credential_path: impl Into<String>) -> Self {
        Self {
            credential_path: credential_path.into(),
        }
    }
}

impl AdminCredentialStorePort for AdminFileAdapter {
    fn load_admin_credential(&self) -> Result<Option<AdminCredential>, PlatformError> {
        let Some(encoded) = storage::read_private_small_optional(
            &self.credential_path,
            MAX_ADMIN_CREDENTIAL_BYTES,
        )?
        else {
            return Ok(None);
        };
        serde_json::from_str(&encoded)
            .map(Some)
            .map_err(|_| invalid_credential_record())
    }

    fn save_admin_credential(&self, credential: &AdminCredential) -> Result<(), PlatformError> {
        storage::require_private_root_file_optional(&self.credential_path)?;
        let encoded = serde_json::to_vec(credential).map_err(|_| invalid_credential_record())?;
        if encoded.len() > MAX_ADMIN_CREDENTIAL_BYTES {
            return Err(invalid_credential_record());
        }
        storage::atomic_write_private(&self.credential_path, &encoded)
    }
}

impl AdminRandomPort for AdminFileAdapter {
    fn fill_random(&self, destination: &mut [u8]) -> Result<(), PlatformError> {
        getrandom::fill(destination)
            .map_err(|_| PlatformError::Io("secure random source is unavailable".to_owned()))
    }
}

fn invalid_credential_record() -> PlatformError {
    PlatformError::InvalidState("invalid administrator credential record".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::admin::AdminPasswordHash;

    #[test]
    fn persisted_record_contains_only_hash_and_must_change_flag() {
        let record = AdminCredential {
            password_hash: AdminPasswordHash::new("$argon2id$v=19$m=1,t=1,p=1$c2FsdA$aGFzaA"),
            must_change: true,
        };
        let value = serde_json::to_value(record).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert!(value.get("password_hash").is_some());
        assert_eq!(
            value.get("must_change"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn persisted_record_rejects_unknown_fields() {
        let value = r#"{
            "password_hash":"$argon2id$v=19$m=1,t=1,p=1$c2FsdA$aGFzaA",
            "must_change":true,
            "token":"must-not-be-persisted"
        }"#;
        assert!(serde_json::from_str::<AdminCredential>(value).is_err());
    }
}
