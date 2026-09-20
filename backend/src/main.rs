use serde_json::{json, Value};
use shanten_backend::{
    automation::Automation,
    ipc::{
        AmuletAction, AmuletActionRequest, AutorunAction, BackendSnapshot, CommandResult,
        ConfigTables, FlowDumpResult, JsonMap, PacketLogSnapshot, SwitchRequest, TsumoLoopStatus,
        VersionMismatch,
    },
    logging::{self, LogBuffer},
    pipeline::{self, ModuleRegistry, PacketModuleInfo, PipelineConfig, GAME_RECORD},
    plugins::{
        MarketplaceSnapshot, PluginFrontendBundle, PluginInfo, PluginManager, PluginScanError,
        PluginUpdateInfo,
    },
    proxy::ProxyControl,
    recommendations,
    runtime::{normalize_config, register_builtin_modules},
    services::{event, Services},
    VERSION,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
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

    pub fn check_frontend_version(&self, version: String) -> Option<VersionMismatch> {
        (!version.is_empty() && version != VERSION).then(|| VersionMismatch {
            frontend_version: version,
            backend_version: VERSION.into(),
        })
    }

    pub async fn packet_pipeline(&self) -> PipelineConfig {
        self.state.pipeline.read().await.clone()
    }

    pub fn packet_modules(&self) -> Vec<PacketModuleInfo> {
        self.state.module_registry.snapshot()
    }

    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.state.plugins.list()
    }

    pub async fn plugin_marketplace(&self) -> Result<MarketplaceSnapshot, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins.marketplace().map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn add_plugin_marketplace_source(
        &self,
        url: String,
    ) -> Result<MarketplaceSnapshot, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .add_marketplace_source(&url)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn remove_plugin_marketplace_source(
        &self,
        url: String,
    ) -> Result<MarketplaceSnapshot, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .remove_marketplace_source(&url)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn install_marketplace_plugin(
        &self,
        source_url: String,
        plugin_id: String,
    ) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .install_marketplace_plugin(&source_url, &plugin_id)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub fn plugin_frontends(&self) -> Vec<PluginFrontendBundle> {
        self.state.plugins.frontend_bundles()
    }

    pub fn plugin_config(&self, id: String) -> Result<Value, String> {
        self.state
            .plugins
            .get_config(&id)
            .map_err(|error| error.to_string())
    }

    pub fn set_plugin_config(&self, id: String, value: Value) -> Result<(), String> {
        self.state
            .plugins
            .set_config(&id, value)
            .map_err(|error| error.to_string())
    }

    pub async fn invoke_plugin(
        &self,
        id: String,
        method: String,
        params: Value,
    ) -> Result<Value, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .invoke(&id, &method, params)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub fn plugin_scan_errors(&self) -> Vec<PluginScanError> {
        self.state.plugins.scan_errors()
    }

    pub fn report_plugin_frontend_health(&self, id: String, token: String, error: Option<String>) {
        self.state
            .plugins
            .report_frontend_health(&id, &token, error);
    }

    pub async fn install_plugin(&self, archive: String) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            use base64::Engine;
            if archive.len() > 180 * 1024 * 1024 {
                return Err("plugin archive is too large".into());
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(archive)
                .map_err(|error| error.to_string())?;
            plugins.install(&bytes).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn uninstall_plugin(
        &self,
        id: String,
        remove_config: bool,
    ) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .uninstall(&id, remove_config)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn rescan_plugins(&self) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || plugins.rescan().map_err(|error| error.to_string()))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn set_plugin_enabled(
        &self,
        id: String,
        enabled: bool,
        approved_permissions: Vec<shanten_backend::pipeline::PacketOperation>,
    ) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .set_enabled(&id, enabled, approved_permissions)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn restart_plugin(&self, id: String) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || plugins.restart(&id).map_err(|error| error.to_string()))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn set_plugin_auto_update(
        &self,
        id: String,
        enabled: bool,
    ) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .set_auto_update(&id, enabled)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn check_plugin_updates(&self) -> Result<Vec<PluginUpdateInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || plugins.check_updates())
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn update_plugin(&self, id: String) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || plugins.update(&id).map_err(|error| error.to_string()))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn set_packet_pipeline(&self, config: PipelineConfig) -> CommandResult {
        let config = normalize_config(config);
        let mut current = self.state.pipeline.write().await;
        match pipeline::validate(&config)
            .and_then(|_| pipeline::save(&self.state.pipeline_path, &config))
        {
            Ok(()) => {
                *current = config.clone();
                drop(current);
                register_builtin_modules(&self.state.module_registry, &config);
                let _ = self.state.events.send(envelope(
                    "packet_pipeline",
                    serde_json::to_value(&config).unwrap_or(Value::Null),
                ));
                let _ = self.state.events.send(envelope(
                    "packet_modules",
                    serde_json::to_value(self.packet_modules()).unwrap_or(Value::Null),
                ));
                CommandResult {
                    ok: true,
                    ..Default::default()
                }
            }
            Err(error) => CommandResult {
                error: Some(error),
                ..Default::default()
            },
        }
    }

    pub fn packet_log(&self) -> PacketLogSnapshot {
        self.state.services.packet_log_snapshot()
    }

    pub async fn replay_packet(&self, method: String, payload: JsonMap) -> CommandResult {
        if method.is_empty() {
            return failed("invalid-payload");
        }
        if !self.state.services.can_replay(&method) {
            return failed("packet-not-observed");
        }
        let config = self.state.pipeline.read().await.clone();
        match self
            .state
            .proxy
            .replay(
                &method,
                &Value::Object(payload.into_iter().collect()),
                Duration::from_secs(12),
                config,
            )
            .await
        {
            Ok((id, response)) => CommandResult {
                ok: response.get("error").is_none(),
                reason: response
                    .get("error")
                    .is_some()
                    .then(|| "protocol-error".into()),
                msg_id: Some(id),
                response: Some(response),
                ..Default::default()
            },
            Err(error) => failed(error),
        }
    }

    pub async fn fetch_game_record(&self, game_uuid: String) -> CommandResult {
        let game_uuid = game_uuid.trim();
        if game_uuid.is_empty() || game_uuid.len() > 200 {
            return failed("invalid-payload");
        }
        let config = self.state.pipeline.read().await.clone();
        match self
            .state
            .proxy
            .inject(
                ".lq.Lobby.fetchGameRecord",
                &json!({
                    "clientVersionString": "StandaloneWindows_2022-0.16.273",
                    "gameUuid": game_uuid,
                }),
                Duration::from_secs(12),
                config,
                GAME_RECORD,
            )
            .await
        {
            Ok((id, response)) => CommandResult {
                ok: response.get("error").is_none(),
                reason: response
                    .get("error")
                    .is_some()
                    .then(|| "protocol-error".into()),
                msg_id: Some(id),
                response: Some(response),
                ..Default::default()
            },
            Err(error) => failed(error),
        }
    }

    pub fn override_game_record(&self, record: Value) -> CommandResult {
        match self.state.services.arm_game_record_override(&record) {
            Ok(()) => CommandResult {
                ok: true,
                ..Default::default()
            },
            Err(error) => {
                let reason = format!("{error:#}");
                error!(target: "shanten_backend::game_record", error = %reason, "failed to arm game record override");
                failed(reason)
            }
        }
    }

    pub fn update_config(&self, config: ConfigTables) -> Result<(), String> {
        let config = serde_json::to_value(config).map_err(|error| error.to_string())?;
        let result = self.state.services.patch_config(&config);
        for update in [
            event("update_fuse_config", self.state.services.table("fuse")),
            event(
                "update_autorun_config",
                self.state.services.table("autorun"),
            ),
            event("update_config", self.state.services.config_payload()),
        ] {
            let _ = self.state.events.send(update);
        }
        result
    }

    pub fn set_locale(&self, locale: String) {
        self.state.services.set_locale(&locale);
    }

    pub fn dump_flows(&self) -> FlowDumpResult {
        let flows = self.state.proxy.flows();
        FlowDumpResult {
            ok: true,
            count: flows.len(),
            flows,
        }
    }

    pub async fn fetch_activity(&self, activity_id: u64) -> CommandResult {
        request_reply(
            &self.state,
            ".lq.Lobby.fetchAmuletActivityData",
            json!({"activityId": activity_id}),
        )
        .await
    }

    pub async fn discard_tile(&self, tile_id: u64) -> CommandResult {
        if tile_id == 0 {
            return failed("invalid-tile-id");
        }
        request_reply(
            &self.state,
            ".lq.Lobby.amuletActivityGameOperate",
            json!({"activityId":260511,"type":1,"tileList":[tile_id]}),
        )
        .await
    }

    pub async fn upgrade_shop_buff(&self, activity_id: u64, id: u64) -> CommandResult {
        request_reply(
            &self.state,
            ".lq.Lobby.amuletActivityUpgradeShopBuff",
            json!({"activityId":activity_id,"id":id}),
        )
        .await
    }

    pub async fn amulet_action(&self, request: AmuletActionRequest) -> CommandResult {
        hotkey_reply(&self.state, request).await
    }

    pub fn start_tsumo_loop(&self, interval_ms: u64, reset_count: bool) -> TsumoLoopStatus {
        self.state.automation.start_tsumo(interval_ms, reset_count);
        self.state.automation.tsumo_status()
    }

    pub fn stop_tsumo_loop(&self) -> TsumoLoopStatus {
        self.state.automation.stop_tsumo();
        self.state.automation.tsumo_status()
    }

    pub async fn autorun(
        &self,
        action: AutorunAction,
        force: bool,
        mode: Option<String>,
    ) -> CommandResult {
        let result = match action {
            AutorunAction::Start => {
                let current = self.state.services.game_state();
                let has_live = current.get("stage").and_then(Value::as_i64).unwrap_or(-1) >= 0
                    && !current
                        .get("ended")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                if has_live && !force {
                    CommandResult {
                        requires_confirmation: Some(true),
                        reason: Some("existing-live-game".into()),
                        ..Default::default()
                    }
                } else {
                    self.state.automation.start_autorun(has_live && force);
                    CommandResult {
                        ok: true,
                        ..Default::default()
                    }
                }
            }
            AutorunAction::Stop => {
                self.state.automation.stop_autorun();
                CommandResult {
                    ok: true,
                    ..Default::default()
                }
            }
            AutorunAction::SetMode => {
                self.state
                    .automation
                    .set_mode(mode.as_deref().unwrap_or("continuous"));
                CommandResult {
                    ok: true,
                    ..Default::default()
                }
            }
            AutorunAction::Step => {
                self.state.automation.tick().await;
                CommandResult {
                    ok: true,
                    ..Default::default()
                }
            }
            AutorunAction::Probe => match self
                .state
                .proxy
                .request_with_retry(
                    ".lq.Lobby.fetchAmuletActivityData",
                    &json!({"activityId":260511}),
                    Duration::from_secs(12),
                )
                .await
            {
                Ok(_) => CommandResult {
                    ok: true,
                    ..Default::default()
                },
                Err(error) => failed(error),
            },
            AutorunAction::NotifyTestEmail => {
                let config = self
                    .state
                    .services
                    .table("autorun")
                    .get("email_notify")
                    .cloned()
                    .unwrap_or(Value::Null);
                match tokio::task::spawn_blocking(move || {
                    shanten_backend::mail::send(
                        &config,
                        "Shanten Lens 测试通知",
                        "Rust 3.0 后端邮件通知工作正常。",
                    )
                })
                .await
                {
                    Ok(Ok(())) => CommandResult {
                        ok: true,
                        ..Default::default()
                    },
                    Ok(Err(error)) => failed(error),
                    Err(error) => failed(error.to_string()),
                }
            }
        };
        let _ = self.state.events.send(event(
            "autorun_status",
            self.state.automation.autorun_status(),
        ));
        result
    }

    pub fn resolve_confirmation(&self, id: String, ok: bool) {
        self.state.services.resolve_confirmation(&id, ok);
    }

    pub async fn switch(&self, request: SwitchRequest) {
        let data = json!({
            "action": request.action.as_str(),
            "options": request.options,
            "snapshot": request.snapshot,
            "quad_groups": request.quad_groups,
            "structure_groups": request.structure_groups,
            "notify": request.notify,
        });
        let replies = switch_command(&self.state, &data).await;
        for reply in replies {
            let _ = self.state.events.send(reply);
        }
    }

    pub fn open_config_dir(&self) -> Result<(), String> {
        open_directory(self.state.services.config_root())
    }

    pub fn open_log_dir(&self) -> Result<(), String> {
        open_directory(&log_dir(&self.runtime_root))
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
        let reload = async {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                self.state.services.reload_files();
            }
        };
        tokio::select! {
            proxy_result = proxy => {
                let reason = proxy_result.err().map(|error| format!("{error:#}")).unwrap_or_else(|| "MITM proxy stopped".into());
                error!(target: "shanten_backend::proxy", error = %reason, "MITM proxy stopped");
                self.state.services.set_proxy_status(false, Some(reason));
            }
            _ = reload => {}
        }
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

