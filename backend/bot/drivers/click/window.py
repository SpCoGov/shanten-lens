import ctypes
from ctypes import wintypes
from typing import Optional, Tuple

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

HWND = wintypes.HWND
LPARAM = wintypes.LPARAM
BOOL = wintypes.BOOL
LONG = wintypes.LONG
UINT = wintypes.UINT
DWORD = wintypes.DWORD


class POINT(ctypes.Structure):
    _fields_ = [("x", LONG), ("y", LONG)]


class RECT(ctypes.Structure):
    _fields_ = [("left", LONG), ("top", LONG), ("right", LONG), ("bottom", LONG)]


EnumWindows = user32.EnumWindows
EnumWindows.argtypes = [ctypes.WINFUNCTYPE(BOOL, HWND, LPARAM), LPARAM]
EnumWindows.restype = BOOL
EnumWindowsProc = ctypes.WINFUNCTYPE(BOOL, HWND, LPARAM)

GetWindowTextW = user32.GetWindowTextW
GetWindowTextW.argtypes = [HWND, wintypes.LPWSTR, ctypes.c_int]
GetWindowTextW.restype = ctypes.c_int

GetWindowTextLengthW = user32.GetWindowTextLengthW
GetWindowTextLengthW.argtypes = [HWND]
GetWindowTextLengthW.restype = ctypes.c_int

IsWindowVisible = user32.IsWindowVisible
IsWindowVisible.argtypes = [HWND]
IsWindowVisible.restype = BOOL

GetClientRect = user32.GetClientRect
GetClientRect.argtypes = [HWND, ctypes.POINTER(RECT)]
GetClientRect.restype = BOOL

ClientToScreen = user32.ClientToScreen
ClientToScreen.argtypes = [HWND, ctypes.POINTER(POINT)]
ClientToScreen.restype = BOOL

GetForegroundWindow = user32.GetForegroundWindow
GetForegroundWindow.argtypes = []
GetForegroundWindow.restype = HWND

ShowWindow = user32.ShowWindow
ShowWindow.argtypes = [HWND, ctypes.c_int]
ShowWindow.restype = BOOL

SetForegroundWindow = user32.SetForegroundWindow
SetForegroundWindow.argtypes = [HWND]
SetForegroundWindow.restype = BOOL

AllowSetForegroundWindow = user32.AllowSetForegroundWindow
AllowSetForegroundWindow.argtypes = [DWORD]
AllowSetForegroundWindow.restype = BOOL

SW_RESTORE = 9
ASFW_ANY = 0xFFFFFFFF


def _get_title(hwnd: int) -> str:
    length = GetWindowTextLengthW(HWND(hwnd))
    if length <= 0:
        return ""
    buf = ctypes.create_unicode_buffer(length + 1)
    GetWindowTextW(HWND(hwnd), buf, length + 1)
    return buf.value


def find_window_by_keyword(keyword: str) -> Tuple[Optional[int], str]:
    result = {"hwnd": None, "title": ""}
    kw = (keyword or "").lower()

    @EnumWindowsProc
    def _enum_proc(hwnd, _lparam):
        try:
            if not IsWindowVisible(hwnd):
                return True
            title = _get_title(hwnd)
            if not title:
                return True
            if kw in title.lower():
                result["hwnd"] = int(hwnd)
                result["title"] = title
                return False  # stop
            return True
        except Exception:
            return True

    EnumWindows(_enum_proc, 0)
    return result["hwnd"], result["title"]


def get_client_rect_screen(hwnd: int) -> Optional[Tuple[int, int, int, int]]:
    rect = RECT()
    if not GetClientRect(HWND(hwnd), ctypes.byref(rect)):
        return None
    pt = POINT(0, 0)
    if not ClientToScreen(HWND(hwnd), ctypes.byref(pt)):
        return None
    left = pt.x
    top = pt.y
    width = rect.right - rect.left
    height = rect.bottom - rect.top
    return left, top, width, height


def is_foreground(hwnd: int) -> bool:
    return int(GetForegroundWindow() or 0) == int(hwnd)


_last_focus = {"ts": 0.0, "hwnd": 0}


def ensure_focus(hwnd: int, *, min_interval_sec: float = 1.2, restore_before=True) -> bool:
    import time as _t
    if not hwnd:
        return False

    if is_foreground(hwnd):
        return True

    now = _t.time()
    if _last_focus["hwnd"] == hwnd and (now - _last_focus["ts"] < min_interval_sec):
        return False

    if restore_before:
        try:
            ShowWindow(HWND(hwnd), SW_RESTORE)
        except Exception:
            pass

    try:
        AllowSetForegroundWindow(ASFW_ANY)
    except Exception:
        pass

    ok = bool(SetForegroundWindow(HWND(hwnd)))

    if ok and is_foreground(hwnd):
        _last_focus["hwnd"] = hwnd
        _last_focus["ts"] = _t.time()
        return True

    _t.sleep(0.03)
    if is_foreground(hwnd):
        _last_focus["hwnd"] = hwnd
        _last_focus["ts"] = _t.time()
        return True

    return False


def focus_window(hwnd: int, viewport=None) -> bool:
    return ensure_focus(hwnd)
