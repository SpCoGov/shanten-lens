use crate::pipeline::{
    self, ExternalDecision, ExternalPacketModule, ModuleConfig, ModuleRegistry, Packet,
    PacketModuleInfo, PacketOperation, PacketSubscription, PipelineConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use specta::Type;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, BufWriter, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Condvar, Mutex, RwLock as StdRwLock, Weak,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

const RPC_QUEUE_CAPACITY: usize = 256;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
const CIRCUIT_FAILURE_LIMIT: u8 = 3;
const CIRCUIT_RECOVERY_DELAY: Duration = Duration::from_secs(5);
const MAX_UPDATE_ARCHIVE_SIZE: u64 = 128 * 1024 * 1024;
const MAX_UPDATE_FILES: usize = 4_096;
const MAX_MARKETPLACE_REGISTRY_SIZE: u64 = 4 * 1024 * 1024;
const MAX_MARKETPLACE_PLUGINS: usize = 10_000;
const DEFAULT_MARKETPLACE_URL: &str =
    "https://raw.githubusercontent.com/SpCoGov/shanten-lens-marketplace/main/public/registry.json";

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

impl PluginManager {
    pub fn load(
        root: PathBuf,
        config_path: PathBuf,
        registry: Arc<ModuleRegistry>,
        pipeline: Arc<RwLock<PipelineConfig>>,
        pipeline_path: PathBuf,
        events: broadcast::Sender<Value>,
    ) -> anyhow::Result<Arc<Self>> {
        fs::create_dir_all(&root)?;
        let marketplace_config_path = config_path.with_file_name("plugin-marketplaces.json");
        let (update_requests, update_receiver) = mpsc::sync_channel(RPC_QUEUE_CAPACITY);
        let manager = Arc::new(Self {
            root,
            config_path,
            marketplace_config_path,
            registry,
            pipeline,
            pipeline_path,
            events,
            update_requests,
            update_lock: Mutex::new(()),
            plugins: Mutex::new(HashMap::new()),
            scan_errors: Mutex::new(Vec::new()),
            frontend_health: Mutex::new(None),
            health_changed: Condvar::new(),
            frontend_generation: AtomicU64::new(0),
        });
        manager.rescan()?;
        let automatic = Arc::downgrade(&manager);
        thread::spawn(move || {
            while let Ok(id) = update_receiver.recv() {
                let Some(manager) = automatic.upgrade() else {
                    break;
                };
                let enabled = manager
                    .plugins
                    .lock()
                    .unwrap()
                    .get(&id)
                    .is_some_and(|plugin| plugin.choice.auto_update);
                if enabled {
                    if let Err(error) = manager.update(&id) {
                        warn!(target: "shanten_backend::plugin", plugin_id = %id, %error, "automatic plugin update failed");
                    }
                }
            }
        });
        let updater = manager.clone();
        thread::spawn(move || updater.update_automatic());
        Ok(manager)
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        let plugins = self.plugins.lock().unwrap();
        let modules = self.registry.snapshot();
        let mut result: Vec<_> = plugins
            .values()
            .map(|plugin| PluginInfo {
                id: plugin.manifest.id.clone(),
                name: plugin.manifest.name.clone(),
                version: plugin.manifest.version.clone(),
                description: plugin.manifest.description.clone(),
                author: plugin.manifest.author.clone(),
                homepage: plugin.manifest.homepage.clone(),
                source: plugin.manifest.source.clone(),
                issues: plugin.manifest.issues.clone(),
                has_update_source: plugin.manifest.update.is_some()
                    || plugin.manifest.backend.is_some(),
                auto_update: plugin.choice.auto_update,
                api_version: plugin.manifest.api_version,
                enabled: plugin.choice.enabled,
                running: plugin.choice.enabled
                    && (plugin.manifest.backend.is_none()
                        || plugin
                            .provider
                            .as_ref()
                            .is_some_and(ProcessProvider::is_running)),
                has_frontend: plugin.manifest.frontend.is_some(),
                requested_packet_permissions: plugin.manifest.permissions.packet.clone(),
                approved_packet_permissions: plugin.choice.approved_packet_permissions.clone(),
                last_error: plugin.last_error.clone().or_else(|| {
                    plugin
                        .provider
                        .as_ref()
                        .and_then(ProcessProvider::last_error)
                }),
                modules: modules
                    .iter()
                    .filter(|module| {
                        module
                            .provider_id
                            .starts_with(&format!("{}:", plugin.manifest.id))
                    })
                    .cloned()
                    .collect(),
            })
            .collect();
        result.sort_by(|left, right| left.id.cmp(&right.id));
        result
    }

    pub fn marketplace(&self) -> MarketplaceSnapshot {
        let sources = self.marketplace_sources();
        let client = update_client();
        let mut source_info = Vec::with_capacity(sources.len());
        let mut plugins = Vec::new();
        for (url, default) in sources {
            let fetched = client
                .as_ref()
                .map_err(|error| anyhow::anyhow!(error.to_string()))
                .and_then(|client| fetch_marketplace_registry(client, &url));
            match fetched {
                Ok(registry) => {
                    let marketplace_id = registry.marketplace.id.clone();
                    let marketplace_name = registry.marketplace.name.clone();
                    plugins.extend(registry.plugins.into_iter().map(|plugin| {
                        marketplace_plugin_info(&url, &marketplace_id, &marketplace_name, plugin)
                    }));
                    source_info.push(MarketplaceSourceInfo {
                        url,
                        marketplace_id: Some(marketplace_id),
                        name: Some(marketplace_name),
                        default,
                        error: None,
                    });
                }
                Err(error) => source_info.push(MarketplaceSourceInfo {
                    url,
                    marketplace_id: None,
                    name: None,
                    default,
                    error: Some(error.to_string()),
                }),
            }
        }
        plugins.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left.source_url.cmp(&right.source_url))
        });
        MarketplaceSnapshot {
            sources: source_info,
            plugins,
        }
    }

    pub fn add_marketplace_source(&self, url: &str) -> anyhow::Result<MarketplaceSnapshot> {
        let url = update_url(url)?.to_string();
        anyhow::ensure!(
            url != update_url(DEFAULT_MARKETPLACE_URL)?.to_string(),
            "marketplace source is already the default"
        );
        fetch_marketplace_registry(&update_client()?, &url)?;
        let guard = self.update_lock.lock().unwrap();
        let mut config = self.load_marketplace_config();
        anyhow::ensure!(
            !config.sources.iter().any(|source| source == &url),
            "marketplace source already exists"
        );
        config.sources.push(url);
        self.save_marketplace_config(&config)?;
        drop(guard);
        Ok(self.marketplace())
    }

    pub fn remove_marketplace_source(&self, url: &str) -> anyhow::Result<MarketplaceSnapshot> {
        let _guard = self.update_lock.lock().unwrap();
        let url = update_url(url)?.to_string();
        anyhow::ensure!(
            url != update_url(DEFAULT_MARKETPLACE_URL)?.to_string(),
            "default marketplace source cannot be removed"
        );
        let mut config = self.load_marketplace_config();
        let previous = config.sources.len();
        config.sources.retain(|source| source != &url);
        anyhow::ensure!(
            config.sources.len() != previous,
            "marketplace source does not exist"
        );
        self.save_marketplace_config(&config)?;
        drop(_guard);
        Ok(self.marketplace())
    }

    pub fn install_marketplace_plugin(
        &self,
        source_url: &str,
        plugin_id: &str,
    ) -> anyhow::Result<Vec<PluginInfo>> {
        let source_url = update_url(source_url)?.to_string();
        anyhow::ensure!(
            self.marketplace_sources()
                .iter()
                .any(|(source, _)| source == &source_url),
            "unknown marketplace source"
        );
        let client = update_client()?;
        let registry = fetch_marketplace_registry(&client, &source_url)?;
        let plugin = registry
            .plugins
            .into_iter()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| anyhow::anyhow!("plugin is not present in marketplace source"))?;
        anyhow::ensure!(
            marketplace_install_block(&plugin)?.is_none(),
            "plugin cannot be installed on this app version"
        );
        let archive = download_bytes(
            &client,
            &plugin.package.download_url,
            MAX_UPDATE_ARCHIVE_SIZE,
        )?;
        anyhow::ensure!(
            archive.len() as u64 == plugin.package.size,
            "marketplace package size mismatch"
        );
        let actual = format!("{:x}", Sha256::digest(&archive));
        anyhow::ensure!(
            actual.eq_ignore_ascii_case(&plugin.package.sha256),
            "marketplace package SHA-256 mismatch"
        );
        self.install_expected(&archive, Some((&plugin.id, &plugin.version)))
    }

    pub fn rescan(&self) -> anyhow::Result<Vec<PluginInfo>> {
        let _guard = self.update_lock.lock().unwrap();
        self.rescan_locked()
    }

    pub fn scan_errors(&self) -> Vec<PluginScanError> {
        self.scan_errors.lock().unwrap().clone()
    }

    fn rescan_locked(&self) -> anyhow::Result<Vec<PluginInfo>> {
        let config = self.load_config();
        let frontend_generation = self.frontend_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let mut directories: Vec<_> = fs::read_dir(&self.root)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        directories.sort();
        let mut seen = HashSet::new();
        let mut discovered = HashMap::new();
        let mut errors = Vec::new();

        for directory in directories {
            match load_manifest(&directory).and_then(|manifest| {
                validate_plugin_entries(&directory, &manifest)?;
                Ok(manifest)
            }) {
                Ok(manifest) if seen.insert(manifest.id.clone()) => {
                    let choice = config
                        .plugins
                        .get(&manifest.id)
                        .cloned()
                        .unwrap_or_default();
                    discovered.insert(
                        manifest.id.clone(),
                        ManagedPlugin {
                            directory,
                            manifest,
                            choice,
                            provider: None,
                            last_error: None,
                            frontend_generation,
                        },
                    );
                }
                Ok(manifest) => {
                    errors.push(PluginScanError {
                        path: directory.display().to_string(),
                        error: format!("duplicate plugin id: {}", manifest.id),
                    });
                    warn!(target: "shanten_backend::plugin", plugin_id = %manifest.id, "duplicate plugin id")
                }
                Err(error) => {
                    errors.push(PluginScanError {
                        path: directory.display().to_string(),
                        error: error.to_string(),
                    });
                    warn!(target: "shanten_backend::plugin", path = %directory.display(), %error, "failed to load plugin")
                }
            }
        }

        // Stop every old provider before any replacement can register the same module IDs.
        self.plugins.lock().unwrap().clear();
        // Only prune confirmed missing plugins; a broken manifest must not erase its settings.
        if errors.is_empty() {
            let mut orphaned = HashSet::new();
            for module in self.pipeline.blocking_read().modules.iter() {
                if let Some(id) = module
                    .id
                    .strip_prefix("plugin:")
                    .and_then(|id| id.split(':').next())
                {
                    if !discovered.contains_key(id) {
                        orphaned.insert(id.to_owned());
                    }
                }
            }
            for module in self.registry.snapshot() {
                if !module.builtin {
                    if let Some(id) = module
                        .id
                        .strip_prefix("plugin:")
                        .and_then(|id| id.split(':').next())
                    {
                        if !discovered.contains_key(id) {
                            orphaned.insert(id.to_owned());
                        }
                    }
                }
            }
            for id in orphaned {
                self.remove_plugin_modules(&id)?;
            }
        }
        *self.scan_errors.lock().unwrap() = errors;
        self.emit_status();
        for plugin in discovered
            .values_mut()
            .filter(|plugin| plugin.choice.enabled)
        {
            let plugin_config_path = self.plugin_config_path(&plugin.manifest.id);
            start_managed_plugin(
                plugin,
                plugin_config_path,
                self.registry.clone(),
                self.pipeline.clone(),
                self.pipeline_path.clone(),
                self.events.clone(),
                self.update_requests.clone(),
            );
        }
        *self.plugins.lock().unwrap() = discovered;
        self.emit_status();
        Ok(self.list())
    }

    pub fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        approved_permissions: Vec<PacketOperation>,
    ) -> anyhow::Result<Vec<PluginInfo>> {
        let _guard = self.update_lock.lock().unwrap();
        {
            let mut plugins = self.plugins.lock().unwrap();
            let plugin = plugins
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?;
            let requested: HashSet<_> =
                plugin.manifest.permissions.packet.iter().copied().collect();
            anyhow::ensure!(
                approved_permissions
                    .iter()
                    .all(|permission| requested.contains(permission)),
                "approved permissions exceed plugin manifest"
            );
            plugin.provider = None;
            plugin.choice.enabled = enabled;
            plugin.choice.approved_packet_permissions = approved_permissions;
            plugin.last_error = None;
            if enabled {
                let plugin_config_path = self.plugin_config_path(id);
                start_managed_plugin(
                    plugin,
                    plugin_config_path,
                    self.registry.clone(),
                    self.pipeline.clone(),
                    self.pipeline_path.clone(),
                    self.events.clone(),
                    self.update_requests.clone(),
                );
            }
            self.save_config(&plugins)?;
        }
        self.emit_status();
        Ok(self.list())
    }

    pub fn restart(&self, id: &str) -> anyhow::Result<Vec<PluginInfo>> {
        let _guard = self.update_lock.lock().unwrap();
        {
            let mut plugins = self.plugins.lock().unwrap();
            let plugin = plugins
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?;
            anyhow::ensure!(plugin.choice.enabled, "plugin is disabled");
            plugin.provider = None;
            plugin.last_error = None;
            plugin.frontend_generation =
                self.frontend_generation.fetch_add(1, Ordering::Relaxed) + 1;
            let plugin_config_path = self.plugin_config_path(id);
            start_managed_plugin(
                plugin,
                plugin_config_path,
                self.registry.clone(),
                self.pipeline.clone(),
                self.pipeline_path.clone(),
                self.events.clone(),
                self.update_requests.clone(),
            );
        }
        self.emit_status();
        Ok(self.list())
    }

    pub fn set_auto_update(&self, id: &str, enabled: bool) -> anyhow::Result<Vec<PluginInfo>> {
        let _guard = self.update_lock.lock().unwrap();
        let mut plugins = self.plugins.lock().unwrap();
        let plugin = plugins
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?;
        anyhow::ensure!(
            plugin.manifest.update.is_some() || plugin.manifest.backend.is_some(),
            "plugin has no update channel"
        );
        plugin.choice.auto_update = enabled;
        self.save_config(&plugins)?;
        drop(plugins);
        self.emit_status();
        Ok(self.list())
    }

    pub fn check_updates(&self) -> Vec<PluginUpdateInfo> {
        let sources: Vec<_> = self
            .plugins
            .lock()
            .unwrap()
            .values()
            .filter_map(|plugin| {
                let provider = plugin.provider.as_ref();
                let manifest_url = plugin
                    .manifest
                    .update
                    .as_ref()
                    .map(|source| source.manifest_url.clone());
                if manifest_url.is_none() && plugin.manifest.backend.is_none() {
                    return None;
                }
                if let Some(provider) = provider {
                    provider.check_update();
                }
                Some((
                    plugin.manifest.id.clone(),
                    plugin.manifest.version.clone(),
                    manifest_url,
                    provider.and_then(ProcessProvider::update_offer),
                ))
            })
            .collect();
        let client = update_client();
        sources
            .into_iter()
            .map(|(plugin_id, current_version, manifest_url, offer)| {
                let fetched = match (offer, manifest_url, &client) {
                    (Some(update), _, _) => Ok(Some(update)),
                    (None, Some(manifest_url), Ok(client)) => {
                        fetch_update(client, &manifest_url, &plugin_id)
                            .map(Some)
                            .map_err(|error| error.to_string())
                    }
                    (None, Some(_), Err(error)) => Err(error.to_string()),
                    (None, None, _) => Ok(None),
                };
                match fetched {
                    Ok(Some(update)) => match is_newer(&current_version, &update.version) {
                        Ok(available) => PluginUpdateInfo {
                            plugin_id,
                            current_version,
                            latest_version: Some(update.version),
                            available,
                            release_notes: update.release_notes,
                            error: None,
                        },
                        Err(error) => PluginUpdateInfo {
                            plugin_id,
                            current_version,
                            latest_version: None,
                            available: false,
                            release_notes: None,
                            error: Some(error.to_string()),
                        },
                    },
                    Ok(None) => PluginUpdateInfo {
                        plugin_id,
                        current_version,
                        latest_version: None,
                        available: false,
                        release_notes: None,
                        error: None,
                    },
                    Err(error) => PluginUpdateInfo {
                        plugin_id,
                        current_version,
                        latest_version: None,
                        available: false,
                        release_notes: None,
                        error: Some(error),
                    },
                }
            })
            .collect()
    }

    pub fn update(&self, id: &str) -> anyhow::Result<Vec<PluginInfo>> {
        let _update_guard = self.update_lock.lock().unwrap();
        let result = self.update_locked(id);
        if let Err(error) = &result {
            let mut plugins = self.plugins.lock().unwrap();
            if let Some(plugin) = plugins.get_mut(id) {
                plugin.last_error = Some(format!("plugin update failed: {error}"));
                plugin.choice.auto_update = false;
            }
            if let Err(save_error) = self.save_config(&plugins) {
                warn!(target: "shanten_backend::plugin", %save_error, "failed to save plugin update failure state");
            }
            drop(plugins);
            self.emit_status();
        }
        result
    }

    fn update_locked(&self, id: &str) -> anyhow::Result<Vec<PluginInfo>> {
        let (directory, current_version, manifest_url, offer) = {
            let plugins = self.plugins.lock().unwrap();
            let plugin = plugins
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?;
            (
                plugin.directory.clone(),
                plugin.manifest.version.clone(),
                plugin
                    .manifest
                    .update
                    .as_ref()
                    .map(|source| source.manifest_url.clone()),
                plugin
                    .provider
                    .as_ref()
                    .and_then(ProcessProvider::update_offer),
            )
        };
        let client = update_client()?;
        let update = match (offer, manifest_url) {
            (Some(update), _) => update,
            (None, Some(manifest_url)) => fetch_update(&client, &manifest_url, id)?,
            (None, None) => anyhow::bail!("plugin has not reported an update"),
        };
        anyhow::ensure!(
            is_newer(&current_version, &update.version)?,
            "plugin is already up to date"
        );
        let archive = download_update(&client, &update)?;
        self.apply_update_archive(id, &directory, &update.version, &archive)
    }

    fn apply_update_archive(
        &self,
        id: &str,
        directory: &Path,
        version: &str,
        archive: &[u8],
    ) -> anyhow::Result<Vec<PluginInfo>> {
        let parent = self
            .root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("plugin root has no parent"))?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let staging = parent.join(format!(".plugin-update-{id}-{nonce}"));
        let backup = parent.join(format!(".plugin-backup-{id}-{nonce}"));
        fs::create_dir(&staging)?;
        if let Err(error) = extract_update(&archive, &staging) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let installed_manifest = match load_manifest(&staging) {
            Ok(manifest) => manifest,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(error);
            }
        };
        if installed_manifest.id != id || installed_manifest.version != version {
            let _ = fs::remove_dir_all(&staging);
            anyhow::bail!("updated plugin identity or version does not match update manifest");
        }
        if let Err(error) = validate_plugin_entries(&staging, &installed_manifest) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }

        let config_path = self.plugin_config_path(id);
        self.plugins.lock().unwrap().remove(id);
        self.emit_status();
        let old_config = match fs::read(&config_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                let _ = self.rescan_locked();
                return Err(error.into());
            }
        };
        if let Err(error) = fs::rename(&directory, &backup) {
            let _ = fs::remove_dir_all(&staging);
            let _ = self.rescan_locked();
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&staging, &directory) {
            fs::rename(&backup, &directory)?;
            let _ = fs::remove_dir_all(&staging);
            let _ = self.rescan_locked();
            return Err(error.into());
        }
        let needs_frontend = self
            .load_config()
            .plugins
            .get(id)
            .is_some_and(|choice| choice.enabled)
            && installed_manifest.frontend.is_some();
        if needs_frontend {
            *self.frontend_health.lock().unwrap() = Some(FrontendHealth {
                id: id.to_owned(),
                token: nonce.to_string(),
                result: None,
            });
        }
        let checked = self
            .rescan_locked()
            .and_then(|_| self.check_health(id, needs_frontend));
        *self.frontend_health.lock().unwrap() = None;
        match checked {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup);
                Ok(self.list())
            }
            Err(error) => {
                self.plugins.lock().unwrap().remove(id);
                self.emit_status();
                // Keep the failed version aside until the old directory has been restored.
                fs::rename(&directory, &staging)?;
                fs::rename(&backup, &directory)?;
                if let Some(bytes) = old_config {
                    fs::write(&config_path, bytes)?;
                } else if config_path.exists() {
                    fs::remove_file(&config_path)?;
                }
                let _ = fs::remove_dir_all(&staging);
                self.rescan_locked()?;
                let message = format!("plugin update rolled back: {error}");
                let mut plugins = self.plugins.lock().unwrap();
                if let Some(plugin) = plugins.get_mut(id) {
                    plugin.last_error = Some(message.clone());
                    // A restored provider may report the same bad update on startup.
                    plugin.choice.auto_update = false;
                }
                self.save_config(&plugins)?;
                drop(plugins);
                self.emit_status();
                anyhow::bail!(message)
            }
        }
    }

    fn check_health(&self, id: &str, needs_frontend: bool) -> anyhow::Result<()> {
        // Catch providers that acknowledge hello but exit immediately on host.ready.
        thread::sleep(Duration::from_millis(500));
        self.check_backend_health(id)?;
        if needs_frontend {
            let health = self.frontend_health.lock().unwrap();
            let (health, _) = self
                .health_changed
                .wait_timeout_while(health, Duration::from_secs(12), |health| {
                    health
                        .as_ref()
                        .is_some_and(|health| health.result.is_none())
                })
                .unwrap();
            health
                .as_ref()
                .and_then(|health| health.result.clone())
                .unwrap_or_else(|| Err("plugin frontend health check timed out".into()))
                .map_err(anyhow::Error::msg)?;
        }
        self.check_backend_health(id)
    }

    fn check_backend_health(&self, id: &str) -> anyhow::Result<()> {
        let plugins = self.plugins.lock().unwrap();
        let plugin = plugins
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("updated plugin was not discovered"))?;
        if let Some(error) = &plugin.last_error {
            anyhow::bail!("{error}");
        }
        if plugin.choice.enabled && plugin.manifest.backend.is_some() {
            anyhow::ensure!(
                plugin
                    .provider
                    .as_ref()
                    .is_some_and(ProcessProvider::is_running),
                "updated plugin backend is offline"
            );
        }
        Ok(())
    }

    pub fn report_frontend_health(&self, id: &str, token: &str, error: Option<String>) {
        let mut health = self.frontend_health.lock().unwrap();
        if let Some(health) = health
            .as_mut()
            .filter(|health| health.id == id && health.token == token && health.result.is_none())
        {
            health.result = Some(error.map_or(Ok(()), Err));
            self.health_changed.notify_all();
        }
    }

    pub fn install(&self, archive: &[u8]) -> anyhow::Result<Vec<PluginInfo>> {
        self.install_expected(archive, None)
    }

    fn install_expected(
        &self,
        archive: &[u8],
        expected: Option<(&str, &str)>,
    ) -> anyhow::Result<Vec<PluginInfo>> {
        let _guard = self.update_lock.lock().unwrap();
        anyhow::ensure!(
            archive.len() as u64 <= MAX_UPDATE_ARCHIVE_SIZE,
            "plugin archive is too large"
        );
        let parent = self
            .root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("plugin root has no parent"))?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let staging = parent.join(format!(".plugin-install-{nonce}"));
        fs::create_dir(&staging)?;
        let result = (|| -> anyhow::Result<()> {
            extract_update(archive, &staging)?;
            let manifest = load_manifest(&staging)?;
            if let Some((id, version)) = expected {
                anyhow::ensure!(
                    manifest.id == id && manifest.version == version,
                    "plugin identity or version does not match marketplace"
                );
            }
            validate_plugin_entries(&staging, &manifest)?;
            anyhow::ensure!(
                !self.plugins.lock().unwrap().contains_key(&manifest.id),
                "plugin is already installed"
            );
            let destination = self.root.join(&manifest.id);
            anyhow::ensure!(!destination.exists(), "plugin directory already exists");
            // Reinstalling a previously removed plugin must never reuse an enabled choice.
            let mut config = self.load_config();
            config
                .plugins
                .insert(manifest.id.clone(), PluginChoice::default());
            fs::write(&self.config_path, serde_json::to_vec_pretty(&config)?)?;
            fs::rename(&staging, destination)?;
            Ok(())
        })();
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result?;
        self.rescan_locked()
    }

    pub fn uninstall(&self, id: &str, remove_config: bool) -> anyhow::Result<Vec<PluginInfo>> {
        let _guard = self.update_lock.lock().unwrap();
        let directory = self
            .plugins
            .lock()
            .unwrap()
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?
            .directory
            .canonicalize()?;
        let root = self.root.canonicalize()?;
        anyhow::ensure!(
            directory != root && directory.starts_with(&root),
            "plugin directory escapes plugin root"
        );
        let mut config = self.load_config();
        config.plugins.remove(id);
        self.plugins.lock().unwrap().remove(id);
        self.emit_status();
        if let Err(error) = fs::remove_dir_all(&directory) {
            let _ = self.rescan_locked();
            return Err(error.into());
        }
        self.remove_plugin_modules(id)?;
        fs::write(&self.config_path, serde_json::to_vec_pretty(&config)?)?;
        if remove_config {
            let path = self.plugin_config_path(id);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        self.rescan_locked()
    }

    fn remove_plugin_modules(&self, id: &str) -> anyhow::Result<()> {
        let prefix = format!("plugin:{id}:");
        let mut config = self.pipeline.blocking_write();
        let mut updated = config.clone();
        updated
            .modules
            .retain(|module| !module.id.starts_with(&prefix));
        pipeline::save(&self.pipeline_path, &updated).map_err(anyhow::Error::msg)?;
        *config = updated;
        self.registry.remove_plugin(id);
        let _ = self
            .events
            .send(event("packet_pipeline", serde_json::to_value(&*config)?));
        let _ = self.events.send(event(
            "packet_modules",
            serde_json::to_value(self.registry.snapshot())?,
        ));
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn frontend_bundles(&self) -> Vec<PluginFrontendBundle> {
        self.plugins
            .lock()
            .unwrap()
            .values()
            .filter(|plugin| plugin.choice.enabled && plugin.manifest.frontend.is_some())
            .map(|plugin| {
                let mut bundle = load_frontend_bundle(plugin);
                bundle.generation = plugin.frontend_generation;
                bundle.health_token = self
                    .frontend_health
                    .lock()
                    .unwrap()
                    .as_ref()
                    .filter(|health| health.id == plugin.manifest.id)
                    .map(|health| health.token.clone());
                bundle
            })
            .collect()
    }

    pub fn get_config(&self, id: &str) -> anyhow::Result<Value> {
        anyhow::ensure!(
            self.plugins.lock().unwrap().contains_key(id),
            "unknown plugin: {id}"
        );
        let path = self.plugin_config_path(id);
        if !path.exists() {
            return Ok(json!({}));
        }
        let value: Value = serde_json::from_slice(&fs::read(path)?)?;
        anyhow::ensure!(value.is_object(), "plugin config must be a JSON object");
        Ok(value)
    }

    pub fn set_config(&self, id: &str, value: Value) -> anyhow::Result<()> {
        anyhow::ensure!(value.is_object(), "plugin config must be a JSON object");
        let plugins = self.plugins.lock().unwrap();
        anyhow::ensure!(plugins.contains_key(id), "unknown plugin: {id}");
        let path = self.plugin_config_path(id);
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("invalid config path"))?,
        )?;
        fs::write(path, serde_json::to_vec_pretty(&value)?)?;
        Ok(())
    }

    pub fn invoke(&self, id: &str, method: &str, params: Value) -> anyhow::Result<Value> {
        anyhow::ensure!(!method.trim().is_empty(), "plugin method cannot be empty");
        let plugins = self.plugins.lock().unwrap();
        let plugin = plugins
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?;
        anyhow::ensure!(plugin.choice.enabled, "plugin is disabled");
        plugin
            .provider
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("plugin has no running backend provider"))?
            .invoke(method, params)
            .map_err(anyhow::Error::msg)
    }

    fn plugin_config_path(&self, id: &str) -> PathBuf {
        self.config_path
            .parent()
            .unwrap_or(&self.root)
            .join("plugin-config")
            .join(format!("{id}.json"))
    }

    fn load_config(&self) -> PluginConfig {
        fs::read(&self.config_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| PluginConfig {
                schema: 1,
                plugins: HashMap::new(),
            })
    }

    fn marketplace_sources(&self) -> Vec<(String, bool)> {
        let default = update_url(DEFAULT_MARKETPLACE_URL)
            .expect("default marketplace URL is valid")
            .to_string();
        let mut sources = vec![(default.clone(), true)];
        for source in self.load_marketplace_config().sources {
            if source != default && !sources.iter().any(|(url, _)| url == &source) {
                sources.push((source, false));
            }
        }
        sources
    }

    fn load_marketplace_config(&self) -> MarketplaceConfig {
        fs::read(&self.marketplace_config_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| MarketplaceConfig {
                schema: 1,
                sources: Vec::new(),
            })
    }

    fn save_marketplace_config(&self, config: &MarketplaceConfig) -> anyhow::Result<()> {
        fs::write(
            &self.marketplace_config_path,
            serde_json::to_vec_pretty(config)?,
        )?;
        Ok(())
    }

    fn save_config(&self, plugins: &HashMap<String, ManagedPlugin>) -> anyhow::Result<()> {
        let mut config = self.load_config();
        config.schema = 1;
        config.plugins.extend(
            plugins
                .iter()
                .map(|(id, plugin)| (id.clone(), plugin.choice.clone())),
        );
        fs::write(&self.config_path, serde_json::to_vec_pretty(&config)?)?;
        Ok(())
    }

    fn emit_status(&self) {
        let _ = self.events.send(event(
            "plugin_status",
            serde_json::to_value(self.list()).unwrap_or(Value::Null),
        ));
    }

    fn update_automatic(&self) {
        let enabled: HashSet<_> = self
            .plugins
            .lock()
            .unwrap()
            .values()
            .filter(|plugin| plugin.choice.auto_update)
            .map(|plugin| plugin.manifest.id.clone())
            .collect();
        for update in self
            .check_updates()
            .into_iter()
            .filter(|update| update.available && enabled.contains(&update.plugin_id))
        {
            if let Err(error) = self.update(&update.plugin_id) {
                warn!(target: "shanten_backend::plugin", plugin_id = %update.plugin_id, %error, "automatic plugin update failed");
            }
        }
    }
}

