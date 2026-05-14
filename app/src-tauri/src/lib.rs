use std::{
  env,
  io::{BufRead, BufReader},
  path::{Path, PathBuf},
  process::{Child, Command, Stdio},
  time::{Duration, Instant},
  sync::{Arc, Mutex},
  thread,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

const LOG_BATCH_MAX_LINES: usize = 64;
const LOG_BATCH_MAX_BYTES: usize = 64 * 1024;
const LOG_CHUNK_MAX_BYTES: usize = 16 * 1024;
const STARTUP_PROGRESS_EVENT: &str = "startup:progress";

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LogPayload {
  Lines { lines: Vec<String> },
  Chunk { id: u64, index: usize, total: usize, text: String },
}

#[derive(Default)]
struct BackendProcState {
  child: Option<Child>,
}

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

fn split_utf8_chunks(s: &str, max_bytes: usize) -> Vec<String> {
  if s.len() <= max_bytes {
    return vec![s.to_string()];
  }

  let mut chunks = Vec::new();
  let mut start = 0;
  while start < s.len() {
    let mut end = (start + max_bytes).min(s.len());
    while end > start && !s.is_char_boundary(end) {
      end -= 1;
    }
    if end == start {
      end = s[start..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| start + i)
        .unwrap_or(s.len());
    }
    chunks.push(s[start..end].to_string());
    start = end;
  }
  chunks
}

fn maybe_switch(app: &AppHandle, gate: &GateState) {
  if gate.switched.load(Ordering::SeqCst) { return; }
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
      if let Some(s) = app.get_webview_window("splash") { let _ = s.close(); }
      if let Some(m) = app.get_webview_window("main") {
        let _ = m.show();
        let _ = m.set_focus();
        let _ = m.set_title("向听镜 - Shanten Lens");
      }
    }
  }
}

fn spawn_log_pump(
  app: AppHandle,
  event_name: &'static str,
  ready_gate: Option<Arc<GateState>>,
  reader: impl std::io::Read + Send + 'static,
) {
  thread::spawn(move || {
    let reader = BufReader::new(reader);
    let mut batch: Vec<String> = Vec::new();
    let mut batch_bytes = 0usize;
    let mut last_flush = Instant::now();
    let mut chunk_id = 0u64;

    let flush = |app: &AppHandle, batch: &mut Vec<String>, batch_bytes: &mut usize, last_flush: &mut Instant| {
      if batch.is_empty() {
        return;
      }
      let payload = LogPayload::Lines {
        lines: std::mem::take(batch),
      };
      let _ = app.emit(event_name, payload);
      *batch_bytes = 0;
      *last_flush = Instant::now();
    };

    for line in reader.lines().flatten() {
      if line.contains("SL_BACKEND_READY") {
        if let Some(progress_state) = app.try_state::<StartupProgressState>() {
          let label = if ready_gate.as_ref().map(|g| g.frontend_ready.load(Ordering::SeqCst)).unwrap_or(false) {
            "后台服务已就绪"
          } else {
            "后台服务已启动"
          };
          let detail = if ready_gate.as_ref().map(|g| g.frontend_ready.load(Ordering::SeqCst)).unwrap_or(false) {
            "正在打开主界面"
          } else {
            "正在等待界面完成渲染"
          };
          set_startup_progress(
            &app,
            &progress_state,
            StartupProgressPayload {
              phase: "backend_ready".into(),
              label: label.into(),
              detail: Some(detail.into()),
              progress: if ready_gate.as_ref().map(|g| g.frontend_ready.load(Ordering::SeqCst)).unwrap_or(false) { 0.98 } else { 0.74 },
              eta_seconds: Some(1),
              indeterminate: false,
            },
          );
        }
        if let Some(gate2) = ready_gate.as_ref() {
          gate2.backend_ready.store(true, Ordering::SeqCst);
          maybe_switch(&app, gate2);
        }
        let _ = app.emit("backend:ready", line.clone());
      }

      if line.len() > LOG_CHUNK_MAX_BYTES {
        flush(&app, &mut batch, &mut batch_bytes, &mut last_flush);
        let parts = split_utf8_chunks(&line, LOG_CHUNK_MAX_BYTES);
        let total = parts.len();
        chunk_id = chunk_id.wrapping_add(1);
        for (index, text) in parts.into_iter().enumerate() {
          let payload = LogPayload::Chunk { id: chunk_id, index, total, text };
          let _ = app.emit(event_name, payload);
        }
        continue;
      }

      batch_bytes += line.len();
      batch.push(line);

      let should_flush =
        batch.len() >= LOG_BATCH_MAX_LINES ||
        batch_bytes >= LOG_BATCH_MAX_BYTES ||
        last_flush.elapsed() >= Duration::from_millis(50);
      if should_flush {
        flush(&app, &mut batch, &mut batch_bytes, &mut last_flush);
      }
    }

    flush(&app, &mut batch, &mut batch_bytes, &mut last_flush);
  });
}

