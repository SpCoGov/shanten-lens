//! Plugin lifecycle, marketplace, updates, frontend bundles and process providers.
mod frontend;
mod manager;
mod manifest;
mod marketplace;
mod process;
#[cfg(test)]
mod tests;
mod updates;

use crate::pipeline::{ModuleRegistry, PacketModuleInfo, PacketOperation, PipelineConfig};
use process::ProcessProvider;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicU64, mpsc::SyncSender, Arc, Condvar, Mutex},
};
use tokio::sync::{broadcast, RwLock};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifest {
    id: String,
    name: String,
    version: String,
    description: Option<String>,
    author: Option<String>,
    homepage: Option<String>,
    source: Option<String>,
    issues: Option<String>,
    update: Option<PluginUpdateSource>,
    api_version: u32,
    backend: Option<BackendManifest>,
    frontend: Option<FrontendManifest>,
    default_locale: Option<String>,
    #[serde(default)]
    permissions: PluginPermissions,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginUpdateSource {
    manifest_url: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemotePluginUpdate {
    id: String,
    version: String,
    download_url: String,
    sha256: String,
    release_notes: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUpdateOffer {
    version: String,
    download_url: String,
    sha256: String,
    release_notes: Option<String>,
}

#[derive(Clone, Deserialize)]
struct FrontendManifest {
    entry: String,
}

#[derive(Clone, Deserialize)]
struct BackendManifest {
    #[serde(rename = "type")]
    kind: String,
    entry: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Clone, Default, Deserialize)]
struct PluginPermissions {
    #[serde(default)]
    packet: Vec<PacketOperation>,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub source: Option<String>,
    pub issues: Option<String>,
    pub has_update_source: bool,
    pub auto_update: bool,
    pub api_version: u32,
    pub enabled: bool,
    pub running: bool,
    pub has_frontend: bool,
    pub requested_packet_permissions: Vec<PacketOperation>,
    pub approved_packet_permissions: Vec<PacketOperation>,
    pub last_error: Option<String>,
    pub modules: Vec<PacketModuleInfo>,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateInfo {
    pub plugin_id: String,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub available: bool,
    pub release_notes: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MarketplacePluginStatus {
    Active,
    Deprecated,
    Blocked,
}

#[derive(Clone, Copy, Debug, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketplaceInstallBlock {
    Blocked,
    UnsupportedApi,
    AppTooOld,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePackageInfo {
    pub download_url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePluginInfo {
    pub source_url: String,
    pub marketplace_id: String,
    pub marketplace_name: String,
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub api_version: u32,
    pub minimum_app_version: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub source: Option<String>,
    pub repository: String,
    pub issues: Option<String>,
    pub license: Option<String>,
    pub categories: Vec<String>,
    pub package: MarketplacePackageInfo,
    pub release_notes: Option<String>,
    pub release_notes_url: Option<String>,
    pub published_at: String,
    pub status: MarketplacePluginStatus,
    pub install_block: Option<MarketplaceInstallBlock>,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSourceInfo {
    pub url: String,
    pub marketplace_id: Option<String>,
    pub name: Option<String>,
    pub default: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSnapshot {
    pub sources: Vec<MarketplaceSourceInfo>,
    pub plugins: Vec<MarketplacePluginInfo>,
}

#[derive(Default, Deserialize, Serialize)]
struct MarketplaceConfig {
    #[serde(default = "plugin_config_schema")]
    schema: u32,
    #[serde(default)]
    sources: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplaceRegistry {
    schema_version: u32,
    marketplace: MarketplaceIdentity,
    revision: String,
    generated_at: String,
    plugins: Vec<MarketplaceRegistryPlugin>,
}

#[derive(Deserialize)]
struct MarketplaceIdentity {
    id: String,
    name: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplaceRegistryPlugin {
    id: String,
    name: String,
    description: Option<String>,
    version: String,
    api_version: u32,
    minimum_app_version: Option<String>,
    author: Option<String>,
    homepage: Option<String>,
    source: Option<String>,
    repository: String,
    issues: Option<String>,
    license: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    package: MarketplacePackage,
    release_notes: Option<String>,
    release_notes_url: Option<String>,
    published_at: String,
    status: MarketplacePluginStatus,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplacePackage {
    download_url: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginFrontendBundle {
    pub plugin_id: String,
    pub source: Option<String>,
    pub entry_path: Option<String>,
    pub default_locale: String,
    pub locales: HashMap<String, Value>,
    pub error: Option<String>,
    pub health_token: Option<String>,
    pub generation: u64,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginScanError {
    pub path: String,
    pub error: String,
}

struct FrontendHealth {
    id: String,
    token: String,
    result: Option<Result<(), String>>,
}

#[derive(Default, Deserialize, Serialize)]
struct PluginConfig {
    #[serde(default = "plugin_config_schema")]
    schema: u32,
    #[serde(default)]
    plugins: HashMap<String, PluginChoice>,
}

fn plugin_config_schema() -> u32 {
    1
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginChoice {
    enabled: bool,
    #[serde(default)]
    auto_update: bool,
    #[serde(default)]
    approved_packet_permissions: Vec<PacketOperation>,
}

struct ManagedPlugin {
    directory: PathBuf,
    manifest: PluginManifest,
    choice: PluginChoice,
    provider: Option<ProcessProvider>,
    last_error: Option<String>,
    frontend_generation: u64,
}

pub struct PluginManager {
    root: PathBuf,
    config_path: PathBuf,
    marketplace_config_path: PathBuf,
    registry: Arc<ModuleRegistry>,
    pipeline: Arc<RwLock<PipelineConfig>>,
    pipeline_path: PathBuf,
    events: broadcast::Sender<Value>,
    update_requests: SyncSender<String>,
    update_lock: Mutex<()>,
    plugins: Mutex<HashMap<String, ManagedPlugin>>,
    scan_errors: Mutex<Vec<PluginScanError>>,
    frontend_health: Mutex<Option<FrontendHealth>>,
    health_changed: Condvar,
    frontend_generation: AtomicU64,
}

fn event(kind: &str, data: Value) -> Value {
    json!({"type":kind,"data":data})
}
