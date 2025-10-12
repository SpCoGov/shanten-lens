from __future__ import annotations
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Optional, List
import json, os, tempfile


@dataclass
class ConfigItem:
    name: str
    default: Any
    value: Any = None

    def effective(self) -> Any:
        if self.value is None: return self.default
        if isinstance(self.value, str) and self.value == "": return self.default
        return self.value


@dataclass
class ConfigTable:
    name: str
    items: Dict[str, ConfigItem] = field(default_factory=dict)
    file: Optional[Path] = None

    @classmethod
    def from_defaults(cls, name: str, defaults: Dict[str, Any], file: Path) -> "ConfigTable":
        return cls(name=name, items={k: ConfigItem(k, v, None) for k, v in defaults.items()}, file=file)

    def apply_disk(self, obj: Dict[str, Any]) -> tuple[bool, bool]:
        before = self.to_values_dict()
        need_write = False
        for k, it in self.items.items():
            if k in obj:
                v = obj[k]
                if it.value != v:
                    it.value = v
            else:
                need_write = True
        after = self.to_values_dict()
        changed = (before != after)
        return changed, need_write

    def patch(self, partial: Dict[str, Any]) -> None:
        for k, v in partial.items():
            if k in self.items:
                self.items[k].value = v

    def to_values_dict(self) -> Dict[str, Any]:
        return {k: it.effective() for k, it in self.items.items()}

    def to_disk_dict(self) -> Dict[str, Any]:
        return {k: it.effective() for k, it in self.items.items()}

    def save(self) -> None:
        assert self.file is not None
        self.file.parent.mkdir(parents=True, exist_ok=True)
        data = json.dumps(self.to_disk_dict(), ensure_ascii=False, indent=2)
        with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=str(self.file.parent), delete=False) as tf:
            tf.write(data)
            tmp = tf.name
        os.replace(tmp, self.file)


class ConfigManager:
    def __init__(self, root: Path, defaults: Dict[str, Dict[str, Any]]):
        self.root = Path(root)
        self.tables: Dict[str, ConfigTable] = {}
        self.defaults = defaults
        self.root.mkdir(parents=True, exist_ok=True)

    def table_path(self, name: str) -> Path:
        return self.root.joinpath(f"{name}.json")

    def load_all(self) -> None:
        for tname, dfl in self.defaults.items():
            p = self.table_path(tname)
            table = ConfigTable.from_defaults(tname, dfl, p)
            if p.exists():
                try:
                    obj = json.loads(p.read_text(encoding="utf-8"))
                except Exception:
                    obj = {}
                changed, need_write = table.apply_disk(obj)
                if need_write:
                    table.save()
            else:
                table.save()
            self.tables[tname] = table

    def to_payload(self) -> Dict[str, Dict[str, Any]]:
        return {name: t.to_values_dict() for name, t in self.tables.items()}

    def apply_patch(self, edit: Dict[str, Dict[str, Any]]) -> List[Path]:
        """返回这次被写入的文件路径列表，供 app.py 做自写标记。"""
        written: List[Path] = []
        for tname, sub in edit.items():
            if tname in self.tables and isinstance(sub, dict):
                t = self.tables[tname]
                t.patch(sub)
                t.save()
                if t.file:
                    written.append(t.file)
        return written

    def handle_file_change(self, path: Path) -> tuple[Optional[str], bool]:
        for tname, table in self.tables.items():
            if table.file and Path(path) == table.file:
                try:
                    obj = json.loads(table.file.read_text(encoding="utf-8"))
                except Exception:
                    return None, False
                changed, need_write = table.apply_disk(obj)
                if need_write:
                    table.save()
                return tname, (changed or need_write)
        return None, False
