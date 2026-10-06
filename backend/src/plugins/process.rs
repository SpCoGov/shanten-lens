use super::manifest::validate_identifier;
use super::updates::{is_newer, validate_update_offer};
use super::{
    event, ManagedPlugin, PluginManifest, PluginUpdateInfo, ProviderUpdateOffer, RemotePluginUpdate,
};
use crate::pipeline::{
    self, ExternalDecision, ExternalPacketModule, ModuleConfig, ModuleRegistry, Packet,
    PacketModuleInfo, PacketOperation, PacketSubscription, PipelineConfig,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex, RwLock as StdRwLock, Weak,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

pub(super) const RPC_QUEUE_CAPACITY: usize = 256;
pub(super) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
const CIRCUIT_FAILURE_LIMIT: u8 = 3;
const CIRCUIT_RECOVERY_DELAY: Duration = Duration::from_secs(5);

pub(super) fn start_managed_plugin(
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

pub(super) struct ProcessProvider {
    pub(super) core: Arc<ProviderCore>,
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

    pub(super) fn is_running(&self) -> bool {
        self.core.online.load(Ordering::Acquire)
    }

    pub(super) fn last_error(&self) -> Option<String> {
        self.core.last_error.lock().unwrap().clone()
    }

    pub(super) fn check_update(&self) {
        let _ = self.core.notify("host.checkUpdate", json!({}));
    }

    pub(super) fn update_offer(&self) -> Option<RemotePluginUpdate> {
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

pub(super) struct ProviderCore {
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
    pub(super) fn call(&self, method: &str, params: Value, timeout: Duration) -> RpcResult {
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
        crate::storage::write_json(&self.config_path, &value).map_err(|error| error.to_string())?;
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

    pub(super) fn disconnect(&self) {
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
