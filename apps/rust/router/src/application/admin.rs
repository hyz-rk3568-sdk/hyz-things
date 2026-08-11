use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};
use zeroize::Zeroize;

use crate::{
    application::ports::{AdminCredentialStorePort, AdminRandomPort, ClockPort, PlatformError},
    domain::admin::{
        valid_new_admin_password, AdminAuthorization, AdminCredential, AdminLoginRequest,
        AdminLoginResponse, AdminPasswordChangeRequest, AdminPasswordHash, SecretString,
        DEFAULT_ADMIN_BOOTSTRAP_PASSWORD,
    },
};

pub const SESSION_IDLE_TIMEOUT_MILLIS: u64 = 15 * 60 * 1_000;
pub const SESSION_ABSOLUTE_TIMEOUT_MILLIS: u64 = 8 * 60 * 60 * 1_000;
pub const MAX_ADMIN_SESSIONS: usize = 64;
pub const LOGIN_RATE_WINDOW_MILLIS: u64 = 60 * 1_000;
pub const MAX_LOGIN_FAILURES_PER_WINDOW: usize = 5;

const SALT_BYTES: usize = 16;
const SESSION_TOKEN_BYTES: usize = 32;

#[derive(Debug)]
pub enum AdminError {
    InvalidCredentials,
    InvalidSession,
    PasswordChangeRequired,
    InvalidNewPassword,
    RateLimited,
    Unavailable(PlatformError),
}

impl fmt::Display for AdminError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCredentials => "invalid administrator credentials",
            Self::InvalidSession => "invalid or expired administrator session",
            Self::PasswordChangeRequired => "administrator password change is required",
            Self::InvalidNewPassword => "new administrator password does not meet policy",
            Self::RateLimited => "administrator login rate limit exceeded",
            Self::Unavailable(_) => "administrator authentication is unavailable",
        })
    }
}

impl Error for AdminError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

impl From<PlatformError> for AdminError {
    fn from(value: PlatformError) -> Self {
        Self::Unavailable(value)
    }
}

struct Session {
    created_at_millis: u64,
    last_seen_millis: u64,
}

struct AdminState {
    credential: AdminCredential,
    sessions: HashMap<[u8; 32], Session>,
    failed_logins: VecDeque<u64>,
}

pub struct AdminApplication {
    store: Arc<dyn AdminCredentialStorePort>,
    random: Arc<dyn AdminRandomPort>,
    clock: Arc<dyn ClockPort>,
    state: Mutex<AdminState>,
}

