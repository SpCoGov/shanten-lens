use std::{sync::Mutex, time::Duration};

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct AppUpdater {
    download: tokio::sync::Mutex<()>,
    // ponytail: keep one installer in memory until exit; use a disk cache if download resumption is needed.
    pending: Mutex<Option<(Update, Vec<u8>)>>,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    downloaded: u64,
    total: Option<u64>,
}

#[tauri::command]
pub fn app_updater_enabled(app: AppHandle) -> bool {
    !cfg!(debug_assertions)
        && cfg!(target_os = "windows")
        && app
            .config()
            .plugins
            .0
            .get("updater")
            .and_then(|config| config.get("pubkey"))
            .and_then(|key| key.as_str())
            .is_some_and(|key| !key.trim().is_empty())
}

#[tauri::command]
pub async fn download_app_update(
    app: AppHandle,
    state: State<'_, AppUpdater>,
    version: String,
    use_system_proxy: bool,
    on_progress: Channel<DownloadProgress>,
) -> Result<(), String> {
    if !app_updater_enabled(app.clone()) {
        return Err("Automatic updates are unavailable in this build".into());
    }
    let _download = state.download.try_lock().map_err(|e| e.to_string())?;
    {
        let pending = state.pending.lock().map_err(|e| e.to_string())?;
        if pending
            .as_ref()
            .is_some_and(|(update, _)| update.version == version)
        {
            return Ok(());
        }
    }
    let mut builder = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .restart_after_install(false);
    if !use_system_proxy {
        builder = builder.no_proxy();
    }
    let mut update = builder
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No update available")?;
    // Never install a different release from the one shown in the UI.
    if update.version != version {
        return Err("The release changed; check for updates again".into());
    }
    update.timeout = Some(Duration::from_secs(600));
    let mut downloaded = 0;
    // The official downloader verifies the signature before returning any bytes.
    let bytes = update
        .download(
            |length, total| {
                downloaded += length as u64;
                let _ = on_progress.send(DownloadProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    *state.pending.lock().map_err(|e| e.to_string())? = Some((update, bytes));
    Ok(())
}

#[tauri::command]
pub fn discard_app_update(state: State<'_, AppUpdater>) -> Result<(), String> {
    let _download = state.download.try_lock().map_err(|e| e.to_string())?;
    state.pending.lock().map_err(|e| e.to_string())?.take();
    Ok(())
}

pub fn install_on_exit(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppUpdater>();
    let pending = state.pending.lock().map_err(|e| e.to_string())?.take();
    if let Some((update, bytes)) = pending {
        update.install(bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}
