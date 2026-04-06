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
  gate.0.frontend_ready.store(true, Ordering::SeqCst);
  maybe_switch(&app, &gate.0);
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
    .invoke_handler(tauri::generate_handler![start_backend, stop_backend, frontend_ready])
    .setup(|app| {
      let ah = app.handle().clone();
      let st = app.state::<BackendState>().0.clone();
      let gate = app.state::<SharedGate>().0.clone();

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
          std::thread::sleep(std::time::Duration::from_secs(20));
          if !gate2.switched.load(Ordering::SeqCst) {
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
