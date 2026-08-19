//! Reads the hot-push application registry written by `tools/deploy-app.sh`.
//!
//! The registry is a root-owned JSON file on the device; a fresh board with
//! firmware-installed applications has no registry yet, which is reported as
//! an empty list. The production path is a fixed constant; tests inject a
//! scratch path so host tests stay deterministic.

use crate::{
    application::ports::{InstalledAppsPort, PlatformError},
    domain::apps::InstalledApp,
};
use std::{collections::BTreeMap, fs, io::ErrorKind};

/// Fixed device path written by the hot-push tool; never caller-provided.
pub const INSTALLED_APPS_REGISTRY_PATH: &str = "/userdata/hyz-things/apps/registry.json";

pub struct RegistryAdapter {
    path: String,
}

impl RegistryAdapter {
    pub fn at(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

impl Default for RegistryAdapter {
    fn default() -> Self {
        Self::at(INSTALLED_APPS_REGISTRY_PATH)
    }
}

impl InstalledAppsPort for RegistryAdapter {
    fn installed_apps(&self) -> Result<Vec<InstalledApp>, PlatformError> {
        let contents = match fs::read(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(PlatformError::Io(format!(
                    "cannot read {}: {error}",
                    self.path
                )));
            }
        };
        let registry: RegistryFile = serde_json::from_slice(&contents).map_err(|error| {
            PlatformError::Io(format!("registry {} is invalid: {error}", self.path))
        })?;
        let mut apps: Vec<InstalledApp> = registry
            .apps
            .into_iter()
            .filter_map(|(name, entry)| {
                let current = entry.current?;
                Some(InstalledApp {
                    name,
                    binary: current.binary,
                    init_script: current.init_script,
                    sha256: Some(current.sha256),
                    deployed_at_unix_ms: current.deployed_at_unix_ms,
                    protocol_versions: current.protocol_versions,
                })
            })
            .collect();
        // Deterministic ordering for the portal and tests.
        apps.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(apps)
    }
}

#[derive(serde::Deserialize)]
struct RegistryFile {
    #[serde(default)]
    apps: BTreeMap<String, RegistryAppEntry>,
}

#[derive(serde::Deserialize)]
struct RegistryAppEntry {
    current: Option<RegistryDeployment>,
}

#[derive(serde::Deserialize)]
struct RegistryDeployment {
    binary: String,
    init_script: String,
    sha256: String,
    #[serde(default)]
    deployed_at_unix_ms: Option<u64>,
    #[serde(default)]
    protocol_versions: BTreeMap<String, u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hyz-things-registry-{}-{label}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("registry.json")
    }

    #[test]
    fn missing_registry_means_a_firmware_board() {
        let adapter = RegistryAdapter::at(scratch("missing").to_string_lossy().into_owned());
        assert_eq!(
            adapter.installed_apps().unwrap(),
            Vec::<InstalledApp>::new()
        );
    }

    #[test]
    fn parses_the_deploy_registry_shape() {
        let path = scratch("shape");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{
              "apps": {{
                "router": {{
                  "current": {{
                    "binary": "/usr/bin/hyz-router",
                    "init_script": "/etc/init.d/S81hyz-router",
                    "sha256": "abcd",
                    "deployed_at_unix_ms": 1700000000000,
                    "protocol_versions": {{ "router": 2 }}
                  }},
                  "previous": {{ "binary": "/usr/bin/hyz-router", "init_script": "/etc/init.d/S81hyz-router", "sha256": "0123", "protocol_versions": {{ "router": 1 }} }}
                }},
                "camera": {{
                  "current": {{
                    "binary": "/usr/bin/hyz-camera",
                    "init_script": "/etc/init.d/S82hyz-camera",
                    "sha256": "ef01",
                    "protocol_versions": {{ "camera": 1 }}
                  }}
                }}
              }}
            }}"#
        )
        .unwrap();

        let apps = RegistryAdapter::at(path.to_string_lossy().into_owned())
            .installed_apps()
            .unwrap();
        // 按名字排序：camera 在 router 之前。
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "camera");
        assert_eq!(apps[0].protocol_versions.get("camera"), Some(&1));
        assert_eq!(apps[1].name, "router");
        assert_eq!(apps[1].sha256.as_deref(), Some("abcd"));
        assert_eq!(apps[1].deployed_at_unix_ms, Some(1_700_000_000_000));
        assert_eq!(apps[1].protocol_versions.get("router"), Some(&2));
    }

    #[test]
    fn rejects_a_corrupt_registry() {
        let path = scratch("corrupt");
        fs::write(&path, b"not json").unwrap();
        assert!(RegistryAdapter::at(path.to_string_lossy().into_owned())
            .installed_apps()
            .is_err());
    }
}