async fn switch_command(state: &State, data: &Value) -> Vec<Value> {
    let action = data.get("action").and_then(Value::as_str).unwrap_or("");
    let wall_limit = data
        .get("options")
        .and_then(|v| v.get("wall_limit"))
        .and_then(Value::as_u64)
        .unwrap_or(36) as usize;
    match action {
        "start" | "start_debug" => {
            let debug = action == "start_debug";
            let generation = if debug {
                state.debug_generation.fetch_add(1, Ordering::SeqCst) + 1
            } else {
                let mut current = state.switch_plan.write().await;
                state
                    .switch_generation
                    .send_modify(|generation| *generation += 1);
                *current = None;
                *state.switch_generation.borrow()
            };
            let snapshot = if action == "start_debug" {
                data.get("snapshot")
                    .cloned()
                    .unwrap_or_else(|| state.services.game_state())
            } else {
                state.services.game_state()
            };
            let skip_signatures = data
                .get("options")
                .and_then(|value| value.get("skip_signatures"))
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let session = snapshot.get("session_id").cloned().unwrap_or(Value::Null);
            let revision = snapshot.get("revision").cloned().unwrap_or(Value::Null);
            let algorithm = data
                .get("options")
                .and_then(|v| v.get("search_algorithm"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let mut plan = tokio::task::spawn_blocking(move || {
                recommendations::switch_plan(&snapshot, wall_limit, &skip_signatures, algorithm.as_deref())
            })
            .await
            .unwrap_or_else(|error| {
                json!({"status":"impossible","reason":format!("search-task-failed: {error}")})
            });
            let mut current = state.switch_plan.write().await;
            let current_generation = if debug {
                state.debug_generation.load(Ordering::SeqCst)
            } else {
                *state.switch_generation.borrow()
            };
            if current_generation != generation {
                return vec![];
            }
            if let Some(object) = plan.as_object_mut() {
                object.insert(
                    "request_source".into(),
                    Value::String(
                        if action == "start_debug" {
                            "debug"
                        } else {
                            "live"
                        }
                        .into(),
                    ),
                );
            }
            plan["plan_id"] = json!(format!(
                "{}-{generation}",
                if debug { "debug" } else { "live" }
            ));
            plan["generation"] = json!(generation);
            plan["session_id"] = session;
            plan["state_revision"] = revision;
            if !debug {
                *current = Some(plan.clone());
            }
            vec![event(
                "discard_recommendation",
                json!([{"yaku":"souzu_switch","data":plan}]),
            )]
        }
        "list_quads" => {
            let catalog = recommendations::quad_catalog(&state.services.game_state(), wall_limit);
            vec![event(
                "discard_recommendation",
                json!([{"yaku":"souzu_switch","data":{"status":"catalog","quad_catalog":catalog}}]),
            )]
        }
        "validate_manual_debug" => {
            state.debug_generation.fetch_add(1, Ordering::SeqCst);
            let snapshot = data
                .get("snapshot")
                .cloned()
                .unwrap_or_else(|| state.services.game_state());
            let mut plan = recommendations::validate_manual_plan(
                &snapshot,
                wall_limit,
                data.get("quad_groups").unwrap_or(&Value::Null),
                data.get("structure_groups").unwrap_or(&Value::Null),
            );
            if let Some(object) = plan.as_object_mut() {
                object.insert("request_source".into(), Value::String("debug".into()));
            }
            vec![event(
                "discard_recommendation",
                json!([{"yaku":"souzu_switch","data":plan}]),
            )]
        }
        "execute_plan" | "execute_full_plan" => {
            let reject = |reason: &str| {
                vec![event(
                    "souzu_switch_control_result",
                    json!({"action":action,"ok":false,"reason":reason}),
                )]
            };
            let Ok(_execution) = state.switch_execution.try_lock() else {
                return reject("execution-already-running");
            };
            let plan = state.switch_plan.read().await.clone();
            let Some(plan) = plan else {
                return vec![event(
                    "souzu_switch_control_result",
                    json!({"action":action,"ok":false,"reason":"no-plan"}),
                )];
            };
            let mut cancellation = state.switch_generation.subscribe();
            let requested_id = data
                .get("options")
                .and_then(|v| v.get("plan_id"))
                .and_then(Value::as_str);
            let game = state.services.game_state();
            if requested_id.is_none()
                || requested_id != plan.get("plan_id").and_then(Value::as_str)
                || plan.get("request_source").and_then(Value::as_str) != Some("live")
                || plan.get("state_revision") != game.get("revision")
                || !plan_is_current(state, &plan).await
            {
                return reject("stale-plan");
            }
            let execute = async {
                if action == "execute_full_plan" {
                    execute_full_switch_plan(state, &plan).await
                } else {
                    execute_switch_batches(state, &plan, false).await
                }
            };
            let result = tokio::select! {
                biased;
                _ = cancellation.changed() => Err("stopped-by-user".into()),
                result = execute => result,
            };
            let (ok, reason) = match result {
                Ok(()) => (true, String::new()),
                Err(reason) => (false, reason),
            };
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":if ok{"completed"}else{"failed"},"phase":"done","reason":reason}),
            ));
            vec![event(
                "souzu_switch_control_result",
                json!({"action":action,"ok":ok,"reason":reason}),
            )]
        }
        "stop" => {
            state
                .switch_generation
                .send_modify(|generation| *generation += 1);
            state.debug_generation.fetch_add(1, Ordering::SeqCst);
            *state.switch_plan.write().await = None;
            vec![event(
                "discard_recommendation",
                json!([{"yaku":"souzu_switch","data":{"status":"impossible","reason":"stopped-by-user"}}]),
            )]
        }
        _ => vec![event(
            "souzu_switch_control_result",
            json!({"action":action,"ok":false,"reason":"unknown-action"}),
        )],
    }
}

