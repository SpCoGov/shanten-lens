use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use serde_json::Value;
use shanten_backend::embedded::BackendRuntime;
use shanten_backend::ipc::{
    AmuletActionRequest, AutorunAction, BackendSnapshot, CommandResult, ConfigTables,
    FlowDumpResult, JsonMap, PacketLogSnapshot, SwitchRequest, TsumoLoopStatus, VersionMismatch,
};
use shanten_backend::pipeline::PipelineConfig;
use tauri::{AppHandle, Emitter, Manager, State};

mod overlay;

const STARTUP_PROGRESS_EVENT: &str = "startup:progress";

#[derive(Default)]
struct GateState {
    backend_ready: AtomicBool,
    frontend_ready: AtomicBool,
    switched: AtomicBool,
}

struct SharedGate(pub Arc<GateState>);

#[derive(Clone, Serialize)]
struct StartupProgressPayload {
    phase: String,
    label: String,
    detail: Option<String>,
    progress: f64,
    eta_seconds: Option<u64>,
    indeterminate: bool,
}

struct StartupProgressState(pub Arc<Mutex<StartupProgressPayload>>);

fn default_startup_progress() -> StartupProgressPayload {
    StartupProgressPayload {
        phase: "bootstrap".into(),
        label: "正在准备启动".into(),
        detail: Some("初始化窗口与运行环境".into()),
        progress: 0.08,
        eta_seconds: Some(8),
        indeterminate: false,
    }
}

fn emit_startup_progress(app: &AppHandle, payload: &StartupProgressPayload) {
    let _ = app.emit(STARTUP_PROGRESS_EVENT, payload.clone());
}

fn set_startup_progress(
    app: &AppHandle,
    state: &StartupProgressState,
    payload: StartupProgressPayload,
) {
    if let Ok(mut guard) = state.0.lock() {
        *guard = payload.clone();
    }
    emit_startup_progress(app, &payload);
}

fn maybe_switch(app: &AppHandle, gate: &GateState) {
    if gate.switched.load(Ordering::SeqCst) {
        return;
    }
    if gate.backend_ready.load(Ordering::SeqCst) && gate.frontend_ready.load(Ordering::SeqCst) {
        if gate.switched.swap(true, Ordering::SeqCst) == false {
            if let Some(progress_state) = app.try_state::<StartupProgressState>() {
                set_startup_progress(
                    app,
                    &progress_state,
                    StartupProgressPayload {
                        phase: "ready".into(),
                        label: "启动完成".into(),
                        detail: Some("正在进入主界面".into()),
                        progress: 1.0,
                        eta_seconds: Some(0),
                        indeterminate: false,
                    },
                );
            }
            if let Some(s) = app.get_webview_window("splash") {
                let _ = s.close();
            }
            if let Some(m) = app.get_webview_window("main") {
                let _ = m.show();
                let _ = m.set_focus();
            }
        }
    }
}

struct IpcBackendState(pub BackendRuntime);

#[tauri::command]
#[specta::specta]
async fn backend_snapshot(state: State<'_, IpcBackendState>) -> Result<BackendSnapshot, String> {
    state.0.snapshot().await
}

#[tauri::command]
#[specta::specta]
fn backend_check_version(
    version: String,
    state: State<'_, IpcBackendState>,
) -> Option<VersionMismatch> {
    state.0.check_frontend_version(version)
}

#[tauri::command]
#[specta::specta]
async fn backend_get_packet_pipeline(
    state: State<'_, IpcBackendState>,
) -> Result<PipelineConfig, String> {
    Ok(state.0.packet_pipeline().await)
}

#[tauri::command]
#[specta::specta]
async fn backend_set_packet_pipeline(
    config: PipelineConfig,
    state: State<'_, IpcBackendState>,
) -> Result<CommandResult, String> {
    Ok(state.0.set_packet_pipeline(config).await)
}

#[tauri::command]
#[specta::specta]
fn backend_get_packet_log(state: State<'_, IpcBackendState>) -> PacketLogSnapshot {
    state.0.packet_log()
}