struct BackendState(pub Arc<Mutex<BackendProcState>>);

#[cfg(windows)]
const BACKEND_BIN_NAME: &str = "shanten-backend.exe";
#[cfg(not(windows))]
const BACKEND_BIN_NAME: &str = "shanten-backend";

fn resolve_backend_path(app: &AppHandle) -> Option<PathBuf> {
  if let Ok(res_dir) = app.path().resource_dir() {
    let p = res_dir.join("bin").join(BACKEND_BIN_NAME);
    if p.exists() {
      return Some(p);
    }
  }
  let dev = Path::new("src-tauri")
    .join("resources")
    .join("bin")
    .join(BACKEND_BIN_NAME);
  if dev.exists() {
    return Some(dev);
  }
  let dev2 = Path::new("src-tauri").join("bin").join(BACKEND_BIN_NAME);
  if dev2.exists() {
    return Some(dev2);
  }
  if let Ok(exe) = env::current_exe() {
    if let Some(dir) = exe.parent() {
      let p = dir.join("resources").join("bin").join(BACKEND_BIN_NAME);
      if p.exists() {
        return Some(p);
      }
      let p2 = dir.join("bin").join(BACKEND_BIN_NAME);
      if p2.exists() {
        return Some(p2);
      }
    }
  }
  None
}