fn update_client() -> anyhow::Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("Shanten-Lens-Plugin-Updater")
        .timeout(Duration::from_secs(20))
        .build()?)
}

fn update_url(value: &str) -> anyhow::Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)?;
    let local_http = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    anyhow::ensure!(
        url.scheme() == "https" || local_http,
        "plugin update URL must use HTTPS"
    );
    Ok(url)
}

fn fetch_update(
    client: &reqwest::blocking::Client,
    manifest_url: &str,
    plugin_id: &str,
) -> anyhow::Result<RemotePluginUpdate> {
    let url = update_url(manifest_url)?;
    let mut update: RemotePluginUpdate =
        client.get(url.clone()).send()?.error_for_status()?.json()?;
    anyhow::ensure!(
        update.id == plugin_id,
        "update manifest plugin id does not match"
    );
    update.download_url = url.join(&update.download_url)?.to_string();
    validate_update_offer(&update.version, &update.download_url, &update.sha256)?;
    Ok(update)
}

fn fetch_marketplace_registry(
    client: &reqwest::blocking::Client,
    registry_url: &str,
) -> anyhow::Result<MarketplaceRegistry> {
    let bytes = download_bytes(client, registry_url, MAX_MARKETPLACE_REGISTRY_SIZE)?;
    let registry: MarketplaceRegistry = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        registry.schema_version == 1,
        "unsupported marketplace schema version"
    );
    validate_identifier(&registry.marketplace.id, "marketplace id")?;
    anyhow::ensure!(
        !registry.marketplace.name.trim().is_empty(),
        "marketplace name cannot be empty"
    );
    anyhow::ensure!(
        !registry.revision.trim().is_empty() && !registry.generated_at.trim().is_empty(),
        "marketplace revision or generation time is missing"
    );
    anyhow::ensure!(
        registry.plugins.len() <= MAX_MARKETPLACE_PLUGINS,
        "marketplace contains too many plugins"
    );
    let mut ids = HashSet::new();
    for plugin in &registry.plugins {
        validate_identifier(&plugin.id, "marketplace plugin id")?;
        anyhow::ensure!(
            ids.insert(plugin.id.as_str()),
            "marketplace contains duplicate plugin ids"
        );
        anyhow::ensure!(
            !plugin.name.trim().is_empty(),
            "marketplace plugin name cannot be empty"
        );
        semver::Version::parse(&plugin.version)?;
        anyhow::ensure!(plugin.api_version > 0, "marketplace plugin API is invalid");
        if let Some(version) = &plugin.minimum_app_version {
            semver::Version::parse(version)?;
        }
        for (label, value) in [
            ("homepage", plugin.homepage.as_deref()),
            ("source", plugin.source.as_deref()),
            ("repository", Some(plugin.repository.as_str())),
            ("issues", plugin.issues.as_deref()),
            ("release notes", plugin.release_notes_url.as_deref()),
        ] {
            if let Some(value) = value {
                update_url(value)
                    .map_err(|error| anyhow::anyhow!("invalid {label} URL: {error}"))?;
            }
        }
        anyhow::ensure!(
            plugin.categories.iter().collect::<HashSet<_>>().len() == plugin.categories.len(),
            "marketplace plugin categories must be unique"
        );
        update_url(&plugin.package.download_url)?;
        anyhow::ensure!(
            plugin.package.size > 0 && plugin.package.size <= MAX_UPDATE_ARCHIVE_SIZE,
            "marketplace package size is invalid"
        );
        anyhow::ensure!(
            plugin.package.sha256.len() == 64
                && plugin
                    .package
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "marketplace package SHA-256 is invalid"
        );
        anyhow::ensure!(
            !plugin.published_at.trim().is_empty(),
            "marketplace plugin publish time is missing"
        );
    }
    Ok(registry)
}

