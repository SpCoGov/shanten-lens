import json
import os
import tempfile
from dataclasses import dataclass, asdict, field
from pathlib import Path
from typing import Literal, Optional

from platformdirs import user_data_dir

Language = Literal["zh-CN", "ja-JP", "en-US"]
Theme = Literal["light", "dark", "system"]


@dataclass
class BackendSection:
    host: str = "127.0.0.1"
    port: int = 8787


@dataclass
class AppConfig:
    language: Language = "zh-CN"
    theme: Theme = "system"
    backend: BackendSection = field(default_factory=BackendSection)


DEFAULT = AppConfig()


def default_data_root() -> Path:
    base = Path(user_data_dir("Shanten Lens"))
    return base / "shanten"


def config_path(data_root: Optional[Path]) -> Path:
    root = data_root or default_data_root()
    root.mkdir(parents=True, exist_ok=True)
    return root / "config.json"


def ensure_config_file(p: Path) -> None:
    if not p.exists():
        p.write_text(json.dumps(asdict(DEFAULT), ensure_ascii=False, indent=2), encoding="utf-8")


def load_config(p: Path) -> AppConfig:
    """严格加载：不存在则创建默认；解析失败则抛异常（由调用方决定是否保留旧值）。"""
    ensure_config_file(p)
    obj = json.loads(p.read_text(encoding="utf-8"))
    b = obj.get("backend") or {}
    return AppConfig(
        language=obj.get("language", DEFAULT.language),
        theme=obj.get("theme", DEFAULT.theme),
        backend=BackendSection(
            host=b.get("host", DEFAULT.backend.host),
            port=int(b.get("port", DEFAULT.backend.port))
        ),
    )


def save_config_atomic(p: Path, cfg: AppConfig) -> None:
    """原子写入：写临时文件后 replace，避免读到半截文件。"""
    p.parent.mkdir(parents=True, exist_ok=True)
    data = json.dumps(asdict(cfg), ensure_ascii=False, indent=2)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=str(p.parent), delete=False) as tf:
        tf.write(data)
        tmp_name = tf.name
    os.replace(tmp_name, p)