fn start_backend_with(app: AppHandle, st: Arc<Mutex<BackendProcState>>) -> Result<String, String> {
  {
    let mut g = st.lock().map_err(|_| "mutex poisoned".to_string())?;
    if let Some(ch) = g.child.as_mut() {
      if ch.try_wait().map_err(|e| e.to_string())?.is_none() {
        return Ok("already running".into());
      }
    }
  }

  let exe = resolve_backend_path(&app).ok_or_else(|| "backend exe not found".to_string())?;
  let gate = app.state::<SharedGate>().0.clone();
  let progress_state = app.state::<StartupProgressState>();

  set_startup_progress(
    &app,
    &progress_state,
    StartupProgressPayload {
      phase: "starting_backend".into(),
      label: "正在启动后台服务".into(),
      detail: Some(format!(
        "加载 {}",
        exe.file_name().and_then(|s| s.to_str()).unwrap_or(BACKEND_BIN_NAME)
      )),
      progress: 0.22,
      eta_seconds: Some(6),
      indeterminate: false,
    },
  );

  let mut cmd = Command::new(&exe);
  cmd.args([
      "--host", "127.0.0.1",
      "--port", "8787"
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

  #[cfg(windows)]
  {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
  }

  let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;

  set_startup_progress(
    &app,
    &progress_state,
    StartupProgressPayload {
      phase: "waiting_backend".into(),
      label: "正在连接后台服务".into(),
      detail: Some("等待本地分析服务返回就绪信号".into()),
      progress: 0.46,
      eta_seconds: Some(4),
      indeterminate: true,
    },
  );

  if let Some(out) = child.stdout.take() {
    let app2 = app.clone();
    let gate2 = gate.clone();
    spawn_log_pump(app2, "backend:stdout", Some(gate2), out);
  }
  if let Some(err) = child.stderr.take() {
    let app2 = app.clone();
    spawn_log_pump(app2, "backend:stderr", None, err);
  }

  {
    let mut g = st.lock().map_err(|_| "mutex poisoned".to_string())?;
    g.child = Some(child);
  }

  let _ = app.emit("backend:spawn", format!("spawned: {}", exe.display()));
  Ok(format!("spawned: {}", exe.display()))
}

fn stop_backend_with(st: Arc<Mutex<BackendProcState>>) -> Result<String, String> {
  let mut g = st.lock().map_err(|_| "mutex poisoned".to_string())?;
  if let Some(mut ch) = g.child.take() {
    let _ = ch.kill();
    let _ = ch.wait();
    Ok("killed".into())
  } else {
    Ok("not running".into())
  }
}

#[tauri::command]
fn start_backend(app: AppHandle, state: State<BackendState>) -> Result<String, String> {
  let st = state.0.clone();
  start_backend_with(app, st)
}

#[tauri::command]
fn stop_backend(state: State<BackendState>) -> Result<String, String> {
  let st = state.0.clone();
  stop_backend_with(st)
}

#[tauri::command]
fn frontend_ready(app: AppHandle, gate: State<SharedGate>) {
  if let Some(progress_state) = app.try_state::<StartupProgressState>() {
    let backend_ready = gate.0.backend_ready.load(Ordering::SeqCst);
    set_startup_progress(
      &app,
      &progress_state,
      StartupProgressPayload {
        phase: if backend_ready { "ready".into() } else { "render_wait_backend".into() },
        label: if backend_ready { "启动完成".into() } else { "界面已准备好".into() },
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
fn get_startup_progress(state: State<StartupProgressState>) -> Result<StartupProgressPayload, String> {
  state.0.lock().map(|guard| guard.clone()).map_err(|_| "mutex poisoned".to_string())
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

// Windows 下兜底强杀所有同名后端进程（静默）
#[cfg(windows)]
fn kill_all_backends_silently() {
  use std::os::windows::process::CommandExt;
  let _ = Command::new("taskkill")
    .args(["/IM", "shanten-backend.exe", "/F", "/T"])
    .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
    .status();
}

#[cfg(not(windows))]
fn kill_all_backends_silently() {
}

pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_fs::init())
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_os::init())
    .manage(BackendState(Arc::new(Mutex::new(BackendProcState::default()))))
    .manage(SharedGate(Arc::new(GateState::default())))
    .manage(StartupProgressState(Arc::new(Mutex::new(default_startup_progress()))))
    .invoke_handler(tauri::generate_handler![
      start_backend,
      stop_backend,
      frontend_ready,
      update_startup_progress,
      get_startup_progress,
      fetch_latest_release
    ])
    .setup(|app| {
      let ah = app.handle().clone();
      let st = app.state::<BackendState>().0.clone();
      let gate = app.state::<SharedGate>().0.clone();
      let startup_progress = app.state::<StartupProgressState>();

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

      let force_autostart = std::env::var("FORCE_AUTOSTART_BACKEND")
        .map(|v| v == "1")
        .unwrap_or(false);

      #[cfg(debug_assertions)]
      {
        if force_autostart {
          let _ = start_backend_with(ah.clone(), st.clone());
        }
      }
      #[cfg(not(debug_assertions))]
      {
        let _ = start_backend_with(ah.clone(), st.clone());
      }

      if let Some(win) = app.get_webview_window("main") {
        let st2 = st.clone();
        win.on_window_event(move |e| {
          if matches!(e, WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed) {
            let _ = stop_backend_with(st2.clone());
            kill_all_backends_silently();
          }
        });
      }

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
                  detail: Some("启动超时，已直接进入主界面，可稍后继续连接后台".into()),
                  progress: 0.96,
                  eta_seconds: Some(0),
                  indeterminate: false,
                },
              );
            }
            if let Some(s) = ah2.get_webview_window("splash") { let _ = s.close(); }
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