fn marketplace_install_block(
    plugin: &MarketplaceRegistryPlugin,
) -> anyhow::Result<Option<MarketplaceInstallBlock>> {
    if plugin.status == MarketplacePluginStatus::Blocked {
        return Ok(Some(MarketplaceInstallBlock::Blocked));
    }
    if plugin.api_version != 1 {
        return Ok(Some(MarketplaceInstallBlock::UnsupportedApi));
    }
    if let Some(minimum) = &plugin.minimum_app_version {
        if semver::Version::parse(minimum)? > semver::Version::parse(crate::VERSION)? {
            return Ok(Some(MarketplaceInstallBlock::AppTooOld));
        }
    }
    Ok(None)
}

fn marketplace_plugin_info(
    source_url: &str,
    marketplace_id: &str,
    marketplace_name: &str,
    plugin: MarketplaceRegistryPlugin,
) -> MarketplacePluginInfo {
    let install_block = marketplace_install_block(&plugin).expect("marketplace plugin validated");
    MarketplacePluginInfo {
        source_url: source_url.to_owned(),
        marketplace_id: marketplace_id.to_owned(),
        marketplace_name: marketplace_name.to_owned(),
        id: plugin.id,
        name: plugin.name,
        description: plugin.description,
        version: plugin.version,
        api_version: plugin.api_version,
        minimum_app_version: plugin.minimum_app_version,
        author: plugin.author,
        homepage: plugin.homepage,
        source: plugin.source,
        repository: plugin.repository,
        issues: plugin.issues,
        license: plugin.license,
        categories: plugin.categories,
        package: MarketplacePackageInfo {
            download_url: plugin.package.download_url,
            sha256: plugin.package.sha256,
            size: plugin.package.size,
        },
        release_notes: plugin.release_notes,
        release_notes_url: plugin.release_notes_url,
        published_at: plugin.published_at,
        status: plugin.status,
        install_block,
    }
}

