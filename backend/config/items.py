from __future__ import annotations
from dataclasses import dataclass
from typing import Any, Optional

@dataclass
class ConfigItem:
    name: str
    default: Any
    value: Any = None
    desc: Optional[str] = None
    kind: Optional[str] = None  # "bool" | "number" | "string" | "select" ...

    @property
    def effective(self) -> Any:
        return self.default if self.value is None else self.value

    def set(self, v: Any) -> None:
        # 与默认相同就不用冗余存储
        self.value = None if v == self.default else v