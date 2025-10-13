use std::{
  env,
  io::{BufRead, BufReader},
  path::{Path, PathBuf},
  process::{Child, Command, Stdio},
  sync::{Arc, Mutex},
  thread,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

#[derive(Default)]
struct BackendProcState {
  child: Option<Child>,
}

// 用 Arc<Mutex<...>> 避免临时借用生命周期问题
struct BackendState(pub Arc<Mutex<BackendProcState>>);

fn resolve_backend_path(app: &AppHandle) -> Option<PathBuf> {
  // 1) 打包后的资源目录 <app dir>/resources/bin/shanten-backend.exe
  if let Ok(res_dir) = app.path().resource_dir() {
    let p = res_dir.join("bin").join("shanten-backend.exe");
    if p.exists() {
      return Some(p);
    }
  }
  // 2) 开发路径：src-tauri/resources/bin/shanten-backend.exe
  let dev = Path::new("src-tauri")
    .join("resources")
    .join("bin")
    .join("shanten-backend.exe");
  if dev.exists() {
    return Some(dev);
  }
  // 3) 兼容 src-tauri/bin/shanten-backend.exe
  let dev2 = Path::new("src-tauri").join("bin").join("shanten-backend.exe");
  if dev2.exists() {
    return Some(dev2);
  }
  // 4) 兼容 <exe dir>/resources/bin/...
  if let Ok(exe) = env::current_exe() {
    if let Some(dir) = exe.parent() {
      let p = dir.join("resources").join("bin").join("shanten-backend.exe");
      if p.exists() {
        return Some(p);
      }
      let p2 = dir.join("bin").join("shanten-backend.exe");
      if p2.exists() {
        return Some(p2);
      }
    }
  }
  None
}

fn start_backend_with(app: AppHandle, st: Arc<Mutex<BackendProcState>>) -> Result<String, String> {
  // 已在运行就直接返回
  {
    let mut g = st.lock().map_err(|_| "mutex poisoned".to_string())?;
    if let Some(ch) = g.child.as_mut() {
      if ch.try_wait().map_err(|e| e.to_string())?.is_none() {
        return Ok("already running".into());
      }
    }
  }

  let exe = resolve_backend_path(&app).ok_or_else(|| "backend exe not found".to_string())?;

  let data_root = app.path()
    .app_data_dir().map_err(|e| e.to_string())?
    .join("shanten");
  let data_root_str = data_root.to_string_lossy().to_string();

  let mut cmd = Command::new(&exe);
  // 固定端口 + 传递数据根
  cmd.args([
      "--host", "127.0.0.1",
      "--port", "8787",
      "--data-root", &data_root_str
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

  // 隐藏控制台窗口（Windows）
  #[cfg(windows)]
  {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
  }

  let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;

  // 后端日志转发到前端
  if let Some(out) = child.stdout.take() {
    let app2 = app.clone();
    thread::spawn(move || {
      let reader = BufReader::new(out);
      for line in reader.lines().flatten() {
        let _ = app2.emit("backend:stdout", line);
      }
    });
  }
  if let Some(err) = child.stderr.take() {
    let app2 = app.clone();
    thread::spawn(move || {
      let reader = BufReader::new(err);
      for line in reader.lines().flatten() {
        let _ = app2.emit("backend:stderr", line);
      }
    });
  }

  {
    let mut g = st.lock().map_err(|_| "mutex poisoned".to_string())?;
    g.child = Some(child);
  }

  let _ = app.emit("backend:ready", "spawn ok");
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
  // 其他平台可按需实现 pkill，这里留空
}

pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_fs::init())
    .plugin(tauri_plugin_opener::init())
    .manage(BackendState(Arc::new(Mutex::new(BackendProcState::default()))))
    .invoke_handler(tauri::generate_handler![start_backend, stop_backend])
    .setup(|app| {
      let ah = app.handle().clone();
      let st = app.state::<BackendState>().0.clone();

      // 开发态默认不自启；如需临时自启：FORCE_AUTOSTART_BACKEND=1 npm run tauri:dev
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
        // release / 打包构建中仍然自启后端
        let _ = start_backend_with(ah.clone(), st.clone());
      }

      // 关闭窗口时收尾
      if let Some(win) = app.get_webview_window("main") {
        let st2 = st.clone();
        win.on_window_event(move |e| {
          if matches!(e, WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed) {
            let _ = stop_backend_with(st2.clone());
            kill_all_backends_silently();
          }
        });
      }

      Ok::<(), Box<dyn std::error::Error>>(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}