impl AdminApplication {
    pub fn initialize(
        store: Arc<dyn AdminCredentialStorePort>,
        random: Arc<dyn AdminRandomPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Result<Self, AdminError> {
        let credential = match store.load_admin_credential()? {
            Some(credential) => {
                parse_argon2id_hash(credential.password_hash.expose())?;
                credential
            }
            None => {
                let credential = AdminCredential {
                    password_hash: hash_password(
                        random.as_ref(),
                        DEFAULT_ADMIN_BOOTSTRAP_PASSWORD,
                    )?,
                    must_change: true,
                };
                store.save_admin_credential(&credential)?;
                credential
            }
        };
        Ok(Self {
            store,
            random,
            clock,
            state: Mutex::new(AdminState {
                credential,
                sessions: HashMap::new(),
                failed_logins: VecDeque::new(),
            }),
        })
    }

    pub fn login(&self, request: &AdminLoginRequest) -> Result<AdminLoginResponse, AdminError> {
        let now = self.clock.unix_time_millis();
        let mut state = self.lock_state()?;
        prune_failures(&mut state.failed_logins, now);
        if state.failed_logins.len() >= MAX_LOGIN_FAILURES_PER_WINDOW {
            return Err(AdminError::RateLimited);
        }
        if !verify_password(&state.credential.password_hash, request.password.expose()) {
            state.failed_logins.push_back(now);
            return Err(AdminError::InvalidCredentials);
        }
        state.failed_logins.clear();
        expire_sessions(&mut state.sessions, now);

        let mut token_bytes = [0_u8; SESSION_TOKEN_BYTES];
        self.random.fill_random(&mut token_bytes)?;
        let token = encode_hex(&token_bytes);
        token_bytes.zeroize();
        let digest = token_digest(token.as_bytes());

        if state.sessions.len() >= MAX_ADMIN_SESSIONS {
            if let Some(oldest) = state
                .sessions
                .iter()
                .min_by_key(|(_, session)| session.created_at_millis)
                .map(|(digest, _)| *digest)
            {
                state.sessions.remove(&oldest);
            }
        }
        state.sessions.insert(
            digest,
            Session {
                created_at_millis: now,
                last_seen_millis: now,
            },
        );
        Ok(AdminLoginResponse {
            token: SecretString::new(token),
            must_change_password: state.credential.must_change,
        })
    }

    pub fn session(&self, token: &SecretString) -> Result<AdminAuthorization, AdminError> {
        let now = self.clock.unix_time_millis();
        let mut state = self.lock_state()?;
        authenticate_session(&mut state, token.expose(), now)?;
        Ok(AdminAuthorization {
            must_change_password: state.credential.must_change,
        })
    }

    /// Authorizes normal administration. Bootstrap sessions are deliberately rejected here and
    /// can only be used with `change_password`.
    pub fn authorize(&self, token: &SecretString) -> Result<AdminAuthorization, AdminError> {
        let authorization = self.session(token)?;
        if authorization.must_change_password {
            return Err(AdminError::PasswordChangeRequired);
        }
        Ok(authorization)
    }

    pub fn change_password(
        &self,
        token: &SecretString,
        request: &AdminPasswordChangeRequest,
    ) -> Result<(), AdminError> {
        if !valid_new_admin_password(request.new_password.expose()) {
            return Err(AdminError::InvalidNewPassword);
        }
        let now = self.clock.unix_time_millis();
        let mut state = self.lock_state()?;
        let session_digest = authenticate_session(&mut state, token.expose(), now)?;
        if !verify_password(
            &state.credential.password_hash,
            request.current_password.expose(),
        ) {
            return Err(AdminError::InvalidCredentials);
        }
        let credential = AdminCredential {
            password_hash: hash_password(self.random.as_ref(), request.new_password.expose())?,
            must_change: false,
        };
        self.store.save_admin_credential(&credential)?;
        state.credential = credential;
        state.sessions.retain(|digest, _| *digest == session_digest);
        Ok(())
    }

    pub fn logout(&self, token: &SecretString) -> Result<(), AdminError> {
        let mut state = self.lock_state()?;
        state
            .sessions
            .remove(&token_digest(token.expose().as_bytes()));
        Ok(())
    }

    /// Restores the in-memory and persisted administrator state used by the host-only web harness.
    /// This seam is absent from production builds.
    #[cfg(feature = "e2e")]
    pub fn reset_e2e_bootstrap(&self) -> Result<(), AdminError> {
        let credential = AdminCredential {
            password_hash: hash_password(self.random.as_ref(), DEFAULT_ADMIN_BOOTSTRAP_PASSWORD)?,
            must_change: true,
        };
        let mut state = self.lock_state()?;
        self.store.save_admin_credential(&credential)?;
        state.credential = credential;
        state.sessions.clear();
        state.failed_logins.clear();
        Ok(())
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, AdminState>, AdminError> {
        self.state.lock().map_err(|_| {
            AdminError::Unavailable(PlatformError::InvalidState(
                "administrator authentication state lock is poisoned".to_owned(),
            ))
        })
    }
}

fn hash_password(
    random: &dyn AdminRandomPort,
    password: &str,
) -> Result<AdminPasswordHash, AdminError> {
    let mut salt_bytes = [0_u8; SALT_BYTES];
    random.fill_random(&mut salt_bytes)?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| crypto_unavailable())?;
    salt_bytes.zeroize();
    let hash = argon2id()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| crypto_unavailable())?
        .to_string();
    Ok(AdminPasswordHash::new(hash))
}

fn verify_password(expected: &AdminPasswordHash, password: &str) -> bool {
    let Ok(hash) = parse_argon2id_hash(expected.expose()) else {
        return false;
    };
    argon2id()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

fn parse_argon2id_hash(value: &str) -> Result<PasswordHash<'_>, AdminError> {
    let hash = PasswordHash::new(value).map_err(|_| crypto_unavailable())?;
    if hash.algorithm.as_str() != "argon2id" {
        return Err(crypto_unavailable());
    }
    Ok(hash)
}

fn argon2id() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

fn crypto_unavailable() -> AdminError {
    AdminError::Unavailable(PlatformError::InvalidState(
        "administrator credential record is invalid".to_owned(),
    ))
}

