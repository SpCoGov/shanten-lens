use super::manifest::{load_manifest, validate_plugin_entries};
use super::process::{
    start_managed_plugin, ProcessProvider, HANDSHAKE_TIMEOUT, RPC_QUEUE_CAPACITY,
};
use super::updates::{extract_update, MAX_UPDATE_ARCHIVE_SIZE};
use super::{
    event, ManagedPlugin, PluginChoice, PluginConfig, PluginInfo, PluginManager, PluginScanError,
};
use crate::pipeline::{self, ModuleRegistry, PacketOperation, PipelineConfig};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread,
};
use tokio::sync::{broadcast, RwLock};
use tracing::warn;

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

    pub fn rescan(&self) -> anyhow::Result<Vec<PluginInfo>> {
        let _guard = self.update_lock.lock().unwrap();
        self.rescan_locked()
    }

    pub fn scan_errors(&self) -> Vec<PluginScanError> {
        self.scan_errors.lock().unwrap().clone()
    }

    pub(super) fn rescan_locked(&self) -> anyhow::Result<Vec<PluginInfo>> {
        let config = self.load_config()?;
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

    pub fn install(&self, archive: &[u8]) -> anyhow::Result<Vec<PluginInfo>> {
        self.install_expected(archive, None)
    }

    pub(super) fn install_expected(
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
            let mut config = self.load_config()?;
            config
                .plugins
                .insert(manifest.id.clone(), PluginChoice::default());
            crate::storage::write_json(&self.config_path, &config)?;
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
        let mut config = self.load_config()?;
        config.plugins.remove(id);
        self.plugins.lock().unwrap().remove(id);
        self.emit_status();
        if let Err(error) = fs::remove_dir_all(&directory) {
            let _ = self.rescan_locked();
            return Err(error.into());
        }
        self.remove_plugin_modules(id)?;
        crate::storage::write_json(&self.config_path, &config)?;
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
        crate::storage::write_json(&path, &value)?;
        Ok(())
    }

    pub fn invoke(&self, id: &str, method: &str, params: Value) -> anyhow::Result<Value> {
        anyhow::ensure!(!method.trim().is_empty(), "plugin method cannot be empty");
        let plugins = self.plugins.lock().unwrap();
        let plugin = plugins
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?;
        anyhow::ensure!(plugin.choice.enabled, "plugin is disabled");
        let core = plugin
            .provider
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("plugin has no running backend provider"))?
            .core
            .clone();
        drop(plugins);
        core.call(
            "frontend.invoke",
            json!({"method":method,"params":params}),
            HANDSHAKE_TIMEOUT,
        )
        .map_err(anyhow::Error::msg)
    }

    pub(super) fn plugin_config_path(&self, id: &str) -> PathBuf {
        self.config_path
            .parent()
            .unwrap_or(&self.root)
            .join("plugin-config")
            .join(format!("{id}.json"))
    }

    pub(super) fn load_config(&self) -> anyhow::Result<PluginConfig> {
        match fs::read(&self.config_path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(PluginConfig {
                schema: 1,
                plugins: HashMap::new(),
            }),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn save_config(
        &self,
        plugins: &HashMap<String, ManagedPlugin>,
    ) -> anyhow::Result<()> {
        let mut config = self.load_config()?;
        config.schema = 1;
        config.plugins.extend(
            plugins
                .iter()
                .map(|(id, plugin)| (id.clone(), plugin.choice.clone())),
        );
        crate::storage::write_json(&self.config_path, &config)?;
        Ok(())
    }

    pub(super) fn emit_status(&self) {
        let _ = self.events.send(event(
            "plugin_status",
            serde_json::to_value(self.list()).unwrap_or(Value::Null),
        ));
    }
}
