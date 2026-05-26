#[cfg(windows)]
mod imp {
  use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    sync::{
      atomic::{AtomicBool, Ordering},
      Arc, Mutex,
    },
    thread,
    time::Duration,
  };

  use serde::{Deserialize, Serialize};
  use tauri::{Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
  use windows::Win32::{
    Foundation::{CloseHandle, COLORREF, HWND, LPARAM, POINT, RECT},
    Graphics::Gdi::{
      ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ, HRGN,
      NULLREGION, RGN_DIFF, RGN_ERROR, RGN_OR,
    },
    System::{
      ProcessStatus::K32GetModuleBaseNameW,
      Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
    UI::{
      HiDpi::GetDpiForWindow,
      WindowsAndMessaging::{
        EnumWindows, GetClientRect, GetWindow, GetWindowLongW, GetWindowRect,
        GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
        SetLayeredWindowAttributes, SetWindowLongW, SetWindowPos, ShowWindow, GW_HWNDPREV,
        GWL_EXSTYLE, HWND_TOP, LWA_ALPHA, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
      },
    },
  };
  use windows::core::BOOL;

  const GAME_PROCESS_NAME: &str = "Jantama_MahjongSoul.exe";
  const ACTIVE_TRACK_INTERVAL: Duration = Duration::from_millis(8);
  const SEARCH_INTERVAL: Duration = Duration::from_millis(750);
  const HUD_PANEL_MAX_WIDTH: i32 = 620;
  const HUD_PANEL_MARGIN_X: i32 = 16;
  const HUD_PANEL_BOTTOM: i32 = 18;
  const HUD_PANEL_HEIGHT: i32 = 188;

  pub struct OverlayState {
    stop: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    status: Arc<Mutex<OverlayStatus>>,
    interactive: Arc<AtomicBool>,
    panel_regions: Arc<Mutex<Vec<PanelRegion>>>,
  }

  impl Drop for OverlayState {
    fn drop(&mut self) {
      self.stop.store(true, Ordering::SeqCst);
    }
  }

  #[derive(Clone, Copy, PartialEq, Eq)]
  struct WindowCandidate {
    hwnd: HWND,
    pid: u32,
  }

  #[derive(Clone, Serialize)]
  pub struct OverlayStatus {
    enabled: bool,
    supported: bool,
    found: bool,
    pid: Option<u32>,
  }

  #[derive(Clone, Copy, Deserialize)]
  pub struct PanelRegion {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
  }

  #[derive(Clone, Copy, PartialEq, Eq)]
  struct ClientArea {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    dpi: u32,
  }

  pub fn is_overlay_supported() -> bool {
    true
  }

  pub fn set_enabled(state: State<OverlayState>, enabled: bool) -> OverlayStatus {
    state.enabled.store(enabled, Ordering::SeqCst);
    if !enabled {
      if let Ok(mut status) = state.status.lock() {
        status.enabled = false;
        status.found = false;
        status.pid = None;
      }
    }
    get_status(state)
  }

  pub fn get_status(state: State<OverlayState>) -> OverlayStatus {
    state.status.lock().map(|status| status.clone()).unwrap_or(OverlayStatus {
      enabled: state.enabled.load(Ordering::SeqCst),
      supported: true,
      found: false,
      pid: None,
    })
  }

  pub fn set_interactive(state: State<OverlayState>, interactive: bool) {
    state.interactive.store(interactive, Ordering::SeqCst);
  }

  pub fn set_panel_regions(state: State<OverlayState>, regions: Vec<PanelRegion>) {
    if let Ok(mut current) = state.panel_regions.lock() {
      *current = regions
        .into_iter()
        .filter(|region| region.width > 0 && region.height > 0)
        .collect();
    }
  }

  pub fn start(app: &tauri::AppHandle) {
    let Some(overlay) = create_overlay_window(app) else {
      return;
    };

    let stop = Arc::new(AtomicBool::new(false));
    let enabled = Arc::new(AtomicBool::new(true));
    let interactive = Arc::new(AtomicBool::new(false));
    let panel_regions = Arc::new(Mutex::new(Vec::<PanelRegion>::new()));
    let status = Arc::new(Mutex::new(OverlayStatus {
      enabled: true,
      supported: true,
      found: false,
      pid: None,
    }));
    app.manage(OverlayState {
      stop: stop.clone(),
      enabled: enabled.clone(),
      status: status.clone(),
      interactive: interactive.clone(),
      panel_regions: panel_regions.clone(),
    });

    thread::spawn(move || {
      let hud_hwnd = match overlay.hwnd() {
        Ok(hwnd) => hwnd,
        Err(_) => return,
      };
      configure_overlay_hwnd(hud_hwnd);

      let mut current_game: Option<WindowCandidate> = None;
      let mut last_area: Option<ClientArea> = None;

      while !stop.load(Ordering::SeqCst) {
        if !enabled.load(Ordering::SeqCst) {
          hide(hud_hwnd);
          current_game = None;
          last_area = None;
          update_status(&status, false, None);
          thread::sleep(BACKGROUND_IDLE_INTERVAL);
          continue;
        }

        if current_game.map(|w| !is_live_overlay_target(w.hwnd)).unwrap_or(true) {
          current_game = find_game_window();
          last_area = None;
        }

        let Some(game) = current_game else {
          hide(hud_hwnd);
          update_status(&status, true, None);
          thread::sleep(SEARCH_INTERVAL);
          continue;
        };
        update_status(&status, true, Some(game.pid));

        match client_area(game.hwnd) {
          Some(area) => {
            let is_interactive = interactive.load(Ordering::SeqCst);
            configure_overlay_clickthrough(hud_hwnd, !is_interactive);
            let regions = panel_regions.lock().map(|regions| regions.clone()).unwrap_or_default();
            let visible_region = build_visible_region(game.hwnd, hud_hwnd, area, is_interactive, &regions);
            let Some(visible_region) = visible_region else {
              hide(hud_hwnd);
              update_status(&status, true, None);
              thread::sleep(ACTIVE_TRACK_INTERVAL);
              continue;
            };

            apply_window_region(hud_hwnd, visible_region);

            let z_after = z_order_insert_after(game.hwnd, hud_hwnd);
            let should_move = last_area != Some(area);
            if should_move {
              unsafe {
                let _ = SetWindowPos(
                  hud_hwnd,
                  Some(z_after),
                  area.x,
                  area.y,
                  area.width,
                  area.height,
                  SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
              }
            } else {
              unsafe {
                let _ = SetWindowPos(
                  hud_hwnd,
                  Some(z_after),
                  area.x,
                  area.y,
                  area.width,
                  area.height,
                  SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
              }
            }
            last_area = Some(area);
          }
          None => {
            hide(hud_hwnd);
            current_game = None;
            last_area = None;
            update_status(&status, true, None);
          }
        }

        thread::sleep(ACTIVE_TRACK_INTERVAL);
      }

      hide(hud_hwnd);
    });
  }

  const BACKGROUND_IDLE_INTERVAL: Duration = Duration::from_millis(250);

  fn create_overlay_window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    WebviewWindowBuilder::new(app, "mahjong-soul-hud", WebviewUrl::App("overlay.html".into()))
      .title("Shanten Lens HUD")
      .decorations(false)
      .transparent(true)
      .shadow(false)
      .resizable(false)
      .skip_taskbar(true)
      .focusable(false)
      .visible(false)
      .inner_size(1.0, 1.0)
      .build()
      .ok()
  }

  fn configure_overlay_hwnd(hwnd: HWND) {
    unsafe {
      configure_overlay_clickthrough(hwnd, true);
      let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
      let _ = ShowWindow(hwnd, SW_HIDE);
    }
  }

  fn configure_overlay_clickthrough(hwnd: HWND, clickthrough: bool) {
    unsafe {
      let mut style = GetWindowLongW(hwnd, GWL_EXSTYLE)
        | WS_EX_LAYERED.0 as i32
        | WS_EX_NOACTIVATE.0 as i32
        | WS_EX_TOOLWINDOW.0 as i32;
      if clickthrough {
        style |= WS_EX_TRANSPARENT.0 as i32;
      } else {
        style &= !(WS_EX_TRANSPARENT.0 as i32);
      }
      let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, style);
    }
  }

  fn hide(hwnd: HWND) {
    unsafe {
      let _ = ShowWindow(hwnd, SW_HIDE);
    }
  }

  fn z_order_insert_after(game_hwnd: HWND, hud_hwnd: HWND) -> HWND {
    match unsafe { GetWindow(game_hwnd, GW_HWNDPREV) } {
      Ok(hwnd) if hwnd != hud_hwnd => hwnd,
      _ => HWND_TOP,
    }
  }

  fn is_usable_game_window(hwnd: HWND) -> bool {
    if !is_live_overlay_target(hwnd) {
      return false;
    }

    process_name_for_window(hwnd).as_deref() == Some(GAME_PROCESS_NAME)
  }

  fn is_live_overlay_target(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
      return false;
    }

    unsafe {
      if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return false;
      }
    }

    match window_rect(hwnd) {
      Some(rect) => (rect.right - rect.left) >= 320 && (rect.bottom - rect.top) >= 240,
      None => false,
    }
  }

  fn find_game_window() -> Option<WindowCandidate> {
    struct SearchState {
      found: Option<WindowCandidate>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
      let state = &mut *(lparam.0 as *mut SearchState);
      if is_usable_game_window(hwnd) {
        state.found = window_pid(hwnd).map(|pid| WindowCandidate { hwnd, pid });
        return BOOL(0);
      }
      BOOL(1)
    }

    let mut state = SearchState { found: None };
    unsafe {
      let _ = EnumWindows(Some(enum_proc), LPARAM(&mut state as *mut SearchState as isize));
    }
    state.found
  }

  fn update_status(status: &Arc<Mutex<OverlayStatus>>, enabled: bool, pid: Option<u32>) {
    if let Ok(mut status) = status.lock() {
      status.enabled = enabled;
      status.supported = true;
      status.found = pid.is_some();
      status.pid = pid;
    }
  }

  fn process_name_for_window(hwnd: HWND) -> Option<String> {
    let pid = window_pid(hwnd)?;

    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) };
    let Ok(handle) = handle else {
      return None;
    };

    let mut buf = [0u16; 260];
    let len = unsafe { K32GetModuleBaseNameW(handle, None, &mut buf) };
    unsafe {
      let _ = CloseHandle(handle);
    }

    if len == 0 {
      return None;
    }

    Some(
      OsString::from_wide(&buf[..len as usize])
        .to_string_lossy()
        .to_string(),
    )
  }

  fn window_pid(hwnd: HWND) -> Option<u32> {
    let mut pid = 0u32;
    unsafe {
      GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    (pid != 0).then_some(pid)
  }

  fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    unsafe {
      if GetWindowRect(hwnd, &mut rect).is_ok() {
        Some(rect)
      } else {
        None
      }
    }
  }

  fn build_visible_region(game_hwnd: HWND, hud_hwnd: HWND, area: ClientArea, interactive: bool, panel_regions: &[PanelRegion]) -> Option<HRGN> {
    let visible = if interactive {
      build_panel_region(area, panel_regions)?
    } else {
      unsafe { CreateRectRgn(0, 0, area.width, area.height) }
    };
    if visible.is_invalid() {
      return None;
    }

    let mut cover = match unsafe { GetWindow(game_hwnd, GW_HWNDPREV) } {
      Ok(hwnd) => Some(hwnd),
      Err(_) => None,
    };

    while let Some(hwnd) = cover {
      if hwnd == hud_hwnd {
        cover = unsafe { GetWindow(hwnd, GW_HWNDPREV).ok() };
        continue;
      }

      if is_occluding_window(hwnd) {
        if let Some(rect) = window_rect(hwnd).and_then(|r| intersect_rect(r, area.screen_rect())) {
          let relative = RECT {
            left: rect.left - area.x,
            top: rect.top - area.y,
            right: rect.right - area.x,
            bottom: rect.bottom - area.y,
          };
          let blocker = unsafe {
            CreateRectRgn(relative.left, relative.top, relative.right, relative.bottom)
          };
          if !blocker.is_invalid() {
            let result = unsafe { CombineRgn(Some(visible), Some(visible), Some(blocker), RGN_DIFF) };
            unsafe {
              let _ = DeleteObject(HGDIOBJ::from(blocker));
            }
            if result == RGN_ERROR || result == NULLREGION {
              unsafe {
                let _ = DeleteObject(HGDIOBJ::from(visible));
              }
              return None;
            }
          }
        }
      }

      cover = unsafe { GetWindow(hwnd, GW_HWNDPREV).ok() };
    }

    Some(visible)
  }

  fn build_panel_region(area: ClientArea, panel_regions: &[PanelRegion]) -> Option<HRGN> {
    let base = unsafe { CreateRectRgn(0, 0, 0, 0) };
    if base.is_invalid() {
      return None;
    }

    let fallback = PanelRegion {
      x: ((area.width - (area.width - HUD_PANEL_MARGIN_X * 2).min(HUD_PANEL_MAX_WIDTH).max(1)) / 2).max(0),
      y: (area.height - HUD_PANEL_BOTTOM - HUD_PANEL_HEIGHT).max(0),
      width: (area.width - HUD_PANEL_MARGIN_X * 2).min(HUD_PANEL_MAX_WIDTH).max(1),
      height: HUD_PANEL_HEIGHT,
    };

    for region in if panel_regions.is_empty() { vec![fallback] } else { panel_regions.to_vec() } {
      let left = region.x.clamp(0, area.width);
      let top = region.y.clamp(0, area.height);
      let right = (region.x + region.width).clamp(0, area.width);
      let bottom = (region.y + region.height).clamp(0, area.height);
      if right <= left || bottom <= top {
        continue;
      }
      let part = unsafe { CreateRectRgn(left, top, right, bottom) };
      if part.is_invalid() {
        continue;
      }
      unsafe {
        let _ = CombineRgn(Some(base), Some(base), Some(part), RGN_OR);
        let _ = DeleteObject(HGDIOBJ::from(part));
      }
    }

    Some(base)
  }

  fn apply_window_region(hwnd: HWND, region: HRGN) {
    unsafe {
      if SetWindowRgn(hwnd, Some(region), true) == 0 {
        let _ = DeleteObject(HGDIOBJ::from(region));
      }
    }
  }

  fn is_occluding_window(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
      return false;
    }

    unsafe {
      if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return false;
      }
    }

    match window_rect(hwnd) {
      Some(rect) => (rect.right - rect.left) >= 8 && (rect.bottom - rect.top) >= 8,
      None => false,
    }
  }

  fn intersect_rect(a: RECT, b: RECT) -> Option<RECT> {
    let left = a.left.max(b.left);
    let top = a.top.max(b.top);
    let right = a.right.min(b.right);
    let bottom = a.bottom.min(b.bottom);
    if right <= left || bottom <= top {
      return None;
    }
    Some(RECT { left, top, right, bottom })
  }

  fn client_area(hwnd: HWND) -> Option<ClientArea> {
    let mut rect = RECT::default();
    unsafe {
      if GetClientRect(hwnd, &mut rect).is_err() {
        return None;
      }
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width < 320 || height < 240 {
      return None;
    }

    let mut point = POINT { x: 0, y: 0 };
    unsafe {
      if !ClientToScreen(hwnd, &mut point).as_bool() {
        return None;
      }
    }

    Some(ClientArea {
      x: point.x,
      y: point.y,
      width,
      height,
      dpi: unsafe { GetDpiForWindow(hwnd) },
    })
  }

  impl ClientArea {
    fn screen_rect(self) -> RECT {
      RECT {
        left: self.x,
        top: self.y,
        right: self.x + self.width,
        bottom: self.y + self.height,
      }
    }
  }
}

#[cfg(not(windows))]
mod imp {
  use serde::Serialize;
  use serde::Deserialize;

  #[derive(Clone, Serialize)]
  pub struct OverlayStatus {
    pub enabled: bool,
    pub supported: bool,
    pub found: bool,
    pub pid: Option<u32>,
  }

  #[derive(Clone, Copy, Deserialize)]
  pub struct PanelRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
  }

  pub fn is_overlay_supported() -> bool {
    false
  }

  pub fn start(_app: &tauri::AppHandle) {}

  pub fn set_enabled(_enabled: bool) -> OverlayStatus {
    OverlayStatus {
      enabled: false,
      supported: false,
      found: false,
      pid: None,
    }
  }

  pub fn get_status() -> OverlayStatus {
    OverlayStatus {
      enabled: false,
      supported: false,
      found: false,
      pid: None,
    }
  }

  pub fn set_interactive(_interactive: bool) {}

  pub fn set_panel_regions(_regions: Vec<PanelRegion>) {}
}

#[cfg(windows)]
pub use imp::OverlayState;
pub use imp::{get_status, is_overlay_supported, set_enabled, set_interactive, set_panel_regions, start, OverlayStatus, PanelRegion};