fn validate_update_offer(version: &str, download_url: &str, sha256: &str) -> anyhow::Result<()> {
    semver::Version::parse(version)?;
    update_url(download_url)?;
    anyhow::ensure!(
        sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "update SHA-256 is invalid"
    );
    Ok(())
}

fn is_newer(current: &str, latest: &str) -> anyhow::Result<bool> {
    Ok(semver::Version::parse(latest)? > semver::Version::parse(current)?)
}

fn download_update(
    client: &reqwest::blocking::Client,
    update: &RemotePluginUpdate,
) -> anyhow::Result<Vec<u8>> {
    let bytes = download_bytes(client, &update.download_url, MAX_UPDATE_ARCHIVE_SIZE)?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(&update.sha256),
        "plugin update SHA-256 mismatch"
    );
    Ok(bytes)
}

fn download_bytes(
    client: &reqwest::blocking::Client,
    url: &str,
    limit: u64,
) -> anyhow::Result<Vec<u8>> {
    let url = update_url(url)?;
    let mut response = client.get(url).send()?.error_for_status()?;
    update_url(response.url().as_str())?;
    anyhow::ensure!(
        response.content_length().is_none_or(|size| size <= limit),
        "download is too large"
    );
    let mut bytes = Vec::new();
    response.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= limit, "download is too large");
    Ok(bytes)
}