#[tauri::command]
#[specta::specta]
async fn backend_replay_packet(
    method: String,
    payload: JsonMap,
    state: State<'_, IpcBackendState>,
) -> Result<CommandResult, String> {
    Ok(state.0.replay_packet(method, payload).await)
}

#[tauri::command]
#[specta::specta]
async fn backend_update_config(
    config: ConfigTables,
    state: State<'_, IpcBackendState>,
) -> Result<(), String> {
    state.0.update_config(config)
}

#[tauri::command]
#[specta::specta]
fn backend_set_locale(app: AppHandle, state: State<'_, IpcBackendState>, locale: String) {
    if let Some(window) = app.get_webview_window("main") {
        let title = if locale.to_ascii_lowercase().starts_with("ja") {
            "向聴レンズ - Shanten Lens"
        } else {
            "向听镜 - Shanten Lens"
        };
        let _ = window.set_title(title);
    }
    state.0.set_locale(locale);
}

#[tauri::command]
#[specta::specta]
fn backend_dump_flows(state: State<'_, IpcBackendState>) -> FlowDumpResult {
    state.0.dump_flows()
}

#[tauri::command]
#[specta::specta]
async fn backend_fetch_activity(
    activity_id: u64,
    state: State<'_, IpcBackendState>,
) -> Result<CommandResult, String> {
    Ok(state.0.fetch_activity(activity_id).await)
}

#[tauri::command]
#[specta::specta]
async fn backend_discard_tile(
    tile_id: u64,
    state: State<'_, IpcBackendState>,
) -> Result<CommandResult, String> {
    Ok(state.0.discard_tile(tile_id).await)
}

#[tauri::command]
#[specta::specta]
async fn backend_upgrade_shop_buff(
    activity_id: u64,
    id: u64,
    state: State<'_, IpcBackendState>,
) -> Result<CommandResult, String> {
    Ok(state.0.upgrade_shop_buff(activity_id, id).await)
}

#[tauri::command]
#[specta::specta]
async fn backend_amulet_action(
    request: AmuletActionRequest,
    state: State<'_, IpcBackendState>,
) -> Result<CommandResult, String> {
    Ok(state.0.amulet_action(request).await)
}

#[tauri::command]
#[specta::specta]
fn backend_start_tsumo_loop(
    interval_ms: u64,
    reset_count: bool,
    state: State<'_, IpcBackendState>,
) -> TsumoLoopStatus {
    state.0.start_tsumo_loop(interval_ms, reset_count)
}

#[tauri::command]
#[specta::specta]
fn backend_stop_tsumo_loop(state: State<'_, IpcBackendState>) -> TsumoLoopStatus {
    state.0.stop_tsumo_loop()
}

#[tauri::command]
#[specta::specta]
async fn backend_autorun(
    action: AutorunAction,
    force: bool,
    mode: Option<String>,
    state: State<'_, IpcBackendState>,
) -> Result<CommandResult, String> {
    Ok(state.0.autorun(action, force, mode).await)
}

#[tauri::command]
#[specta::specta]
fn backend_resolve_confirmation(id: String, ok: bool, state: State<'_, IpcBackendState>) {
    state.0.resolve_confirmation(id, ok);
}

