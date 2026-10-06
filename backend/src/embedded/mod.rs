//! Backend runtime used by the Tauri application.
#[cfg(test)]
mod audit_tests;
mod commands;
mod plugins;
mod qyzz;
mod switch;

use crate::{
    automation::Automation,
    ipc::BackendSnapshot,
    logging::{self, LogBuffer},
    pipeline::{self, ModuleRegistry, PipelineConfig},
    plugins::PluginManager,
    proxy::ProxyControl,
    runtime::{normalize_config, register_builtin_modules},
    services::Services,
    VERSION,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicU64, Arc},
    time::Duration,
};
use tokio::sync::{broadcast, RwLock};
use tracing::error;

#[derive(Clone)]
struct State {
    pipeline_path: PathBuf,
    pipeline: Arc<RwLock<PipelineConfig>>,
    module_registry: Arc<ModuleRegistry>,
    plugins: Arc<PluginManager>,
    events: broadcast::Sender<Value>,
    logs: Arc<LogBuffer>,
    services: Arc<Services>,
    proxy: ProxyControl,
    qyzz: Arc<qyzz::QyzzLink>,
    automation: Arc<Automation>,
    switch_plan: Arc<RwLock<Option<Value>>>,
    switch_generation: tokio::sync::watch::Sender<u64>,
    debug_generation: Arc<AtomicU64>,
    switch_execution: Arc<tokio::sync::Mutex<()>>,
}

/// Backend runtime embedded by the Tauri application and exposed through
/// concrete Tauri commands and named events.
#[derive(Clone)]
pub struct BackendRuntime {
    state: State,
    runtime_root: PathBuf,
    mitm_port: u16,
}

impl BackendRuntime {
    pub fn load(runtime_root: PathBuf) -> anyhow::Result<Self> {
        let pipeline_path = runtime_root.join("pipeline.json");
        let config = normalize_config(pipeline::load(&pipeline_path).map_err(anyhow::Error::msg)?);
        pipeline::save(&pipeline_path, &config).map_err(anyhow::Error::msg)?;
        let module_registry = Arc::new(ModuleRegistry::default());
        register_builtin_modules(&module_registry, &config);
        let (events, _) = broadcast::channel(1_024);
        let logs = logging::init(events.clone(), &log_dir(&runtime_root))?;
        let services = Arc::new(Services::load(runtime_root.clone(), events.clone())?);
        let pipeline = Arc::new(RwLock::new(config));
        let plugins = PluginManager::load(
            runtime_root
                .parent()
                .unwrap_or(&runtime_root)
                .join("plugins"),
            runtime_root.join("plugins.json"),
            module_registry.clone(),
            pipeline.clone(),
            pipeline_path.clone(),
            events.clone(),
        )?;
        let backend_config = services.table("backend");
        let mitm_port = backend_config
            .get("mitm_port")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(10999);
        let proxy = ProxyControl::new(services.clone(), pipeline.clone());
        let automation = Arc::new(Automation::new(services.clone(), proxy.clone()));
        Ok(Self {
            state: State {
                pipeline_path,
                pipeline,
                module_registry,
                plugins,
                events,
                logs,
                services,
                proxy,
                qyzz: Arc::new(qyzz::QyzzLink::default()),
                automation,
                switch_plan: Arc::new(RwLock::new(None)),
                switch_generation: tokio::sync::watch::channel(0).0,
                debug_generation: Arc::new(AtomicU64::new(0)),
                switch_execution: Arc::new(tokio::sync::Mutex::new(())),
            },
            runtime_root,
            mitm_port,
        })
    }

    pub async fn snapshot(&self) -> Result<BackendSnapshot, String> {
        Ok(BackendSnapshot {
            version: VERSION.into(),
            proxy_status: self.state.services.proxy_status(),
            backend_logs: self.state.logs.snapshot(),
            packet_pipeline: self.state.pipeline.read().await.clone(),
            fuse_config: snapshot_part(self.state.services.table("fuse"), "fuse config")?,
            autorun_config: snapshot_part(self.state.services.table("autorun"), "autorun config")?,
            registry: snapshot_part(self.state.services.registry(), "registry")?,
            config: snapshot_part(self.state.services.config_payload(), "config")?,
            game_state: snapshot_part(self.state.services.game_state(), "game state")?,
            autorun_status: snapshot_part(
                self.state.automation.autorun_status(),
                "autorun status",
            )?,
            tsumo_loop_status: self.state.automation.tsumo_status(),
            packet_log: self.state.services.packet_log_snapshot(),
        })
    }

    pub fn open_config_dir(&self) -> Result<(), String> {
        open_directory(self.state.services.config_root())
    }

    pub fn open_log_dir(&self) -> Result<(), String> {
        open_directory(&log_dir(&self.runtime_root))
    }

    pub fn open_record_dir(&self) -> Result<(), String> {
        open_directory(&self.state.services.record_dir())
    }

    pub fn open_plugin_dir(&self) -> Result<(), String> {
        open_directory(self.state.plugins.root())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.state.events.subscribe()
    }

    pub async fn run_background(self) {
        let proxy = shanten_backend::proxy::run(
            ([127, 0, 0, 1], self.mitm_port).into(),
            &self.runtime_root,
            self.state.pipeline.clone(),
            self.state.module_registry.clone(),
            self.state.services.clone(),
            self.state.proxy.clone(),
        );
        let mut source_events = self.state.services.events.subscribe();
        let source_changes = async {
            loop {
                match source_events.recv().await {
                    Ok(event) if event["type"] == "data_source_status" => {
                        self.state.automation.stop_autorun();
                        self.state.automation.stop_tsumo();
                        self.state.switch_generation.send_modify(|generation| *generation += 1);
                        *self.state.switch_plan.write().await = None;
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        let reload = async {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                self.state.services.reload_files();
            }
        };
        let proxy_task = async {
            let proxy_result = proxy.await;
            let reason = proxy_result.err().map(|error| format!("{error:#}")).unwrap_or_else(|| "MITM proxy stopped".into());
            error!(target: "shanten_backend::proxy", error = %reason, "MITM proxy stopped");
            self.state.services.set_proxy_status(false, Some(reason));
        };
        tokio::join!(proxy_task, reload, qyzz::run(&self.state), source_changes);
    }
}

fn envelope(kind: &str, data: Value) -> Value {
    json!({"type": kind, "data": data})
}

fn log_dir(runtime_root: &std::path::Path) -> PathBuf {
    runtime_root.parent().unwrap_or(runtime_root).join("logs")
}

fn open_directory(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut value = std::process::Command::new("explorer.exe");
        value.arg(path);
        value
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut value = std::process::Command::new("open");
        value.arg(path);
        value
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut value = std::process::Command::new("xdg-open");
        value.arg(path);
        value
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn snapshot_part<T: serde::de::DeserializeOwned>(value: Value, name: &str) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("invalid {name}: {error}"))
}
