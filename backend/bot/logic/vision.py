import os

import cv2


def match_icon(roi_bgr, icon_path: str, thresh: float):
    if roi_bgr is None or roi_bgr.size == 0: return None
    if not icon_path or not os.path.exists(icon_path): return None
    ico = cv2.imread(icon_path, cv2.IMREAD_UNCHANGED)
    if ico is None: return None
    ico = ico[:, :, :3] if ico.shape[2] == 4 else ico
    roi_g = cv2.cvtColor(roi_bgr, cv2.COLOR_BGR2GRAY)
    ico_g = cv2.cvtColor(ico, cv2.COLOR_BGR2GRAY)
    res = cv2.matchTemplate(roi_g, ico_g, cv2.TM_CCOEFF_NORMED)
    _minV, maxV, _minL, maxL = cv2.minMaxLoc(res)
    if maxV < thresh: return None
    h, w = ico_g.shape
    x1, y1 = maxL
    return x1, y1, x1 + w, y1 + h, maxV
