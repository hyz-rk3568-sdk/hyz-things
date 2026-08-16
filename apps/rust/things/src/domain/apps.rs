//! Installed-application value objects backing the portal home page.

use std::collections::BTreeMap;

/// One deployed application as recorded by the hot-push registry
/// (`/userdata/hyz-things/apps/registry.json`). Applications installed from
/// firmware have no registry entry; the portal renders them as 固件内置.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstalledApp {
    pub name: String,
    pub binary: String,
    pub init_script: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub protocol_versions: BTreeMap<String, u32>,
}

impl InstalledApp {
    pub fn firmware(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            binary: String::new(),
            init_script: String::new(),
            sha256: None,
            protocol_versions: BTreeMap::new(),
        }
    }
}