fn extract_update(archive: &[u8], destination: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive))?;
    anyhow::ensure!(
        zip.len() <= MAX_UPDATE_FILES,
        "plugin update contains too many files"
    );
    let mut total_size = 0_u64;
    for index in 0..zip.len() {
        let mut file = zip.by_index(index)?;
        total_size = total_size.saturating_add(file.size());
        anyhow::ensure!(
            total_size <= MAX_UPDATE_ARCHIVE_SIZE,
            "plugin update expands beyond size limit"
        );
        let relative = file
            .enclosed_name()
            .ok_or_else(|| anyhow::anyhow!("plugin update contains an unsafe path"))?;
        let target = destination.join(relative);
        if file.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        fs::create_dir_all(
            target
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid update path"))?,
        )?;
        let mut output = fs::File::create(&target)?;
        std::io::copy(&mut file, &mut output)?;
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
        }
    }
    anyhow::ensure!(
        destination.join("plugin.json").is_file(),
        "plugin update ZIP must contain plugin.json at its root"
    );
    Ok(())
}

fn load_frontend_bundle(plugin: &ManagedPlugin) -> PluginFrontendBundle {
    let result = (|| -> anyhow::Result<(String, String, HashMap<String, Value>)> {
        let frontend = plugin.manifest.frontend.as_ref().expect("frontend checked");
        let directory = plugin.directory.canonicalize()?;
        let entry = directory.join(&frontend.entry).canonicalize()?;
        anyhow::ensure!(
            entry.starts_with(&directory),
            "plugin frontend entry escapes plugin directory"
        );
        anyhow::ensure!(entry.is_file(), "plugin frontend entry is not a file");
        let source = fs::read_to_string(&entry)?;
        let entry_path = webview_path(&entry);
        let mut locales = HashMap::new();
        let locale_dir = directory.join("locales");
        if locale_dir.is_dir() {
            for entry in fs::read_dir(locale_dir)?.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let Some(locale) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let value: Value = serde_json::from_slice(&fs::read(&path)?)?;
                anyhow::ensure!(value.is_object(), "plugin locale must be a JSON object");
                locales.insert(locale.to_owned(), value);
            }
        }
        Ok((source, entry_path, locales))
    })();
    match result {
        Ok((source, entry_path, locales)) => PluginFrontendBundle {
            plugin_id: plugin.manifest.id.clone(),
            health_token: None,
            generation: 0,
            source: Some(source),
            entry_path: Some(entry_path),
            default_locale: plugin
                .manifest
                .default_locale
                .clone()
                .unwrap_or_else(|| "zh-CN".into()),
            locales,
            error: None,
        },
        Err(error) => PluginFrontendBundle {
            plugin_id: plugin.manifest.id.clone(),
            health_token: None,
            generation: 0,
            source: None,
            entry_path: None,
            default_locale: plugin
                .manifest
                .default_locale
                .clone()
                .unwrap_or_else(|| "zh-CN".into()),
            locales: HashMap::new(),
            error: Some(error.to_string()),
        },
    }
}

fn webview_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(path) = value.strip_prefix(r"\\?\UNC\") {
            return format!(r"//{path}").replace('\\', "/");
        }
        return value
            .strip_prefix(r"\\?\")
            .unwrap_or(&value)
            .replace('\\', "/");
    }
    #[cfg(not(windows))]
    value.into_owned()
}

fn start_managed_plugin(
    plugin: &mut ManagedPlugin,
    config_path: PathBuf,
    registry: Arc<ModuleRegistry>,
    pipeline: Arc<RwLock<PipelineConfig>>,
    pipeline_path: PathBuf,
    events: broadcast::Sender<Value>,
    update_requests: SyncSender<String>,
) {
    match ProcessProvider::start(
        plugin.directory.clone(),
        plugin.manifest.clone(),
        config_path,
        plugin
            .choice
            .approved_packet_permissions
            .iter()
            .copied()
            .filter(|permission| plugin.manifest.permissions.packet.contains(permission))
            .collect(),
        registry,
        pipeline,
        pipeline_path,
        events,
        update_requests,
    ) {
        Ok(provider) => plugin.provider = provider,
        Err(error) => {
            plugin.last_error = Some(error.to_string());
            warn!(target: "shanten_backend::plugin", plugin_id = %plugin.manifest.id, %error, "failed to start plugin");
        }
    }
}

fn load_manifest(directory: &Path) -> anyhow::Result<PluginManifest> {
    let path = directory.join("plugin.json");
    let manifest: PluginManifest = serde_json::from_slice(&fs::read(&path)?)?;
    validate_identifier(&manifest.id, "plugin id")?;
    anyhow::ensure!(
        !manifest.name.trim().is_empty(),
        "plugin name cannot be empty"
    );
    anyhow::ensure!(
        !manifest.version.trim().is_empty(),
        "plugin version cannot be empty"
    );
    anyhow::ensure!(manifest.api_version == 1, "unsupported plugin API version");
    for (label, value) in [
        ("homepage", manifest.homepage.as_deref()),
        ("source", manifest.source.as_deref()),
        ("issues", manifest.issues.as_deref()),
    ] {
        if let Some(value) = value {
            let url = reqwest::Url::parse(value)?;
            anyhow::ensure!(
                matches!(url.scheme(), "http" | "https"),
                "plugin {label} URL must use HTTP or HTTPS"
            );
        }
    }
    if let Some(update) = &manifest.update {
        update_url(&update.manifest_url)?;
    }
    anyhow::ensure!(
        manifest.backend.is_some() || manifest.frontend.is_some(),
        "plugin has no backend or frontend entry"
    );
    Ok(manifest)
}

fn validate_plugin_entries(directory: &Path, manifest: &PluginManifest) -> anyhow::Result<()> {
    let root = directory.canonicalize()?;
    for (label, entry) in [
        (
            "backend",
            manifest.backend.as_ref().map(|value| value.entry.as_str()),
        ),
        (
            "frontend",
            manifest.frontend.as_ref().map(|value| value.entry.as_str()),
        ),
    ] {
        let Some(entry) = entry else { continue };
        let path = root.join(entry).canonicalize()?;
        anyhow::ensure!(
            path.starts_with(&root) && path.is_file(),
            "plugin {label} entry is invalid"
        );
    }
    Ok(())
}

struct ProcessProvider {
    core: Arc<ProviderCore>,
    child: Arc<Mutex<Option<Child>>>,
}

