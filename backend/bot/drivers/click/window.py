import ctypes
from ctypes import wintypes

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

HWND = wintypes.HWND
LPARAM = wintypes.LPARAM
BOOL = wintypes.BOOL
LONG = wintypes.LONG


class POINT(ctypes.Structure):
    _fields_ = [("x", LONG), ("y", LONG)]


class RECT(ctypes.Structure):
    _fields_ = [("left", LONG), ("top", LONG), ("right", LONG), ("bottom", LONG)]


EnumWindows = user32.EnumWindows
EnumWindowsProc = ctypes.WINFUNCTYPE(BOOL, HWND, LPARAM)
GetWindowTextW = user32.GetWindowTextW
GetWindowTextLengthW = user32.GetWindowTextLengthW
IsWindowVisible = user32.IsWindowVisible
GetClientRect = user32.GetClientRect
ClientToScreen = user32.ClientToScreen

GetForegroundWindow = user32.GetForegroundWindow
SetForegroundWindow = user32.SetForegroundWindow
SetActiveWindow = user32.SetActiveWindow
ShowWindow = user32.ShowWindow
SetWindowPos = user32.SetWindowPos
AttachThreadInput = user32.AttachThreadInput
GetWindowThreadProcessId = user32.GetWindowThreadProcessId
GetCurrentThreadId = kernel32.GetCurrentThreadId
BringWindowToTop = user32.BringWindowToTop
keybd_event = user32.keybd_event

SW_RESTORE = 9
SW_SHOW = 5
HWND_TOPMOST = -1
HWND_NOTOPMOST = -2
SWP_NOSIZE = 0x0001
SWP_NOMOVE = 0x0002
SWP_SHOWWINDOW = 0x0040
VK_MENU = 0x12  # Alt


def _get_title(hwnd: int) -> str:
    length = GetWindowTextLengthW(hwnd)
    if length <= 0:
        return ""
    buf = ctypes.create_unicode_buffer(length + 1)
    GetWindowTextW(hwnd, buf, length + 1)
    return buf.value


def find_window_by_keyword(keyword: str):
    result = {"hwnd": None, "title": ""}
    kw = (keyword or "").lower()

    def _enum_proc(hwnd, _lparam):
        if not IsWindowVisible(hwnd):
            return True
        title = _get_title(hwnd)
        if not title:
            return True
        if kw in title.lower():
            result["hwnd"] = hwnd
            result["title"] = title
            return False  # stop enum
        return True

    EnumWindows(EnumWindowsProc(_enum_proc), 0)
    return result["hwnd"], result["title"]


def get_client_rect_screen(hwnd: int):
    rect = RECT()
    if not GetClientRect(hwnd, ctypes.byref(rect)):
        return None
    pt = POINT(0, 0)
    if not ClientToScreen(hwnd, ctypes.byref(pt)):
        return None
    left = pt.x
    top = pt.y
    width = rect.right - rect.left
    height = rect.bottom - rect.top
    return left, top, width, height


def is_foreground(hwnd) -> bool:
    return GetForegroundWindow() == hwnd


def _alt_nudge():
    keybd_event(VK_MENU, 0, 0, 0)
    keybd_event(VK_MENU, 0, 2, 0)


def ensure_focus(hwnd: int, viewport=None, click_fallback: bool = False) -> bool:
    ShowWindow(hwnd, SW_RESTORE)
    ShowWindow(hwnd, SW_SHOW)
    _alt_nudge()

    cur_tid = GetCurrentThreadId()
    tgt_tid = GetWindowThreadProcessId(hwnd, None)
    attached = AttachThreadInput(cur_tid, tgt_tid, True)
    try:
        BringWindowToTop(hwnd)
        SetForegroundWindow(hwnd)
        SetActiveWindow(hwnd)
    finally:
        if attached:
            AttachThreadInput(cur_tid, tgt_tid, False)

    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW)
    SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW)

    if is_foreground(hwnd):
        return True

    if click_fallback and viewport:
        try:
            import pyautogui, time as _t
            left, top, w, h = viewport
            pyautogui.moveTo(left + 10, top + 10, duration=0)
            pyautogui.mouseDown()
            pyautogui.mouseUp()
            _t.sleep(0.02)
        except Exception:
            pass
        _alt_nudge()
        BringWindowToTop(hwnd)
        SetForegroundWindow(hwnd)
        SetActiveWindow(hwnd)

    return is_foreground(hwnd)


def focus_window(hwnd: int, viewport=None) -> bool:
    """便捷包装：带兜底点击"""
    return ensure_focus(hwnd, viewport=viewport, click_fallback=True)
