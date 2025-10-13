from __future__ import annotations
from pathlib import Path
from typing import Dict, Any, Optional, Tuple
import json

from .items import ConfigItem


class ConfigTable:
    def __init__(self, name: str, file: Path):
        self.name = name
        self.file = Path(file)
        self.items: Dict[str, ConfigItem] = {}

    # Builder：集中定义默认项
    def add(self, key: str, default: Any, *, desc: str | None = None, kind: str | None = None) -> "ConfigTable":
        self.items[key] = ConfigItem(name=key, default=default, desc=desc, kind=kind)
        return self

    # 读/写
    def get(self, key: str, default: Any = None) -> Any:
        it = self.items.get(key)
        return default if it is None else it.effective

    def set(self, key: str, value: Any) -> None:
        if key not in self.items:
            # 未注册项：默认允许动态添加（可改为抛错）
            self.items[key] = ConfigItem(name=key, default=None, value=value)
        else:
            self.items[key].set(value)

    def to_values_dict(self) -> Dict[str, Any]:
        return {k: it.effective for k, it in self.items.items()}

    # 磁盘交互
    def load_merge(self) -> Tuple[bool, bool]:
        """合并磁盘 -> 内存；返回 (是否值变化, 是否需要写回补齐)"""
        changed = False
        need_write = False

        if self.file.exists():
            try:
                data = json.loads(self.file.read_text(encoding="utf-8"))
                if not isinstance(data, dict):
                    data = {}
            except Exception:
                data = {}
        else:
            data = {}
            need_write = True

        for k, it in self.items.items():
            old = it.effective
            if k in data:
                it.set(data[k])
                if it.effective != old:
                    changed = True
            else:
                need_write = True

        return changed, need_write

    def save(self) -> None:
        obj = self.to_values_dict()
        self.file.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.file.with_suffix(".tmp")
        tmp.write_text(json.dumps(obj, ensure_ascii=False, indent=2), encoding="utf-8")
        tmp.replace(self.file)

    def patch(self, partial: Dict[str, Any]) -> bool:
        """对表打补丁；返回是否有有效变化"""
        changed = False
        for k, v in partial.items():
            old = self.items.get(k).effective if k in self.items else None
            self.set(k, v)
            if self.items[k].effective != old:
                changed = True
        return changed