fn value_ids(value: Option<&Value>) -> Vec<u64> {
    value
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

fn operation_available(game: &Value, operation_type: u64) -> bool {
    game.get("next_operation")
        .and_then(Value::as_array)
        .is_some_and(|operations| {
            operations.iter().any(|operation| {
                operation.get("type").and_then(Value::as_u64) == Some(operation_type)
            })
        })
}

fn deck_face(game: &Value, id: u64) -> Option<&str> {
    game.get("deck_map")?.get(id.to_string())?.as_str()
}

async fn plan_is_current(state: &State, plan: &Value) -> bool {
    let id = plan.get("plan_id").and_then(Value::as_str);
    id.is_some()
        && plan.get("generation").and_then(Value::as_u64) == Some(*state.switch_generation.borrow())
        && plan.get("session_id") == state.services.game_state().get("session_id")
        && state
            .switch_plan
            .read()
            .await
            .as_ref()
            .is_some_and(|current| current.get("plan_id").and_then(Value::as_str) == id)
}

async fn wait_for_game_change(
    state: &State,
    plan: &Value,
    old_hand: &[u64],
    old_change_count: u64,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let game = state.services.game_state();
        let hand = value_ids(game.get("hand_tiles"));
        let change_count = game
            .get("change_tile_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if hand != old_hand || change_count != old_change_count {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Err("game-state-update-timeout".into())
}

async fn send_game_operation(
    state: &State,
    operation_type: u64,
    tile_list: &[u64],
) -> Result<(), String> {
    match state
        .proxy
        .request_with_retry(
            ".lq.Lobby.amuletActivityGameOperate",
            &json!({"activityId":260511,"type":operation_type,"tileList":tile_list}),
            Duration::from_secs(12),
        )
        .await
    {
        Ok((_, response)) if response.get("error").is_none() => Ok(()),
        Ok((_, response)) => Err(format!(
            "protocol-error: {}",
            response.get("error").unwrap_or(&Value::Null)
        )),
        Err(error) => Err(error),
    }
}

async fn execute_switch_batches(
    state: &State,
    plan: &Value,
    finish_switch: bool,
) -> Result<(), String> {
    if plan.get("status").and_then(Value::as_str) != Some("plan") {
        return Err("plan-not-executable".into());
    }
    let raw = plan
        .get("switch_discards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let batches = if raw.first().is_some_and(Value::is_number) {
        vec![Value::Array(raw)]
    } else {
        raw
    };
    for (index, batch) in batches.iter().enumerate() {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let discard_ids = value_ids(Some(batch));
        let game = state.services.game_state();
        let hand = value_ids(game.get("hand_tiles"));
        if discard_ids.is_empty() || discard_ids.iter().any(|id| !hand.contains(id)) {
            return Err("switch-plan-no-longer-matches-hand".into());
        }
        let keep = hand
            .iter()
            .copied()
            .filter(|id| !discard_ids.contains(id))
            .collect::<Vec<_>>();
        let old_change_count = game
            .get("change_tile_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let _ = state.services.events.send(event(
            "souzu_switch_execution",
            json!({"status":"running","phase":"switch","batch_index":index+1,"batch_total":batches.len(),"batch_count":batches.len()}),
        ));
        send_game_operation(state, 101, &keep).await?;
        wait_for_game_change(
            state,
            plan,
            &hand,
            old_change_count,
            Duration::from_secs(12),
        )
        .await?;
    }
    if !finish_switch {
        return Ok(());
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let game = state.services.game_state();
        if operation_available(&game, 100) {
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":"running","phase":"finish-switch"}),
            ));
            match state
                .proxy
                .request_with_retry(
                    ".lq.Lobby.amuletActivityOperate",
                    &json!({"activityId":260511,"type":3,"args":[]}),
                    Duration::from_secs(12),
                )
                .await
            {
                Ok((_, response)) if response.get("error").is_none() => return Ok(()),
                Ok((_, response)) => {
                    return Err(format!(
                        "protocol-error: {}",
                        response.get("error").unwrap_or(&Value::Null)
                    ))
                }
                Err(error) => return Err(error),
            }
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Err("finish-switch-operation-timeout".into())
}

fn choose_discard(game: &Value, plan: &Value) -> Option<u64> {
    let hand = value_ids(game.get("hand_tiles"));
    if let Some(planned) = plan.get("post_draw_discards").and_then(Value::as_array) {
        if let Some(id) = planned
            .iter()
            .filter_map(Value::as_u64)
            .find(|id| hand.contains(id))
        {
            return Some(id);
        }
    }
    let mut wanted = HashMap::<String, usize>::new();
    for face in plan
        .get("target13")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        *wanted.entry(face.to_owned()).or_default() += 1;
    }
    let mut kept = HashMap::<String, usize>::new();
    for id in hand.iter().copied() {
        let face = deck_face(game, id).unwrap_or("");
        let count = kept.entry(face.to_owned()).or_default();
        if *count >= wanted.get(face).copied().unwrap_or(0) {
            return Some(id);
        }
        *count += 1;
    }
    hand.last().copied()
}