impl ProcessProvider {
    fn start(
        directory: PathBuf,
        manifest: PluginManifest,
        config_path: PathBuf,
        approved_permissions: Vec<PacketOperation>,
        registry: Arc<ModuleRegistry>,
        pipeline: Arc<RwLock<PipelineConfig>>,
        pipeline_path: PathBuf,
        events: broadcast::Sender<Value>,
        update_requests: SyncSender<String>,
    ) -> anyhow::Result<Option<Self>> {
        let Some(backend) = manifest.backend.clone() else {
            return Ok(None);
        };
        anyhow::ensure!(backend.kind == "process", "unsupported backend type");
        let directory = directory.canonicalize()?;
        let entry = directory.join(&backend.entry).canonicalize()?;
        anyhow::ensure!(
            entry.starts_with(&directory),
            "plugin entry escapes plugin directory"
        );
        anyhow::ensure!(entry.is_file(), "plugin entry is not a file");

        let mut command = Command::new(&entry);
        command
            .args(&backend.args)
            .current_dir(&directory)
            .env("SHANTEN_LENS_PLUGIN_ID", &manifest.id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("plugin stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("plugin stdout missing"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("plugin stderr missing"))?;
        let child = Arc::new(Mutex::new(Some(child)));
        let (writer, writer_rx) = mpsc::sync_channel(RPC_QUEUE_CAPACITY);
        let core = Arc::new(ProviderCore {
            plugin_id: manifest.id.clone(),
            current_version: manifest.version.clone(),
            config_path,
            permissions: approved_permissions.into_iter().collect(),
            online: AtomicBool::new(false),
            disconnected: Mutex::new(false),
            last_error: Mutex::new(None),
            provider_id: StdRwLock::new(None),
            writer,
            pending: Mutex::new(HashMap::new()),
            modules: Mutex::new(HashMap::new()),
            update_offer: Mutex::new(None),
            next_id: AtomicU64::new(1),
            registry,
            pipeline,
            pipeline_path,
            events,
            update_requests,
        });

        spawn_writer(&manifest.id, Arc::downgrade(&core), stdin, writer_rx);
        spawn_reader(core.clone(), stdout, child.clone());
        spawn_stderr(&manifest.id, stderr);

        let hello = core.call(
            "host.hello",
            json!({"apiVersion":1,"pluginId":manifest.id}),
            HANDSHAKE_TIMEOUT,
        );
        let hello: HelloResult = match hello
            .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
        {
            Ok(value) => value,
            Err(error) => {
                stop_child(&child);
                core.disconnect();
                anyhow::bail!("plugin handshake failed: {error}")
            }
        };
        if let Err(error) = validate_identifier(&hello.provider_id, "provider id") {
            stop_child(&child);
            core.disconnect();
            return Err(error);
        }
        {
            let disconnected = core.disconnected.lock().unwrap();
            anyhow::ensure!(!*disconnected, "plugin exited during handshake");
            *core.provider_id.write().unwrap() = Some(hello.provider_id);
            core.online.store(true, Ordering::Release);
        }
        let _ = core.notify("host.ready", json!({}));
        info!(target: "shanten_backend::plugin", plugin_id = %core.plugin_id, "plugin provider started");

        Ok(Some(Self { core, child }))
    }

    fn is_running(&self) -> bool {
        self.core.online.load(Ordering::Acquire)
    }

    fn last_error(&self) -> Option<String> {
        self.core.last_error.lock().unwrap().clone()
    }

    fn invoke(&self, method: &str, params: Value) -> RpcResult {
        self.core.call(
            "frontend.invoke",
            json!({"method":method,"params":params}),
            HANDSHAKE_TIMEOUT,
        )
    }

    fn check_update(&self) {
        let _ = self.core.notify("host.checkUpdate", json!({}));
    }

    fn update_offer(&self) -> Option<RemotePluginUpdate> {
        self.core.update_offer.lock().unwrap().clone()
    }
}

impl Drop for ProcessProvider {
    fn drop(&mut self) {
        let _ = self.core.notify("host.shutdown", json!({}));
        self.core.disconnect();
        stop_child(&self.child);
    }
}

type RpcResult = Result<Value, String>;

struct ProviderCore {
    disconnected: Mutex<bool>,
    plugin_id: String,
    current_version: String,
    config_path: PathBuf,
    permissions: HashSet<PacketOperation>,
    online: AtomicBool,
    last_error: Mutex<Option<String>>,
    provider_id: StdRwLock<Option<String>>,
    writer: SyncSender<Value>,
    pending: Mutex<HashMap<u64, mpsc::Sender<RpcResult>>>,
    modules: Mutex<HashMap<String, String>>,
    update_offer: Mutex<Option<RemotePluginUpdate>>,
    next_id: AtomicU64,
    registry: Arc<ModuleRegistry>,
    pipeline: Arc<RwLock<PipelineConfig>>,
    pipeline_path: PathBuf,
    events: broadcast::Sender<Value>,
    update_requests: SyncSender<String>,
}

impl ProviderCore {
    fn call(&self, method: &str, params: Value, timeout: Duration) -> RpcResult {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, sender);
        if let Err(error) = self.writer.try_send(json!({
            "jsonrpc":"2.0","id":id,"method":method,"params":params
        })) {
            self.pending.lock().unwrap().remove(&id);
            return Err(match error {
                TrySendError::Full(_) => "plugin RPC queue full".into(),
                TrySendError::Disconnected(_) => "plugin RPC disconnected".into(),
            });
        }
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(error) => {
                self.pending.lock().unwrap().remove(&id);
                Err(match error {
                    mpsc::RecvTimeoutError::Timeout => "plugin RPC timeout".into(),
                    mpsc::RecvTimeoutError::Disconnected => "plugin RPC disconnected".into(),
                })
            }
        }
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.writer
            .try_send(json!({"jsonrpc":"2.0","method":method,"params":params}))
            .map_err(|error| match error {
                TrySendError::Full(_) => "plugin RPC queue full".into(),
                TrySendError::Disconnected(_) => "plugin RPC disconnected".into(),
            })
    }

    fn receive(self: &Arc<Self>, message: Value) {
        let disconnected = self.disconnected.lock().unwrap();
        if *disconnected {
            return;
        }
        if message.get("method").is_none() {
            let Some(id) = message.get("id").and_then(Value::as_u64) else {
                return;
            };
            let Some(sender) = self.pending.lock().unwrap().remove(&id) else {
                return;
            };
            let result = if let Some(error) = message.get("error") {
                Err(error.to_string())
            } else {
                Ok(message.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ = sender.send(result);
            return;
        }

        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "module.register" => self.register_module(params),
            "module.replaceSubscriptions" => self.replace_subscriptions(params),
            "config.set" => self.set_config(params),
            "plugin.updateAvailable" => self.offer_update(params),
            _ => Err(format!("unknown plugin method: {method}")),
        };
        if let Some(id) = message.get("id").cloned() {
            let response = match result {
                Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                Err(error) => {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":error}})
                }
            };
            let _ = self.writer.try_send(response);
        }
    }

    fn register_module(self: &Arc<Self>, params: Value) -> RpcResult {
        let params: RegisterModuleParams =
            serde_json::from_value(params).map_err(|error| error.to_string())?;
        validate_identifier(&params.module_id, "module id").map_err(|error| error.to_string())?;
        validate_permissions(&params.subscriptions, &self.permissions)?;
        let provider_id = self
            .provider_id
            .read()
            .unwrap()
            .clone()
            .ok_or("provider handshake is not complete")?;
        let provider_key = format!("{}:{provider_id}", self.plugin_id);
        let global_id = format!(
            "plugin:{}:{provider_id}:{}",
            self.plugin_id, params.module_id
        );
        let module = PacketModuleInfo {
            id: global_id.clone(),
            provider_id: provider_key,
            name: params.name.unwrap_or_else(|| params.module_id.clone()),
            description: params.description,
            builtin: false,
            online: true,
            subscriptions_revision: params.subscriptions_revision,
            subscriptions: params.subscriptions,
            invocation_count: 0,
            total_duration_us: 0,
            timeout_count: 0,
            dropped_read_count: 0,
        };
        self.registry.register_external(
            module,
            Arc::new(ProcessModule {
                core: Arc::downgrade(self),
                module_id: params.module_id.clone(),
                circuit: Mutex::new((0, None)),
            }),
        )?;
        self.modules
            .lock()
            .unwrap()
            .insert(params.module_id, global_id.clone());
        if let Err(error) = self.ensure_pipeline_entry(&global_id) {
            let _ = self.registry.set_online(&global_id, false);
            return Err(error);
        }
        self.emit_modules();
        Ok(json!({"id":global_id}))
    }

    fn replace_subscriptions(&self, params: Value) -> RpcResult {
        let params: ReplaceSubscriptionsParams =
            serde_json::from_value(params).map_err(|error| error.to_string())?;
        validate_permissions(&params.subscriptions, &self.permissions)?;
        let global_id = self
            .modules
            .lock()
            .unwrap()
            .get(&params.module_id)
            .cloned()
            .ok_or("module is not registered")?;
        self.registry
            .replace_subscriptions(&global_id, params.revision, params.subscriptions)?;
        self.emit_modules();
        Ok(json!({"revision":params.revision}))
    }

    fn set_config(&self, value: Value) -> RpcResult {
        if !value.is_object() {
            return Err("plugin config must be a JSON object".into());
        }
        let parent = self
            .config_path
            .parent()
            .ok_or("invalid plugin config path")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        fs::write(
            &self.config_path,
            serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    }

    fn offer_update(&self, params: Value) -> RpcResult {
        let offer: ProviderUpdateOffer =
            serde_json::from_value(params).map_err(|error| error.to_string())?;
        validate_update_offer(&offer.version, &offer.download_url, &offer.sha256)
            .map_err(|error| error.to_string())?;
        let available =
            is_newer(&self.current_version, &offer.version).map_err(|error| error.to_string())?;
        if !available {
            *self.update_offer.lock().unwrap() = None;
            return Ok(json!({"accepted":false,"reason":"not newer"}));
        }
        let update = RemotePluginUpdate {
            id: self.plugin_id.clone(),
            version: offer.version.clone(),
            download_url: offer.download_url,
            sha256: offer.sha256,
            release_notes: offer.release_notes.clone(),
        };
        *self.update_offer.lock().unwrap() = Some(update);
        let info = PluginUpdateInfo {
            plugin_id: self.plugin_id.clone(),
            current_version: self.current_version.clone(),
            latest_version: Some(offer.version),
            available: true,
            release_notes: offer.release_notes,
            error: None,
        };
        let _ = self.events.send(event(
            "plugin_update_available",
            serde_json::to_value(info).unwrap_or(Value::Null),
        ));
        let _ = self.update_requests.try_send(self.plugin_id.clone());
        Ok(json!({"accepted":true}))
    }

    fn ensure_pipeline_entry(&self, id: &str) -> Result<(), String> {
        let mut config = self.pipeline.blocking_write();
        if config.modules.iter().any(|module| module.id == id) {
            return Ok(());
        }
        let mut updated = config.clone();
        updated.modules.push(ModuleConfig {
            id: id.into(),
            enabled: true,
            options: Value::Null,
        });
        pipeline::save(&self.pipeline_path, &updated)?;
        *config = updated;
        let _ = self.events.send(event(
            "packet_pipeline",
            serde_json::to_value(&*config).unwrap_or(Value::Null),
        ));
        Ok(())
    }

    fn emit_modules(&self) {
        let _ = self.events.send(event(
            "packet_modules",
            serde_json::to_value(self.registry.snapshot()).unwrap_or(Value::Null),
        ));
    }

    fn disconnect(&self) {
        let mut disconnected = self.disconnected.lock().unwrap();
        if *disconnected {
            return;
        }
        *disconnected = true;
        if self.online.swap(false, Ordering::AcqRel) {
            *self.last_error.lock().unwrap() = Some("plugin provider disconnected".into());
        }
        let provider_id = self.provider_id.read().unwrap().clone();
        if let Some(provider_id) = provider_id {
            self.registry
                .set_provider_offline(&format!("{}:{provider_id}", self.plugin_id));
            self.emit_modules();
        }
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for (_, sender) in pending {
            let _ = sender.send(Err("plugin provider disconnected".into()));
        }
    }
}

struct ProcessModule {
    core: Weak<ProviderCore>,
    module_id: String,
    circuit: Mutex<(u8, Option<Instant>)>,
}

impl ExternalPacketModule for ProcessModule {
    fn notify(&self, packet: &Packet, revision: u64) -> Result<(), String> {
        self.core
            .upgrade()
            .ok_or("plugin provider disconnected")?
            .notify(
                "packet.read",
                json!({"moduleId":self.module_id,"revision":revision,"packet":packet}),
            )
    }

    fn decide(
        &self,
        packet: &Packet,
        revision: u64,
        timeout: Duration,
    ) -> Result<ExternalDecision, String> {
        let core = self.core.upgrade().ok_or("plugin provider disconnected")?;
        {
            let mut circuit = self.circuit.lock().unwrap();
            if circuit.1.is_some_and(|until| until > Instant::now()) {
                return Err("plugin module circuit open".into());
            }
            circuit.1 = None;
        }
        let decision = core
            .call(
                "packet.handle",
                json!({
                    "moduleId":self.module_id,
                    "revision":revision,
                    "deadlineMs":timeout.as_millis(),
                    "packet":packet
                }),
                timeout,
            )
            .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()));
        let mut circuit = self.circuit.lock().unwrap();
        match decision {
            Ok(decision) => {
                *circuit = (0, None);
                Ok(decision)
            }
            Err(error) => {
                circuit.0 = circuit.0.saturating_add(1);
                if circuit.0 >= CIRCUIT_FAILURE_LIMIT {
                    *circuit = (0, Some(Instant::now() + CIRCUIT_RECOVERY_DELAY));
                }
                warn!(target: "shanten_backend::plugin", plugin_id = %core.plugin_id, module_id = %self.module_id, %error, "plugin packet decision failed");
                Err(error)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelloResult {
    provider_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterModuleParams {
    module_id: String,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    subscriptions_revision: u64,
    subscriptions: Vec<PacketSubscription>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaceSubscriptionsParams {
    module_id: String,
    revision: u64,
    subscriptions: Vec<PacketSubscription>,
}

fn validate_permissions(
    subscriptions: &[PacketSubscription],
    permissions: &HashSet<PacketOperation>,
) -> Result<(), String> {
    for operation in subscriptions
        .iter()
        .flat_map(|subscription| &subscription.operations)
    {
        if !permissions.contains(operation) {
            return Err(format!("packet operation {operation:?} was not declared"));
        }
    }
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        "invalid {label}"
    );
    Ok(())
}

fn spawn_writer(
    plugin_id: &str,
    core: Weak<ProviderCore>,
    stdin: std::process::ChildStdin,
    receiver: Receiver<Value>,
) {
    let plugin_id = plugin_id.to_owned();
    thread::spawn(move || {
        let mut writer = BufWriter::new(stdin);
        for message in receiver {
            if serde_json::to_writer(&mut writer, &message).is_err()
                || writer.write_all(b"\n").is_err()
                || writer.flush().is_err()
            {
                warn!(target: "shanten_backend::plugin", %plugin_id, "plugin stdin write failed");
                if let Some(core) = core.upgrade() {
                    core.disconnect();
                }
                break;
            }
        }
    });
}

fn spawn_reader(
    core: Arc<ProviderCore>,
    stdout: std::process::ChildStdout,
    child: Arc<Mutex<Option<Child>>>,
) {
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => match serde_json::from_str(&line) {
                    Ok(message) => core.receive(message),
                    Err(error) => {
                        warn!(target: "shanten_backend::plugin", plugin_id = %core.plugin_id, %error, "invalid plugin JSON-RPC message")
                    }
                },
                Err(error) => {
                    warn!(target: "shanten_backend::plugin", plugin_id = %core.plugin_id, %error, "plugin stdout read failed");
                    break;
                }
            }
        }
        core.disconnect();
        stop_child(&child);
    });
}

fn stop_child(child: &Mutex<Option<Child>>) {
    if let Ok(mut child) = child.lock() {
        if let Some(mut child) = child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_stderr(plugin_id: &str, stderr: std::process::ChildStderr) {
    let plugin_id = plugin_id.to_owned();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            info!(target: "shanten_backend::plugin", %plugin_id, message = %line);
        }
    });
}