#[tauri::command]
#[specta::specta]
async fn backend_switch(
    request: SwitchRequest,
    state: State<'_, IpcBackendState>,
) -> Result<(), String> {
    state.0.switch(request).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn backend_open_config_dir(state: State<'_, IpcBackendState>) -> Result<(), String> {
    state.0.open_config_dir()
}

#[tauri::command]
fn shutdown_app(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn frontend_ready(app: AppHandle, gate: State<SharedGate>) {
    if let Some(progress_state) = app.try_state::<StartupProgressState>() {
        let backend_ready = gate.0.backend_ready.load(Ordering::SeqCst);
        set_startup_progress(
            &app,
            &progress_state,
            StartupProgressPayload {
                phase: if backend_ready {
                    "ready".into()
                } else {
                    "render_wait_backend".into()
                },
                label: if backend_ready {
                    "启动完成".into()
                } else {
                    "界面已准备好".into()
                },
                detail: if backend_ready {
                    Some("正在进入主界面".into())
                } else {
                    Some("正在等待后台服务连接".into())
                },
                progress: if backend_ready { 1.0 } else { 0.9 },
                eta_seconds: if backend_ready { Some(0) } else { None },
                indeterminate: !backend_ready,
            },
        );
    }
    gate.0.frontend_ready.store(true, Ordering::SeqCst);
    maybe_switch(&app, &gate.0);
}

#[tauri::command]
fn update_startup_progress(
    app: AppHandle,
    state: State<StartupProgressState>,
    phase: String,
    label: String,
    detail: Option<String>,
    progress: f64,
    eta_seconds: Option<u64>,
    indeterminate: Option<bool>,
) {
    set_startup_progress(
        &app,
        &state,
        StartupProgressPayload {
            phase,
            label,
            detail,
            progress: progress.clamp(0.0, 1.0),
            eta_seconds,
            indeterminate: indeterminate.unwrap_or(false),
        },
    );
}

#[tauri::command]
fn get_startup_progress(
    state: State<StartupProgressState>,
) -> Result<StartupProgressPayload, String> {
    state
        .0
        .lock()
        .map(|guard| guard.clone())
        .map_err(|_| "mutex poisoned".to_string())
}

#[tauri::command]
fn is_overlay_supported() -> bool {
    overlay::is_overlay_supported()
}

#[cfg(windows)]
#[tauri::command]
fn get_overlay_status(state: State<overlay::OverlayState>) -> overlay::OverlayStatus {
    overlay::get_status(state)
}

#[cfg(not(windows))]
#[tauri::command]
fn get_overlay_status() -> overlay::OverlayStatus {
    overlay::get_status()
}

#[cfg(windows)]
#[tauri::command]
fn set_overlay_enabled(
    state: State<overlay::OverlayState>,
    enabled: bool,
) -> overlay::OverlayStatus {
    overlay::set_enabled(state, enabled)
}

#[cfg(not(windows))]
#[tauri::command]
fn set_overlay_enabled(enabled: bool) -> overlay::OverlayStatus {
    overlay::set_enabled(enabled)
}

#[cfg(windows)]
#[tauri::command]
fn set_overlay_interactive(state: State<overlay::OverlayState>, interactive: bool) {
    overlay::set_interactive(state, interactive)
}

#[cfg(not(windows))]
#[tauri::command]
fn set_overlay_interactive(interactive: bool) {
    overlay::set_interactive(interactive)
}

#[cfg(windows)]
#[tauri::command]
fn set_overlay_panel_regions(
    state: State<overlay::OverlayState>,
    regions: Vec<overlay::PanelRegion>,
) {
    overlay::set_panel_regions(state, regions)
}

#[cfg(not(windows))]
#[tauri::command]
fn set_overlay_panel_regions(regions: Vec<overlay::PanelRegion>) {
    overlay::set_panel_regions(regions)
}

#[tauri::command]
fn fetch_latest_release(use_system_proxy: bool) -> Result<String, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent("Shanten-Lens-Updater")
        .timeout(Duration::from_secs(15));
    if !use_system_proxy {
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    let response = client
        .get("https://api.github.com/repos/SpCoGov/shanten-lens/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("GitHub Releases API returned {status}: {text}"));
    }
    Ok(text)
}

fn ipc_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new()
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
        .commands(tauri_specta::collect_commands![
            backend_snapshot,
            backend_check_version,
            backend_get_packet_pipeline,
            backend_set_packet_pipeline,
            backend_get_packet_log,
            backend_replay_packet,
            backend_update_config,
            backend_set_locale,
            backend_dump_flows,
            backend_fetch_activity,
            backend_discard_tile,
            backend_upgrade_shop_buff,
            backend_amulet_action,
            backend_start_tsumo_loop,
            backend_stop_tsumo_loop,
            backend_autorun,
            backend_resolve_confirmation,
            backend_switch,
            backend_open_config_dir,
        ])
}

fn export_ipc_bindings(ipc: &tauri_specta::Builder<tauri::Wry>) {
    ipc.export(
        specta_typescript::Typescript::default()
            .bigint(specta_typescript::BigIntExportBehavior::Number),
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.ts"),
    )
    .expect("failed to export Tauri IPC bindings");
}

pub fn run() {
    let ipc = ipc_builder();
    #[cfg(debug_assertions)]
    export_ipc_bindings(&ipc);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .manage(SharedGate(Arc::new(GateState::default())))
        .manage(StartupProgressState(Arc::new(Mutex::new(
            default_startup_progress(),
        ))))
        .invoke_handler(tauri::generate_handler![
            shutdown_app,
            frontend_ready,
            update_startup_progress,
            get_startup_progress,
            is_overlay_supported,
            get_overlay_status,
            set_overlay_enabled,
            set_overlay_interactive,
            set_overlay_panel_regions,
            fetch_latest_release,
            backend_snapshot,
            backend_check_version,
            backend_get_packet_pipeline,
            backend_set_packet_pipeline,
            backend_get_packet_log,
            backend_replay_packet,
            backend_update_config,
            backend_set_locale,
            backend_dump_flows,
            backend_fetch_activity,
            backend_discard_tile,
            backend_upgrade_shop_buff,
            backend_amulet_action,
            backend_start_tsumo_loop,
            backend_stop_tsumo_loop,
            backend_autorun,
            backend_resolve_confirmation,
            backend_switch,
            backend_open_config_dir
        ])
        .setup(|app| {
            let runtime_root = app.path().app_data_dir()?.join("configs");
            let backend = BackendRuntime::load(runtime_root)?;
            app.manage(IpcBackendState(backend.clone()));

            let ah = app.handle().clone();
            let gate = app.state::<SharedGate>().0.clone();
            let startup_progress = app.state::<StartupProgressState>();

            let mut backend_events = backend.subscribe();
            let event_app = ah.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match backend_events.recv().await {
                        Ok(payload) => {
                            if let (Some(kind), Some(data)) = (
                                payload.get("type").and_then(Value::as_str),
                                payload.get("data"),
                            ) {
                                let _ = event_app.emit(&format!("backend:{kind}"), data.clone());
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            tauri::async_runtime::spawn(backend.run_background());

            set_startup_progress(
                &ah,
                &startup_progress,
                StartupProgressPayload {
                    phase: "bootstrap".into(),
                    label: "正在初始化应用".into(),
                    detail: Some("创建窗口并准备启动流程".into()),
                    progress: 0.1,
                    eta_seconds: Some(8),
                    indeterminate: false,
                },
            );

            gate.backend_ready.store(true, Ordering::SeqCst);
            let _ = ah.emit("backend:ready", "embedded IPC backend ready");
            maybe_switch(&ah, &gate);

            overlay::start(&ah);

            {
                let ah2 = ah.clone();
                let gate2 = gate.clone();
                tauri::async_runtime::spawn(async move {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    if !gate2.switched.load(Ordering::SeqCst) {
                        if let Some(progress_state) = ah2.try_state::<StartupProgressState>() {
                            set_startup_progress(
                                &ah2,
                                &progress_state,
                                StartupProgressPayload {
                                    phase: "fallback".into(),
                                    label: "主界面已打开".into(),
                                    detail: Some(
                                        "启动超时，已直接进入主界面，可稍后继续连接后台".into(),
                                    ),
                                    progress: 0.96,
                                    eta_seconds: Some(0),
                                    indeterminate: false,
                                },
                            );
                        }
                        if let Some(s) = ah2.get_webview_window("splash") {
                            let _ = s.close();
                        }
                        if let Some(m) = ah2.get_webview_window("main") {
                            let _ = m.show();
                            let _ = m.set_focus();
                        }
                        gate2.switched.store(true, Ordering::SeqCst);
                    }
                });
            }

            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
