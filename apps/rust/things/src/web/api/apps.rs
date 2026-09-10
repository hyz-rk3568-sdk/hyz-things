pub(crate) const APPS_ENDPOINT: &str = "/api/v1/apps";

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstalledAppDto {
    pub(crate) name: String,
    pub(crate) binary: String,
    pub(crate) init_script: String,
    #[serde(default)]
    pub(crate) sha256: Option<String>,
    #[serde(default)]
    pub(crate) deployed_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub(crate) protocol_versions: std::collections::BTreeMap<String, u32>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppsResponseDto {
    pub(crate) apps: Vec<InstalledAppDto>,
}