fn event(kind: &str, data: Value) -> Value {
    json!({"type":kind,"data":data})
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        manager: Arc<PluginManager>,
        directory: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "shanten-plugin-test-{}-{nonce}",
                std::process::id()
            ));
            let root = directory.join("plugins");
            fs::create_dir_all(&root).unwrap();
            let (events, _) = broadcast::channel(64);
            let (update_requests, _) = mpsc::sync_channel(4);
            let manager = Arc::new(PluginManager {
                root,
                config_path: directory.join("plugins.json"),
                marketplace_config_path: directory.join("plugin-marketplaces.json"),
                registry: Arc::new(ModuleRegistry::default()),
                pipeline: Arc::new(RwLock::new(PipelineConfig {
                    schema: 1,
                    modules: vec![],
                })),
                pipeline_path: directory.join("pipeline.json"),
                events,
                update_requests,
                update_lock: Mutex::new(()),
                plugins: Mutex::new(HashMap::new()),
                scan_errors: Mutex::new(vec![]),
                frontend_health: Mutex::new(None),
                health_changed: Condvar::new(),
                frontend_generation: AtomicU64::new(0),
            });
            Self { manager, directory }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.manager.plugins.lock().unwrap().clear();
            let target = self.directory.canonicalize().unwrap();
            assert!(target.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            fs::remove_dir_all(target).unwrap();
        }
    }

    fn archive(version: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("plugin.json", options).unwrap();
        write!(zip, "{}", json!({"id":"test.plugin","name":"Test","version":version,"apiVersion":1,"frontend":{"entry":"index.js"}})).unwrap();
        zip.start_file("index.js", options).unwrap();
        zip.write_all(b"export function activate() {}").unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn marketplace_installs_verified_archive_disabled() {
        let fixture = Fixture::new();
        let archive = archive("1.0.0");
        let sha256 = format!("{:x}", Sha256::digest(&archive));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let plugin = json!({
            "id":"test.plugin",
            "name":"Test",
            "version":"1.0.0",
            "apiVersion":1,
            "repository":"https://example.com/test.plugin",
            "package":{"downloadUrl":format!("{base}/plugin.zip"),"sha256":sha256,"size":archive.len()},
            "publishedAt":"2026-09-12T00:00:00Z",
            "status":"active"
        });
        let registry = serde_json::to_vec(&json!({
            "schemaVersion":1,
            "marketplace":{"id":"test-market","name":"Test Market"},
            "revision":"1",
            "generatedAt":"2026-09-12T00:00:00Z",
            "plugins":[plugin.clone()]
        }))
        .unwrap();
        let served_archive = archive.clone();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                let body = if request.starts_with("GET /registry.json ") {
                    &registry
                } else {
                    &served_archive
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        let source_url = format!("{base}/registry.json");
        fs::write(
            &fixture.manager.marketplace_config_path,
            serde_json::to_vec(&json!({"schema":1,"sources":[source_url]})).unwrap(),
        )
        .unwrap();

        let installed = fixture
            .manager
            .install_marketplace_plugin(&source_url, "test.plugin")
            .unwrap();
        server.join().unwrap();
        assert_eq!(installed[0].version, "1.0.0");
        assert!(!installed[0].enabled);
        assert!(fixture
            .manager
            .remove_marketplace_source(DEFAULT_MARKETPLACE_URL)
            .is_err());

        let mut candidate: MarketplaceRegistryPlugin = serde_json::from_value(plugin).unwrap();
        candidate.status = MarketplacePluginStatus::Blocked;
        assert_eq!(
            marketplace_install_block(&candidate).unwrap(),
            Some(MarketplaceInstallBlock::Blocked)
        );
        candidate.status = MarketplacePluginStatus::Active;
        candidate.api_version = 2;
        assert_eq!(
            marketplace_install_block(&candidate).unwrap(),
            Some(MarketplaceInstallBlock::UnsupportedApi)
        );
        candidate.api_version = 1;
        candidate.minimum_app_version = Some("999.0.0".into());
        assert_eq!(
            marketplace_install_block(&candidate).unwrap(),
            Some(MarketplaceInstallBlock::AppTooOld)
        );
    }

    #[test]
    fn install_uninstall_and_scan_errors() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        assert!(!manager.install(&archive("1.0.0")).unwrap()[0].enabled);
        assert!(manager.install(&archive("1.0.0")).is_err());
        manager
            .set_config("test.plugin", json!({"keep":true}))
            .unwrap();
        manager.uninstall("test.plugin", false).unwrap();
        manager.install(&archive("1.0.0")).unwrap();
        assert_eq!(
            manager.get_config("test.plugin").unwrap(),
            json!({"keep":true})
        );
        let duplicate = manager.root.join("duplicate");
        fs::create_dir(&duplicate).unwrap();
        extract_update(&archive("1.0.0"), &duplicate).unwrap();
        let broken = manager.root.join("broken");
        fs::create_dir(&broken).unwrap();
        fs::write(broken.join("plugin.json"), "invalid").unwrap();
        manager.rescan().unwrap();
        assert_eq!(manager.scan_errors().len(), 2);
        // The lexically first duplicate is the discovered plugin.
        manager.uninstall("test.plugin", true).unwrap();
        assert!(!manager.plugin_config_path("test.plugin").exists());
    }

    #[test]
    fn rescan_cleans_modules_left_by_previously_uninstalled_plugins() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let id = "plugin:removed.plugin:main:stale";
        let mut module = PacketModuleInfo::builtin(id, vec![]);
        module.builtin = false;
        manager.registry.register(module).unwrap();
        manager
            .pipeline
            .blocking_write()
            .modules
            .push(ModuleConfig {
                id: id.into(),
                enabled: true,
                options: Value::Null,
            });
        manager.rescan().unwrap();
        assert!(manager.registry.get(id).is_none());
        assert!(manager.pipeline.blocking_read().modules.is_empty());
    }

    #[test]
    fn uninstall_removes_only_owned_modules_from_registry_and_saved_pipeline() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        manager.install(&archive("1.0.0")).unwrap();
        let other = manager.root.join("other");
        fs::create_dir(&other).unwrap();
        extract_update(&archive("1.0.0"), &other).unwrap();
        let path = other.join("plugin.json");
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("test.plugin", "test.plugin.other");
        fs::write(path, text).unwrap();
        for id in [
            "plugin:test.plugin:main:first",
            "plugin:test.plugin:other:second",
            "plugin:test.plugin.other:main:keep",
            "method_filter",
        ] {
            let mut module = PacketModuleInfo::builtin(id, vec![]);
            module.builtin = id == "method_filter";
            manager.registry.register(module).unwrap();
            manager
                .pipeline
                .blocking_write()
                .modules
                .push(ModuleConfig {
                    id: id.into(),
                    enabled: true,
                    options: json!({"keep":true}),
                });
        }
        manager.uninstall("test.plugin", false).unwrap();
        let config = manager.pipeline.blocking_read().clone();
        let ids: Vec<_> = config.modules.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["plugin:test.plugin.other:main:keep", "method_filter"]
        );
        assert!(manager
            .registry
            .get("plugin:test.plugin:main:first")
            .is_none());
        assert!(manager
            .registry
            .get("plugin:test.plugin:other:second")
            .is_none());
        assert!(manager
            .registry
            .get("plugin:test.plugin.other:main:keep")
            .is_some());
        let saved: PipelineConfig =
            serde_json::from_slice(&fs::read(&manager.pipeline_path).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(saved).unwrap(),
            serde_json::to_value(config).unwrap()
        );
    }

    #[test]
    fn frontend_health_failure_restores_version_and_config() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        manager.install(&archive("1.0.0")).unwrap();
        manager.set_enabled("test.plugin", true, vec![]).unwrap();
        manager
            .set_config("test.plugin", json!({"version":1}))
            .unwrap();
        let reporter = manager.clone();
        let task = thread::spawn(move || {
            for _ in 0..300 {
                let token = reporter
                    .frontend_bundles()
                    .into_iter()
                    .find_map(|bundle| bundle.health_token);
                if let Some(token) = token {
                    reporter
                        .set_config("test.plugin", json!({"version":2}))
                        .unwrap();
                    reporter.report_frontend_health("test.plugin", "stale-token", None);
                    assert!(reporter
                        .frontend_health
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .result
                        .is_none());
                    reporter.report_frontend_health(
                        "test.plugin",
                        &token,
                        Some("activation failed".into()),
                    );
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            panic!("update health request was not published");
        });
        let error = manager
            .apply_update_archive(
                "test.plugin",
                &manager.root.join("test.plugin"),
                "2.0.0",
                &archive("2.0.0"),
            )
            .unwrap_err();
        task.join().unwrap();
        assert!(error.to_string().contains("rolled back"));
        assert_eq!(manager.list()[0].version, "1.0.0");
        assert_eq!(
            manager.get_config("test.plugin").unwrap(),
            json!({"version":1})
        );
        assert!(manager.list()[0]
            .last_error
            .as_ref()
            .unwrap()
            .contains("activation failed"));
    }

    #[test]
    fn backend_start_failure_rolls_back() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        manager.install(&archive("1.0.0")).unwrap();
        manager.set_enabled("test.plugin", true, vec![]).unwrap();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("plugin.json", options).unwrap();
        write!(
            zip,
            "{}",
            json!({"id":"test.plugin","name":"Test","version":"2.0.0","apiVersion":1,
            "backend":{"type":"process","entry":"broken.exe","args":["--exact","plugins::tests::provider_process","--nocapture"]}})
        )
        .unwrap();
        zip.start_file("broken.exe", options).unwrap();
        // This fixture exits without a handshake when its ID is test.plugin.
        zip.write_all(&fs::read(std::env::current_exe().unwrap()).unwrap())
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(manager
            .apply_update_archive(
                "test.plugin",
                &manager.root.join("test.plugin"),
                "2.0.0",
                &bytes
            )
            .is_err());
        assert_eq!(manager.list()[0].version, "1.0.0");
        assert!(manager.list()[0].running);
    }

    #[test]
    fn healthy_frontend_update_commits() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        manager.install(&archive("1.0.0")).unwrap();
        manager.set_enabled("test.plugin", true, vec![]).unwrap();
        let reporter = manager.clone();
        let task = thread::spawn(move || {
            for _ in 0..300 {
                if let Some(token) = reporter
                    .frontend_bundles()
                    .into_iter()
                    .find_map(|bundle| bundle.health_token)
                {
                    reporter.report_frontend_health("test.plugin", &token, None);
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            panic!("update health request was not published");
        });
        manager
            .apply_update_archive(
                "test.plugin",
                &manager.root.join("test.plugin"),
                "2.0.0",
                &archive("2.0.0"),
            )
            .unwrap();
        task.join().unwrap();
        assert_eq!(manager.list()[0].version, "2.0.0");
        assert!(manager.list()[0].running);
        assert!(!fs::read_dir(&fixture.directory).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".plugin-")));
    }

    #[test]
    fn disabled_update_does_not_execute_frontend() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        manager.install(&archive("1.0.0")).unwrap();
        manager
            .apply_update_archive(
                "test.plugin",
                &manager.root.join("test.plugin"),
                "2.0.0",
                &archive("2.0.0"),
            )
            .unwrap();
        assert_eq!(manager.list()[0].version, "2.0.0");
        assert!(!manager.list()[0].enabled);
        assert!(manager.frontend_health.lock().unwrap().is_none());
    }

    #[test]
    fn provider_process() {
        if std::env::var("SHANTEN_LENS_PLUGIN_ID").as_deref() != Ok("test.provider") {
            return;
        }
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            let message: Value = serde_json::from_str(&line).unwrap();
            let response = match message["method"].as_str() {
                Some("host.hello") => {
                    json!({"jsonrpc":"2.0","id":message["id"],"result":{"providerId":"main"}})
                }
                Some("host.ready") => {
                    json!({"jsonrpc":"2.0","method":"module.register","params":{"moduleId":"observer","subscriptionsRevision":1,"subscriptions":[]}})
                }
                Some("host.shutdown") => return,
                _ => continue,
            };
            let mut out = std::io::stdout().lock();
            writeln!(out, "{response}").unwrap();
            out.flush().unwrap();
        }
    }

    #[test]
    fn rescan_keeps_replacement_online_after_old_disconnect() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let directory = manager.root.join("test.provider");
        fs::create_dir(&directory).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            directory.join("provider.exe"),
        )
        .unwrap();
        fs::write(directory.join("plugin.json"), json!({"id":"test.provider","name":"Test","version":"1.0.0","apiVersion":1,
            "backend":{"type":"process","entry":"provider.exe","args":["--exact","plugins::tests::provider_process","--nocapture"]}}).to_string()).unwrap();
        manager.rescan().unwrap();
        manager.set_enabled("test.provider", true, vec![]).unwrap();
        let old = manager
            .plugins
            .lock()
            .unwrap()
            .get("test.provider")
            .unwrap()
            .provider
            .as_ref()
            .unwrap()
            .core
            .clone();
        for _ in 0..3 {
            manager.rescan().unwrap();
            for _ in 0..100 {
                if manager
                    .registry
                    .snapshot()
                    .iter()
                    .any(|module| module.online)
                {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            old.disconnect();
            assert!(manager.list()[0].running);
            assert!(manager
                .registry
                .snapshot()
                .iter()
                .any(|module| module.online));
            assert!(manager
                .registry
                .external("plugin:test.provider:main:observer")
                .is_some());
        }
    }
}
