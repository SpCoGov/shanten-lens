from typing import List, Callable, Optional, Tuple, Dict

import cv2
import mss
import numpy as np
import pyautogui
import time
from loguru import logger

from backend.bot.drivers.click.clicker import Clicker
from backend.bot.config import BotConfig
from backend.bot.drivers.click.roi import slot_centers_by_bbox
from backend.bot.drivers.click.window import (
    find_window_by_keyword,
    get_client_rect_screen,
    focus_window,
    is_foreground,   # 新增：用来判断前台
)
from backend.bot.logic.handmap import screen_slot_indices_from_ids, choose_discard_slot_by_id


def button_centers_by_order(
        bbox: Tuple[int, int, int, int],
        present_ops: List[int],
        order: List[int],
        margin_ratio: float = 0.06
) -> Dict[int, Tuple[int, int]]:
    x1, y1, x2, y2 = bbox
    w = x2 - x1
    pad = int(round(w * margin_ratio))
    ix1, ix2 = x1 + pad, x2 - pad
    ys = (y1 + y2) // 2
    ops = [op for op in order if op in set(present_ops)]
    n = max(1, len(ops))
    if n == 1:
        xs = [(ix1 + ix2) // 2]
    else:
        xs = [int(round(ix1 + (ix2 - ix1) * (i / (n - 1)))) for i in range(n)]
    return {op: (xs[i], ys) for i, op in enumerate(ops)}


class BotPipeline:
    def __init__(self, cfg: BotConfig):
        self.cfg = cfg
        self.clicker = Clicker(cfg.ack_timeout_sec, cfg.ack_retry, cfg.ack_settle_ms, cfg.ack_check_ms)
        self._viewport: Optional[Tuple[int, int, int, int]] = None
        self._hwnd = None

    def bind_window(self, keyword: Optional[str] = None) -> bool:
        kw = keyword or self.cfg.window_title_keyword
        if not kw:
            return False
        hwnd, _title = find_window_by_keyword(kw)
        if not hwnd:
            return False
        vp = get_client_rect_screen(hwnd)
        if not vp:
            return False
        self._viewport = vp
        self._hwnd = hwnd
        return True

    def ensure_bound(self, keyword: str | None = None) -> bool:
        if self._viewport is None:
            return self.bind_window(keyword or self.cfg.window_title_keyword)
        return True

    def refresh_viewport(self) -> bool:
        if not self._hwnd:
            return False
        vp = get_client_rect_screen(self._hwnd)
        if not vp:
            return False
        self._viewport = vp
        return True

    def _bbox_from_norm(self, norm_rect: Tuple[float, float, float, float]) -> Tuple[int, int, int, int]:
        if self._viewport:
            left, top, w, h = self._viewport
        else:
            left, top, w, h = 0, 0, self.cfg.screen_width, self.cfg.screen_height
        x1 = int(round(left + norm_rect[0] * w))
        y1 = int(round(top + norm_rect[1] * h))
        x2 = int(round(left + norm_rect[2] * w))
        y2 = int(round(top + norm_rect[3] * h))
        return x1, y1, x2, y2

    def _grab(self):
        with mss.mss() as sct:
            if self._viewport:
                left, top, w, h = self._viewport
                raw = sct.grab({"left": left, "top": top, "width": w, "height": h})
                img = np.array(raw)
                return cv2.cvtColor(img, cv2.COLOR_BGRA2BGR), (left, top)
            else:
                mon = sct.monitors[0]
                raw = sct.grab(mon)
                img = np.array(raw)
                return cv2.cvtColor(img, cv2.COLOR_BGRA2BGR), (0, 0)

    def _map16x9(self, x16: float, y9: float) -> tuple[int, int]:
        if self._viewport:
            left, top, w, h = self._viewport
            scale = min(w / 16.0, h / 9.0)
            ox = left + (w - scale * 16) / 2.0
            oy = top + (h - scale * 9) / 2.0
            return int(round(ox + x16 * scale)), int(round(oy + y9 * scale))
        scale = min(self.cfg.screen_width / 16.0, self.cfg.screen_height / 9.0)
        ox = (self.cfg.screen_width - scale * 16) / 2.0
        oy = (self.cfg.screen_height - scale * 9) / 2.0
        return int(round(ox + x16 * scale)), int(round(oy + y9 * scale))

    def _button_bar_bbox(self) -> tuple[int, int, int, int]:
        if self.cfg.button_bar_norm:
            return self._bbox_from_norm(self.cfg.button_bar_norm)
        x1, y_mid = self._map16x9(self.cfg.btn_x_left, self.cfg.btn_y_line)
        x2, _ = self._map16x9(self.cfg.btn_x_right, self.cfg.btn_y_line)
        y1 = y_mid - 40
        y2 = y_mid + 40
        return x1, y1, x2, y2

    def _hand_bar_bbox(self) -> tuple[int, int, int, int]:
        if self.cfg.hand_bar_norm:
            return self._bbox_from_norm(self.cfg.hand_bar_norm)
        x1, y2 = self._map16x9(2.0, 8.7)
        x2, _ = self._map16x9(14.0, 8.7)
        y1 = y2 - 150
        return x1, y1, x2, y2

    def _viewport_center(self) -> Tuple[int, int]:
        if self._viewport:
            left, top, w, h = self._viewport
            cx = left + w // 2
            cy = top + int(h * 0.55)
            return cx, cy
        return self._map16x9(8.0, 5.0)

    def selftest_move(
            self,
            present_ops: List[int] = None,
            hand_ids_with_draw: List[int] = None,
            id2label: Dict[int, str] = None,
            step_delay: float = 0.25,
            hover_ms: int = 150,
    ):
        if not self.ensure_bound():
            logger.error("未绑定窗口。请先 pipeline.bind_window('雀魂')。")
            return
        self.refresh_viewport()

        left, top, w, h = self._viewport
        logger.info(f"viewport: left={left}, top={top}, w={w}, h={h}")

        def _pt(x, y, tag=""):
            logger.info(f" -> {tag} @ ({x},{y})")
            pyautogui.moveTo(x, y, duration=step_delay)
            time.sleep(hover_ms / 1000.0)

        _pt(left + 10, top + 10, "corner-UL")
        _pt(left + w - 10, top + 10, "corner-UR")
        _pt(left + w - 10, top + h - 10, "corner-DR")
        _pt(left + 10, top + h - 10, "corner-DL")
        cx_mid, cy_mid = self._viewport_center()
        _pt(cx_mid, cy_mid, "viewport-center(~55%)")

        bx1, by1, bx2, by2 = self._button_bar_bbox()
        logger.info(f"button_bar_bbox = {(bx1, by1, bx2, by2)}")
        if present_ops:
            if self.cfg.button_order:
                centers = button_centers_by_order(
                    bbox=(bx1, by1, bx2, by2),
                    present_ops=present_ops,
                    order=self.cfg.button_order,
                    margin_ratio=self.cfg.button_margin
                )
            else:
                wbar = bx2 - bx1
                xs = [bx1 + int(wbar * 0.0), bx1 + int(wbar * 0.5), bx1 + int(wbar * 1.0)]
                ys = (by1 + by2) // 2
                centers = {}
                for i, op in enumerate(sorted(set(present_ops))[:3]):
                    centers[op] = (xs[i], ys)
            for op, (cx, cy) in centers.items():
                _pt(cx, cy, f"button op={op}")

        hx1, hy1, hx2, hy2 = self._hand_bar_bbox()
        logger.info(f"hand_bar_bbox   = {(hx1, hy1, hx2, hy2)}")
        centers = slot_centers_by_bbox(
            (hx1, hy1, hx2, hy2),
            n_slots=self.cfg.hand_slots,
            margin_ratio=self.cfg.hand_margin,
            left_comp_px=self.cfg.hand_left_comp_px,
            right_comp_px=self.cfg.hand_right_comp_px
        )

        screen_slot_map = None
        if hand_ids_with_draw and id2label:
            try:
                slots = screen_slot_indices_from_ids(hand_ids_with_draw, id2label)
                screen_slot_map = {}
                for idx_input, slot in enumerate(slots):
                    tid = hand_ids_with_draw[idx_input]
                    lab = id2label.get(tid, "??")
                    screen_slot_map.setdefault(slot, []).append((tid, lab))
            except Exception as e:
                logger.warning(f"计算手牌槽位映射失败: {e}")

        for i, (cx, cy) in enumerate(centers):
            if screen_slot_map and i in screen_slot_map:
                info = " / ".join(f"{tid}:{lab}" for tid, lab in screen_slot_map[i])
                tag = f"hand slot#{i} [{info}]"
            else:
                tag = f"hand slot#{i}"
            _pt(cx, cy, tag)

    def _blind_close_popups(self):
        if self._viewport:
            left, top, w, h = self._viewport
            cx_ok = left + w // 2
            cy_ok = top + int(h * 0.72)
            pyautogui.moveTo(cx_ok, cy_ok, duration=0)
            pyautogui.mouseDown(); pyautogui.mouseUp()
            time.sleep(0.12)
            cx_x = left + w - 20
            cy_x = top + 20
            pyautogui.moveTo(cx_x, cy_x, duration=0)
            pyautogui.mouseDown(); pyautogui.mouseUp()
            time.sleep(0.12)
        else:
            cx_ok, cy_ok = self._map16x9(8.0, 6.5)
            pyautogui.moveTo(cx_ok, cy_ok, duration=0)
            pyautogui.mouseDown(); pyautogui.mouseUp()
            time.sleep(0.12)
            cx_x, cy_x = self._map16x9(15.5, 1.0)
            pyautogui.moveTo(cx_x, cy_x, duration=0)
            pyautogui.mouseDown(); pyautogui.mouseUp()
            time.sleep(0.12)

    def click_op(
            self,
            present_ops: List[int],
            target_op: int,
            state_ok_pred: Optional[Callable[[], bool]] = None,
            on_retry: Optional[Callable[[], None]] = None
    ) -> bool:
        if not self.ensure_bound():
            return False
        self.refresh_viewport()
        if on_retry is None:
            on_retry = self._blind_close_popups

        bbox = self._button_bar_bbox()
        centers: Dict[int, Tuple[int, int]] = {}
        if self.cfg.button_order:
            centers = button_centers_by_order(
                bbox=bbox,
                present_ops=present_ops,
                order=self.cfg.button_order,
                margin_ratio=self.cfg.button_margin
            )
        if target_op not in centers:
            return False

        cx, cy = centers[target_op]

        def do_click():
            # 常态不抢前台；若被遮挡会进入重试
            self.clicker.click_xy(cx, cy)

        def ok_pred():
            return True if state_ok_pred is None else state_ok_pred()

        def _retry():
            # 失败了再把窗口拉到前台，再执行原 on_retry（如关弹窗）
            if self._hwnd:
                try:
                    focus_window(self._hwnd, viewport=self._viewport)
                    time.sleep(0.02)
                except Exception:
                    pass
            on_retry()

        return self.clicker.click_with_ack(do_click, ok_pred, _retry)

    def _drag_tile_to_center(self, from_x: int, from_y: int):
        import pyautogui, time as _t, numpy as _np

        to_x, to_y = self._viewport_center()

        _, _, w, h = self._viewport if self._viewport else (0, 0, self.cfg.screen_width, self.cfg.screen_height)
        lift_px = max(6, int(h * 0.008))
        jitter = max(1, int(h * 0.0015))
        jx = _np.random.randint(-jitter, jitter + 1)
        jy = _np.random.randint(-jitter, jitter + 1)

        t_move_in = 0.03
        t_down_wait = 0.02
        t_lift = 0.04
        t_line = 0.10
        t_end_hold = 0.02
        t_after = 0.01

        pyautogui.moveTo(from_x, from_y, duration=t_move_in)
        pyautogui.mouseDown()
        _t.sleep(t_down_wait)
        pyautogui.moveTo(from_x, from_y - lift_px, duration=t_lift)
        pyautogui.moveTo(to_x + jx, to_y + jy, duration=t_line)
        _t.sleep(t_end_hold)
        pyautogui.mouseUp()
        _t.sleep(t_after)

    def click_discard_by_index(self, slot_idx: int, n_slots: Optional[int] = None) -> bool:
        if not self.ensure_bound():
            return False
        self.refresh_viewport()

        bbox = self._hand_bar_bbox()
        n = n_slots or self.cfg.hand_slots
        centers = slot_centers_by_bbox(
            bbox,
            n_slots=n,
            margin_ratio=self.cfg.hand_margin,
            left_comp_px=self.cfg.hand_left_comp_px,
            right_comp_px=self.cfg.hand_right_comp_px
        )
        if not (0 <= slot_idx < len(centers)):
            return False
        cx, cy = centers[slot_idx]

        def do_click():
            self._drag_tile_to_center(cx, cy)

        def ok_pred():
            return True

        def _retry():
            if self._hwnd:
                try:
                    focus_window(self._hwnd, viewport=self._viewport)  # 内部是 ShowWindow+ASFW_ANY+SFW
                    time.sleep(0.03)  # 让合成器/命中测试稳定一帧
                except Exception:
                    pass
            self._blind_close_popups()

        return self.clicker.click_with_ack(do_click, ok_pred, _retry)

    def _viewport_left_center(self) -> Tuple[int, int]:
        if self._viewport:
            left, top, w, h = self._viewport
            cx = left + int(w * 0.25)
            cy = top + int(h * 0.55)
            return cx, cy
        return self._map16x9(4.0, 5.0)

    def click_left_center_once(self) -> bool:
        if not self.ensure_bound():
            if not self.bind_window(self.cfg.window_title_keyword):
                return False
        self.refresh_viewport()
        cx, cy = self._viewport_left_center()
        # 一次性 anti-AFK：仅在不在前台时，才尝试轻量拉前台
        if self._hwnd and not is_foreground(self._hwnd):
            try:
                focus_window(self._hwnd, viewport=self._viewport)
                time.sleep(0.02)
            except Exception:
                pass
        try:
            self.clicker.click_xy(cx, cy)
            return True
        except Exception as e:
            logger.warning(f"anti-AFK click failed: {e}")
            return False

    def _viewport_left_edge_nudged(self, nudged_ratio: float = 0.03) -> Tuple[int, int]:
        if self._viewport:
            left, top, w, h = self._viewport
            cx = left + int(w * max(0.0, min(nudged_ratio, 0.25)))
            cy = top + int(h * 0.55)
            return cx, cy
        return self._map16x9(0.5, 5.0)

    def click_left_edge_nudged_once(self, nudged_ratio: float = 0.03) -> bool:
        if not self.ensure_bound():
            if not self.bind_window(self.cfg.window_title_keyword):
                logger.debug("anti-AFK: bind window failed (edge-nudged)")
                return False
        self.refresh_viewport()
        cx, cy = self._viewport_left_edge_nudged(nudged_ratio)
        if self._hwnd and not is_foreground(self._hwnd):
            try:
                focus_window(self._hwnd, viewport=self._viewport)
                time.sleep(0.02)
            except Exception:
                pass
        try:
            self.clicker.click_xy(cx, cy)
            logger.debug(f"anti-AFK clicked at LEFT-EDGE-NUDGED ({cx},{cy}) ratio={nudged_ratio}")
            return True
        except Exception as e:
            logger.warning(f"anti-AFK edge-nudged click failed: {e}")
            return False

    def click_discard_by_tile_id(
            self,
            tile_id: int,
            hand_ids_with_draw: List[int],
            id2label: Dict[int, str],
            allow_tsumogiri: bool = False
    ) -> bool:
        if not self.ensure_bound():
            logger.error("未绑定窗口")
            return False
        self.refresh_viewport()

        slot = choose_discard_slot_by_id(
            hand_ids_with_draw=hand_ids_with_draw,
            target_id=tile_id,
            id2label=id2label,
            allow_tsumogiri=allow_tsumogiri
        )
        logger.info(f"choose_discard_slot_by_id -> slot={slot} "
                    f"(allow_tsumogiri={allow_tsumogiri})")

        last_idx = len(hand_ids_with_draw) - 1
        is_last = (last_idx >= 0 and hand_ids_with_draw[last_idx] == tile_id)
        if slot < 0 and is_last:
            slot = last_idx
            logger.info(f"fallback: target is last-draw -> force slot={slot}")

        if slot < 0:
            try:
                screen_slots = screen_slot_indices_from_ids(hand_ids_with_draw, id2label)
                candidates = [screen_slots[i] for i, tid in enumerate(hand_ids_with_draw) if tid == tile_id]
                if candidates:
                    slot = candidates[-1]
                    logger.info(f"fallback: screen map -> slot={slot} (candidates={candidates})")
            except Exception as e:
                logger.info(f"fallback: screen map failed: {e}")

        if slot < 0:
            logger.error(f"tile_id={tile_id} 无法定位槽位。"
                         f"hand_len={len(hand_ids_with_draw)}, last={hand_ids_with_draw[-1] if hand_ids_with_draw else None}")
            return False

        hx1, hy1, hx2, hy2 = self._hand_bar_bbox()
        centers = slot_centers_by_bbox(
            (hx1, hy1, hx2, hy2),
            n_slots=len(hand_ids_with_draw),
            margin_ratio=self.cfg.hand_margin,
            left_comp_px=self.cfg.hand_left_comp_px,
            right_comp_px=self.cfg.hand_right_comp_px
        )
        if not (0 <= slot < len(centers)):
            logger.error(f"computed slot out of range: slot={slot}, centers_len={len(centers)}")
            return False

        cx, cy = centers[slot]
        label = id2label.get(tile_id, "??")
        logger.info(f"OK: tile_id={tile_id} label={label} -> slot={slot} pos=({cx},{cy}) "
                    f"bbox={(hx1, hy1, hx2, hy2)}")

        def do_click():
            self._drag_tile_to_center(cx, cy)

        def ok_pred():
            return (self._hwnd is not None) and is_foreground(self._hwnd)

        def _retry():
            if self._hwnd:
                try:
                    focus_window(self._hwnd, viewport=self._viewport)
                    time.sleep(0.01)
                except Exception:
                    pass
            self._blind_close_popups()

        return self.clicker.click_with_ack(do_click, ok_pred, _retry)