fn authenticate_session(
    state: &mut AdminState,
    token: &str,
    now: u64,
) -> Result<[u8; 32], AdminError> {
    expire_sessions(&mut state.sessions, now);
    let digest = token_digest(token.as_bytes());
    let session = state
        .sessions
        .get_mut(&digest)
        .ok_or(AdminError::InvalidSession)?;
    session.last_seen_millis = now;
    Ok(digest)
}

fn expire_sessions(sessions: &mut HashMap<[u8; 32], Session>, now: u64) {
    sessions.retain(|_, session| {
        now.saturating_sub(session.last_seen_millis) < SESSION_IDLE_TIMEOUT_MILLIS
            && now.saturating_sub(session.created_at_millis) < SESSION_ABSOLUTE_TIMEOUT_MILLIS
    });
}

fn prune_failures(failures: &mut VecDeque<u64>, now: u64) {
    while failures
        .front()
        .is_some_and(|attempt| now.saturating_sub(*attempt) >= LOGIN_RATE_WINDOW_MILLIS)
    {
        failures.pop_front();
    }
}

fn token_digest(token: &[u8]) -> [u8; 32] {
    Sha256::digest(token).into()
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    };

    #[derive(Default)]
    struct TestPlatform {
        credential: Mutex<Option<AdminCredential>>,
        now: AtomicU64,
        random_sequence: AtomicU64,
    }

    impl AdminCredentialStorePort for TestPlatform {
        fn load_admin_credential(&self) -> Result<Option<AdminCredential>, PlatformError> {
            Ok(self.credential.lock().unwrap().clone())
        }

        fn save_admin_credential(&self, credential: &AdminCredential) -> Result<(), PlatformError> {
            *self.credential.lock().unwrap() = Some(credential.clone());
            Ok(())
        }
    }

    impl AdminRandomPort for TestPlatform {
        fn fill_random(&self, destination: &mut [u8]) -> Result<(), PlatformError> {
            let sequence = self.random_sequence.fetch_add(1, Ordering::Relaxed);
            for (index, byte) in destination.iter_mut().enumerate() {
                *byte = sequence.wrapping_add(index as u64) as u8;
            }
            Ok(())
        }
    }

    impl ClockPort for TestPlatform {
        fn unix_time_millis(&self) -> u64 {
            self.now.load(Ordering::Relaxed)
        }
    }

    fn login(app: &AdminApplication, password: &str) -> AdminLoginResponse {
        app.login(&AdminLoginRequest {
            password: SecretString::new(password),
        })
        .unwrap()
    }

    #[test]
    fn bootstrap_is_argon2id_and_only_allows_password_change() {
        let platform = Arc::new(TestPlatform::default());
        let app =
            AdminApplication::initialize(platform.clone(), platform.clone(), platform.clone())
                .unwrap();
        let stored = platform.credential.lock().unwrap().clone().unwrap();
        assert!(stored.password_hash.expose().starts_with("$argon2id$"));
        assert!(stored.must_change);

        let response = login(&app, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD);
        assert!(response.must_change_password);
        assert!(matches!(
            app.authorize(&response.token),
            Err(AdminError::PasswordChangeRequired)
        ));
        app.change_password(
            &response.token,
            &AdminPasswordChangeRequest {
                current_password: SecretString::new(DEFAULT_ADMIN_BOOTSTRAP_PASSWORD),
                new_password: SecretString::new("replacement-passphrase"),
            },
        )
        .unwrap();
        app.authorize(&response.token).unwrap();
        assert!(
            !platform
                .credential
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .must_change
        );
    }

    #[test]
    fn sessions_enforce_idle_and_absolute_expiry() {
        let platform = Arc::new(TestPlatform::default());
        let app =
            AdminApplication::initialize(platform.clone(), platform.clone(), platform.clone())
                .unwrap();
        let idle = login(&app, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD).token;
        platform
            .now
            .store(SESSION_IDLE_TIMEOUT_MILLIS, Ordering::Relaxed);
        assert!(matches!(
            app.authorize(&idle),
            Err(AdminError::InvalidSession)
        ));

        platform.now.store(0, Ordering::Relaxed);
        let absolute = login(&app, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD).token;
        for now in (SESSION_IDLE_TIMEOUT_MILLIS / 2..SESSION_ABSOLUTE_TIMEOUT_MILLIS)
            .step_by((SESSION_IDLE_TIMEOUT_MILLIS / 2) as usize)
        {
            platform.now.store(now, Ordering::Relaxed);
            assert!(matches!(
                app.authorize(&absolute),
                Err(AdminError::PasswordChangeRequired)
            ));
        }
        platform
            .now
            .store(SESSION_ABSOLUTE_TIMEOUT_MILLIS, Ordering::Relaxed);
        assert!(matches!(
            app.authorize(&absolute),
            Err(AdminError::InvalidSession)
        ));
    }

    #[test]
    fn failed_logins_are_rate_limited_in_a_bounded_window() {
        let platform = Arc::new(TestPlatform::default());
        let app =
            AdminApplication::initialize(platform.clone(), platform.clone(), platform.clone())
                .unwrap();
        for _ in 0..MAX_LOGIN_FAILURES_PER_WINDOW {
            assert!(matches!(
                app.login(&AdminLoginRequest {
                    password: SecretString::new("wrong-password"),
                }),
                Err(AdminError::InvalidCredentials)
            ));
        }
        assert!(matches!(
            app.login(&AdminLoginRequest {
                password: SecretString::new(DEFAULT_ADMIN_BOOTSTRAP_PASSWORD),
            }),
            Err(AdminError::RateLimited)
        ));
        platform
            .now
            .store(LOGIN_RATE_WINDOW_MILLIS, Ordering::Relaxed);
        login(&app, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD);
    }

    #[cfg(feature = "e2e")]
    #[test]
    fn e2e_reset_restores_bootstrap_credential_and_clears_authentication_state() {
        let platform = Arc::new(TestPlatform::default());
        let app =
            AdminApplication::initialize(platform.clone(), platform.clone(), platform.clone())
                .unwrap();
        let bootstrap = login(&app, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD);
        app.change_password(
            &bootstrap.token,
            &AdminPasswordChangeRequest {
                current_password: SecretString::new(DEFAULT_ADMIN_BOOTSTRAP_PASSWORD),
                new_password: SecretString::new("replacement-passphrase"),
            },
        )
        .unwrap();
        for _ in 0..MAX_LOGIN_FAILURES_PER_WINDOW {
            assert!(matches!(
                app.login(&AdminLoginRequest {
                    password: SecretString::new("wrong-password"),
                }),
                Err(AdminError::InvalidCredentials)
            ));
        }
        assert!(matches!(
            app.login(&AdminLoginRequest {
                password: SecretString::new("replacement-passphrase"),
            }),
            Err(AdminError::RateLimited)
        ));

        app.reset_e2e_bootstrap().unwrap();

        {
            let state = app.state.lock().unwrap();
            assert!(state.sessions.is_empty());
            assert!(state.failed_logins.is_empty());
            assert!(state.credential.must_change);
        }
        assert!(matches!(
            app.session(&bootstrap.token),
            Err(AdminError::InvalidSession)
        ));
        assert!(matches!(
            app.login(&AdminLoginRequest {
                password: SecretString::new("replacement-passphrase"),
            }),
            Err(AdminError::InvalidCredentials)
        ));
        let reset = login(&app, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD);
        assert!(reset.must_change_password);
        let stored = platform.credential.lock().unwrap().clone().unwrap();
        assert!(stored.must_change);
        assert!(stored.password_hash.expose().starts_with("$argon2id$"));
    }

    #[test]
    fn session_storage_is_bounded_and_uses_digest_keys() {
        let platform = Arc::new(TestPlatform::default());
        let app =
            AdminApplication::initialize(platform.clone(), platform.clone(), platform.clone())
                .unwrap();
        let first = login(&app, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD).token;
        let mut newest = None;
        for index in 1..=MAX_ADMIN_SESSIONS {
            platform.now.store(index as u64, Ordering::Relaxed);
            newest = Some(login(&app, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD).token);
        }
        let newest = newest.unwrap();
        let state = app.state.lock().unwrap();
        assert_eq!(state.sessions.len(), MAX_ADMIN_SESSIONS);
        assert!(state
            .sessions
            .contains_key(&token_digest(newest.expose().as_bytes())));
        assert_ne!(
            newest.expose().as_bytes(),
            token_digest(newest.expose().as_bytes()).as_slice()
        );
        drop(state);
        assert!(matches!(
            app.authorize(&first),
            Err(AdminError::InvalidSession)
        ));
    }
}