async fn execute_full_switch_plan(state: &State, plan: &Value) -> Result<(), String> {
    if plan
        .get("mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.starts_with("wanxiang"))
    {
        return Err("wanxiang-plan-has-no-full-kan-execution".into());
    }
    execute_switch_batches(state, plan, true).await?;
    let mut pending_quads = plan
        .get("quad_faces")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if pending_quads.len() != 2 {
        return Err("full-plan-requires-two-quads".into());
    }
    let draws_needed = plan
        .get("draws_needed")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut post_kan_discards = 0usize;
    while Instant::now() < deadline {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let game = state.services.game_state();
        let hand = value_ids(game.get("hand_tiles"));
        if let Some((quad_index, quad_ids)) =
            pending_quads.iter().enumerate().find_map(|(index, face)| {
                let ids = hand
                    .iter()
                    .copied()
                    .filter(|id| deck_face(&game, *id) == Some(face.as_str()))
                    .take(4)
                    .collect::<Vec<_>>();
                (ids.len() == 4).then_some((index, ids))
            })
        {
            if operation_available(&game, 4) {
                let _ = state.services.events.send(event(
                    "souzu_switch_execution",
                    json!({"status":"running","phase":"kan","quad_face":pending_quads[quad_index]}),
                ));
                send_game_operation(state, 4, &quad_ids).await?;
                let change_count = game
                    .get("change_tile_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                wait_for_game_change(state, plan, &hand, change_count, Duration::from_secs(12))
                    .await?;
                pending_quads.remove(quad_index);
                continue;
            }
        }
        if pending_quads.is_empty()
            && (operation_available(&game, 8)
                || game
                    .get("ting_list")
                    .and_then(Value::as_array)
                    .is_some_and(|values| !values.is_empty())
                || post_kan_discards >= draws_needed)
        {
            return Ok(());
        }
        if operation_available(&game, 1) {
            let discard = choose_discard(&game, plan).ok_or("no-safe-discard")?;
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":"running","phase":"discard","tile_id":discard}),
            ));
            send_game_operation(state, 1, &[discard]).await?;
            let change_count = game
                .get("change_tile_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            wait_for_game_change(state, plan, &hand, change_count, Duration::from_secs(12)).await?;
            if pending_quads.is_empty() {
                post_kan_discards += 1;
            }
            continue;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Err("full-plan-execution-timeout".into())
}

async fn request_reply(state: &State, method: &str, payload: Value) -> CommandResult {
    match state
        .proxy
        .request_with_retry(method, &payload, Duration::from_secs(12))
        .await
    {
        Ok((id, response)) => CommandResult {
            ok: response.get("error").is_none(),
            reason: response
                .get("error")
                .is_some()
                .then(|| "protocol-error".into()),
            msg_id: Some(id),
            response: Some(response),
            ..Default::default()
        },
        Err(error) => failed(error),
    }
}

async fn hotkey_reply(state: &State, request: AmuletActionRequest) -> CommandResult {
    let action = request.action.as_str();
    let (method, payload) = match request.action {
        AmuletAction::BuyPack => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":4,"args":[request.good_id.unwrap_or(0)]}),
        ),
        AmuletAction::RefreshShop => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":9,"args":[]}),
        ),
        AmuletAction::Skip => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":3,"args":[]}),
        ),
        AmuletAction::SelectCandidate => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":16,"args":[request.selected_index.unwrap_or(0)]}),
        ),
        AmuletAction::SellEffect => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":5,"args":[request.uid.unwrap_or(0)]}),
        ),
        AmuletAction::SortEffect => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":8,"args":request.sorted_uid.unwrap_or_default()}),
        ),
        AmuletAction::SellRecent => {
            let state_value = state.services.game_state();
            let raw_id = request.raw_id;
            let uid = state_value
                .get("effect_list")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items.iter().rev().find(|item| {
                        raw_id.is_none() || item.get("id").and_then(Value::as_u64) == raw_id
                    })
                })
                .and_then(|item| item.get("uid"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            (
                ".lq.Lobby.amuletActivityOperate",
                json!({"activityId":260511,"type":5,"args":[uid]}),
            )
        }
    };
    let mut result = request_reply(state, method, payload).await;
    result.action = Some(action.into());
    result
}

