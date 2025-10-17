from __future__ import annotations
from pathlib import Path
from typing import Dict, Any, Optional, Tuple, List

from backend.config.table import ConfigTable


class ConfigManager:
    def __init__(self, root: Path):
        self.root = Path(root)
        self.tables: Dict[str, ConfigTable] = {}

    def add_table(self, t: ConfigTable) -> "ConfigManager":
        self.tables[t.name] = t
        return self

    def __getitem__(self, tname: str) -> ConfigTable:
        return self.tables[tname]

    def get(self, dotted: str, default: Any = None) -> Any:
        t, _, k = dotted.partition(".")
        tb = self.tables.get(t)
        return default if not tb else tb.get(k, default)

    def set(self, dotted: str, value: Any, *, persist: bool = False) -> None:
        t, _, k = dotted.partition(".")
        tb = self.tables.setdefault(t, ConfigTable(name=t, file=self.root / f"{t}.json"))
        tb.set(k, value)
        if persist:
            tb.save()

    def load_all(self) -> bool:
        any_changed = False
        for t in self.tables.values():
            changed, need_write = t.load_merge()
            any_changed = any_changed or changed
            if need_write:
                t.save()
        return any_changed

    def to_payload(self) -> Dict[str, Dict[str, Any]]:
        return {
            name: t.to_values_dict()
            for name, t in self.tables.items()
            if not (isinstance(name, str) and name == "fuse")
        }

    def to_table_payload(self, name: str) -> Dict[str, Any]:
        tb = self.tables.get(name)
        return tb.to_values_dict() if tb else {}

    def apply_patch(self, edit: Dict[str, Dict[str, Any]]) -> List[Path]:
        """
        前端 patch：{表: {键: 值}}
        返回：本次写入到磁盘的文件列表（用于在 app.py 中标记 recent_writes）
        """
        written: List[Path] = []
        for tname, partial in edit.items():
            tb = self.tables.get(tname)
            if not tb:
                tb = ConfigTable(name=tname, file=self.root / f"{tname}.json")
                self.add_table(tb)
            if tb.patch(partial):
                tb.save()
                written.append(tb.file)
        return written

    def handle_file_change(self, path: Path) -> Tuple[Optional[str], bool]:
        p = Path(path)
        tname = p.stem
        tb = self.tables.get(tname)
        if not tb:
            tb = ConfigTable(name=tname, file=p)
            self.add_table(tb)
        changed, need_write = tb.load_merge()
        if need_write:
            tb.save()
        return tname, (changed or need_write)