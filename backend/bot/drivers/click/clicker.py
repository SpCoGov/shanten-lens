import time
import pyautogui

pyautogui.FAILSAFE = False
pyautogui.PAUSE = 0.0


class Clicker:
    def __init__(self, timeout_sec: float, retry: int, settle_ms: int, check_ms: int):
        self.timeout_sec = timeout_sec
        self.retry = retry
        self.settle_ms = settle_ms
        self.check_ms = check_ms

    @staticmethod
    def click_xy(x: int, y: int, hold: float = 0.06):
        pyautogui.moveTo(x, y, duration=0)
        pyautogui.mouseDown()
        time.sleep(hold)
        pyautogui.mouseUp()

    @staticmethod
    def double_click_xy(x: int, y: int, gap: float = 0.06, hold: float = 0.03):
        pyautogui.moveTo(x, y, duration=0)
        pyautogui.mouseDown()
        time.sleep(hold)
        pyautogui.mouseUp()
        time.sleep(gap)
        pyautogui.mouseDown()
        time.sleep(hold)
        pyautogui.mouseUp()

    def click_with_ack(self, do_click, ok_pred, on_retry=None) -> bool:
        """
        带“确认/重试”的执行器：
        - do_click(): 执行动作
        - ok_pred(): 轮询确认是否成功（True=成功）
        - on_retry(): 失败后可选的修复/清障回调
        """
        for _ in range(self.retry + 1):
            do_click()
            time.sleep(self.settle_ms / 1000.0)
            t0 = time.time()
            while time.time() - t0 < self.timeout_sec:
                if ok_pred():
                    return True
                time.sleep(self.check_ms / 1000.0)
            if on_retry:
                on_retry()
        return False
