use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use hyz_things::{
    application::{
        admin::{AdminApplication, AdminError},
        ports::{AdminCredentialStorePort, AdminRandomPort, ClockPort, PlatformError},
    },
    domain::admin::{
        AdminCredential, AdminLoginRequest, SecretString, DEFAULT_ADMIN_BOOTSTRAP_PASSWORD,
    },
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

#[test]
fn administrator_session_stays_valid_until_thirty_days() {
    let platform = Arc::new(TestPlatform::default());
    let app =
        AdminApplication::initialize(platform.clone(), platform.clone(), platform.clone()).unwrap();
    let session = app
        .login(&AdminLoginRequest {
            password: SecretString::new(DEFAULT_ADMIN_BOOTSTRAP_PASSWORD),
        })
        .unwrap()
        .token;
    let thirty_days_millis = 30_u64 * 24 * 60 * 60 * 1_000;

    platform
        .now
        .store(thirty_days_millis - 1, Ordering::Relaxed);
    assert!(matches!(
        app.authorize(&session),
        Err(AdminError::PasswordChangeRequired)
    ));

    platform.now.store(thirty_days_millis, Ordering::Relaxed);
    assert!(matches!(
        app.authorize(&session),
        Err(AdminError::InvalidSession)
    ));
}
