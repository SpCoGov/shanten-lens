import time

import pyautogui

from .vision import match_icon


def try_close(scene_bgr, tpl_ok: str, tpl_close: str, thresh: float):
    for tpl in (tpl_ok, tpl_close):
        if not tpl:
            continue
        m = match_icon(scene_bgr, tpl, thresh)
        if m:
            x1, y1, x2, y2, _ = m
            cx, cy = (x1 + x2) // 2, (y1 + y2) // 2
            pyautogui.moveTo(cx, cy, 0)
            pyautogui.mouseDown()
            pyautogui.mouseUp()
            time.sleep(0.15)
            return True
    return False