fn failed(reason: impl Into<String>) -> CommandResult {
    CommandResult {
        reason: Some(reason.into()),
        ..Default::default()
    }
}

fn snapshot_part<T: serde::de::DeserializeOwned>(value: Value, name: &str) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("invalid {name}: {error}"))
}

#[cfg(test)]
mod audit_tests {
    use super::*;

    #[tokio::test]
    async fn debug_cannot_replace_live_plan_and_execution_checks_identity_and_session() {
        const CHILD_ROOT: &str = "SHANTEN_SWITCH_TEST_ROOT";
        let root = match std::env::var_os(CHILD_ROOT) {
            Some(root) => PathBuf::from(root),
            None => {
                let root = std::env::temp_dir().join(format!(
                    "shanten-switch-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                // Runtime logging is process-global; finish that process before removing its files.
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "embedded::audit_tests::debug_cannot_replace_live_plan_and_execution_checks_identity_and_session",
                        "--nocapture",
                    ])
                    .env(CHILD_ROOT, &root)
                    .output()
                    .unwrap();
                assert!(output.status.success(), "{output:?}");
                std::fs::remove_dir_all(root).unwrap();
                return;
            }
        };
        let runtime_root = root.join("runtime");
        let runtime = tokio::task::spawn_blocking(move || BackendRuntime::load(runtime_root))
            .await
            .unwrap()
            .unwrap();
        let state = &runtime.state;
        switch_command(state, &json!({"action":"start"})).await;
        let live = state.switch_plan.read().await.clone().unwrap();
        switch_command(state, &json!({"action":"start_debug","snapshot":{}})).await;
        switch_command(
            state,
            &json!({"action":"validate_manual_debug","snapshot":{}}),
        )
        .await;
        assert_eq!(state.switch_plan.read().await.as_ref(), Some(&live));
        assert!(plan_is_current(state, &live).await);
        let result = switch_command(
            state,
            &json!({"action":"execute_plan","options":{"plan_id":"debug-1"}}),
        )
        .await;
        assert_eq!(result[0]["data"]["reason"], "stale-plan");
        let lock = state.switch_execution.lock().await;
        let result = switch_command(
            state,
            &json!({"action":"execute_plan","options":{"plan_id":live["plan_id"]}}),
        )
        .await;
        assert_eq!(result[0]["data"]["reason"], "execution-already-running");
        drop(lock);
        state.services.update_game_state(&pipeline::Packet {
            direction: pipeline::Direction::Inbound,
            packet_type: "Res".into(),
            method: ".lq.Lobby.fetchAmuletActivityData".into(),
            id: Some(1),
            data: json!({"game":{"state":{"current":5}}}),
        });
        assert!(!plan_is_current(state, &live).await);
        let result = switch_command(
            state,
            &json!({"action":"execute_plan","options":{"plan_id":live["plan_id"]}}),
        )
        .await;
        assert_eq!(result[0]["data"]["reason"], "stale-plan");
        let mut cancel = state.switch_generation.subscribe();
        switch_command(state, &json!({"action":"stop"})).await;
        assert!(cancel.has_changed().unwrap());
        cancel.changed().await.unwrap();
        assert!(state.switch_plan.read().await.is_none());
        drop(runtime);
    }
}
