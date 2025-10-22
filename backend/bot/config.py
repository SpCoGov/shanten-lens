from dataclasses import dataclass
from typing import Optional, Tuple, List


@dataclass
class BotConfig:
    screen_width: int
    screen_height: int

    window_title_keyword: Optional[str] = "雀魂"

    hand_bar_norm: Optional[Tuple[float, float, float, float]] = None
    button_bar_norm: Optional[Tuple[float, float, float, float]] = None

    hand_slots: int = 14  # 槽位数
    hand_margin: float = 0.02  # 左右内边距占比（相对 hand_bar 宽度）
    hand_left_comp_px: int = 0  # 左侧微调像素
    hand_right_comp_px: int = 0  # 右侧微调像素

    button_order: Optional[List[int]] = None  # 例如 [4, 8, 100, 101]
    button_margin: float = 0.06  # 左右内边距占比（相对 button_bar 宽度）

    btn_x_left: float = 6.40
    btn_x_right: float = 10.875
    btn_y_line: float = 6.45

    KAN: int = 3
    TSUMO: int = 8
    SKIP: int = 100

    # 确认/重试
    ack_timeout_sec: float = 1.6
    ack_retry: int = 2
    ack_settle_ms: int = 140
    ack_check_ms: int = 70
